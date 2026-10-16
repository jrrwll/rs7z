use crate::model::{
    archive::{ArchiveDestination, ArchiveOperation, ArchiveOptions},
    operations::OperationError,
};
use crate::service::{archive, operations, seven_zip_cli};
use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::PathBuf,
};

use super::operations::ProgressHandle;

const ALL_NAMES: &[&str] = &[
    "CRC32", "CRC64", "SHA256", "SHA1", "BLAKE2sp", "MD5", "XXH64", "SHA384", "SHA512", "SHA3-256",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    Crc32,
    Crc64,
    Xxh64,
    Sha1,
    Sha256,
    Blake2sp,
    Md5,
    All,
}

impl HashAlgorithm {
    pub const MENU: &[(Self, &str)] = &[
        (Self::Crc32, "CRC-32"),
        (Self::Crc64, "CRC-64"),
        (Self::Xxh64, "XXH64"),
        (Self::Sha1, "SHA-1"),
        (Self::Sha256, "SHA-256"),
        (Self::Blake2sp, "BLAKE2sp"),
        (Self::Md5, "MD5"),
        (Self::All, "*"),
    ];

    fn switch(self) -> &'static str {
        match self {
            Self::Crc32 => "-scrcCRC32",
            Self::Crc64 => "-scrcCRC64",
            Self::Xxh64 => "-scrcXXH64",
            Self::Sha1 => "-scrcSHA1",
            Self::Sha256 => "-scrcSHA256",
            Self::Blake2sp => "-scrcBLAKE2SP",
            Self::Md5 => "-scrcMD5",
            Self::All => "-scrc*",
        }
    }

    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Crc32 => &ALL_NAMES[0..1],
            Self::Crc64 => &ALL_NAMES[1..2],
            Self::Sha256 => &ALL_NAMES[2..3],
            Self::Sha1 => &ALL_NAMES[3..4],
            Self::Blake2sp => &ALL_NAMES[4..5],
            Self::Md5 => &ALL_NAMES[5..6],
            Self::Xxh64 => &ALL_NAMES[6..7],
            Self::All => ALL_NAMES,
        }
    }
}

pub enum HashTarget {
    FileSystem(Vec<PathBuf>),
    Archive {
        path: PathBuf,
        entries: Vec<String>,
        password: Option<String>,
    },
}

pub struct HashResult {
    pub label: String,
    pub values: Vec<(&'static str, String)>,
}

impl HashResult {
    pub fn text(&self) -> String {
        let mut text = self.label.clone();
        for (name, value) in &self.values {
            text.push('\n');
            text.push_str(name);
            text.push_str(": ");
            text.push_str(value);
        }
        text
    }
}

fn failed(message: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(message.to_string())
}

fn parse_hashes(
    output: &mut File,
    algorithm: HashAlgorithm,
) -> Result<Vec<(&'static str, String)>, OperationError> {
    output.seek(SeekFrom::Start(0)).map_err(failed)?;
    let mut found = std::collections::HashMap::new();
    for line in BufReader::new(output).lines() {
        let line = line.map_err(failed)?;
        if let Some((name, value)) = line.split_once("for data:") {
            let name = name.trim();
            let value = value.trim();
            let (digest, count) = value.split_once('-').unwrap_or((value, ""));
            if ALL_NAMES.contains(&name)
                && !digest.is_empty()
                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                && (!value.contains('-')
                    || (count.len() == 8 && count.bytes().all(|byte| byte.is_ascii_hexdigit())))
            {
                found.insert(name.to_owned(), value.to_ascii_uppercase());
            }
        }
    }
    algorithm
        .names()
        .iter()
        .map(|&name| {
            found
                .remove(name)
                .map(|value| (name, value))
                .ok_or_else(|| failed(format!("7zz did not return a {name} checksum.")))
        })
        .collect()
}

pub fn calculate(
    target: HashTarget,
    algorithm: HashAlgorithm,
    progress: &ProgressHandle,
) -> Result<HashResult, OperationError> {
    progress.check()?;
    let mut staged = None;
    let (paths, label) = match target {
        HashTarget::FileSystem(paths) => {
            if paths.is_empty() {
                return Err(failed("Choose at least one file or folder."));
            }
            let label = if paths.len() == 1 {
                paths[0].display().to_string()
            } else {
                format!("{} selected items", paths.len())
            };
            (paths, label)
        }
        HashTarget::Archive {
            path,
            entries,
            password,
        } => {
            if entries.is_empty() {
                return Err(failed("Choose at least one archive entry."));
            }
            let directory = tempfile::tempdir().map_err(failed)?;
            let options = ArchiveOptions {
                password,
                ..Default::default()
            };
            let count = entries.len();
            let mut paths = Vec::with_capacity(count);
            for (index, entry) in entries.iter().enumerate() {
                progress.check()?;
                let destination = directory.path().join(index.to_string());
                std::fs::create_dir(&destination).map_err(failed)?;
                archive::apply_with_options(
                    &path,
                    &ArchiveOperation::Transfer {
                        entries: vec![entry.clone()],
                        destination: ArchiveDestination::FileSystem(destination.clone()),
                        move_entries: false,
                    },
                    &options,
                    &progress.phase(
                        index as f32 * 70. / count as f32,
                        (index + 1) as f32 * 70. / count as f32,
                    ),
                )?;
                paths.push(destination.join(entry.rsplit('/').next().unwrap_or(entry)));
            }
            let label = if entries.len() == 1 {
                format!("{} / {}", path.display(), entries[0])
            } else {
                format!("{} / {} selected entries", path.display(), entries.len())
            };
            staged = Some(directory);
            (paths, label)
        }
    };
    let mut output = tempfile::tempfile().map_err(failed)?;
    let mut command = seven_zip_cli::command().map_err(failed)?;
    command.args(["h", algorithm.switch(), "-spd", "--"]);
    command.args(&paths);
    let hash_progress = if staged.is_some() {
        progress.phase(70., 100.)
    } else {
        progress.clone()
    };
    operations::run_archive_command_capture_to_file(
        command,
        output.try_clone().map_err(failed)?,
        &hash_progress,
    )?;
    progress.check()?;
    let values = parse_hashes(&mut output, algorithm)?;
    Ok(HashResult { label, values })
}
