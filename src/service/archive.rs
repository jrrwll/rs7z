mod compression;
pub(super) mod rar;
mod read_only;
mod seven_zip;
pub(super) mod stream;
pub(super) mod tar;
mod zip;

use super::{logging, operations::ProgressHandle};
use crate::model::{
    archive::{ArchiveEntry, ArchiveFormat, ArchiveIndex, ArchiveOperation},
    archive::{ArchiveOptions, StreamFormat},
    file::{FileItem, FileKind},
    operations::OperationError,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const ZIP_EXTENSIONS: &[&str] = &[
    "zip", "z01", "zipx", "jar", "xpi", "odt", "ods", "docx", "xlsx", "epub", "ipa", "apk", "appx",
];

pub const OPEN_EXTENSIONS: &[&str] = &[
    "zip", "z01", "zipx", "jar", "xpi", "odt", "ods", "docx", "xlsx", "epub", "ipa", "apk", "appx",
    "7z", "001", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "lzma", "tlz", "zst", "tzst", "z",
    "Z", "taz", "rar", "gzip", "bzip2", "zstd",
];

pub const SAVE_EXTENSIONS: &[&str] = &[
    "zip", "zipx", "jar", "xpi", "odt", "ods", "docx", "xlsx", "epub", "ipa", "apk", "appx", "7z",
    "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "lzma", "tlz", "zst", "tzst",
];

pub fn is_extract_only(path: &Path) -> bool {
    matches!(
        format(path),
        Some(ArchiveFormat::Rar | ArchiveFormat::Stream(_))
    )
}

pub fn extract_only_message(path: &Path) -> String {
    format!("{} supports extraction only.", format_name(path))
}

pub fn is_stream(path: &Path) -> bool {
    matches!(format(path), Some(ArchiveFormat::Stream(_)))
}

/// Derive the single output name from the source, never from stream metadata.
pub fn stream_output_name(path: &Path) -> Result<String, String> {
    if !is_stream(path) {
        return Err(format!("Unsupported standalone stream: {}", path.display()));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Source name must be UTF-8.")?;
    let (name, _) = name.rsplit_once('.').ok_or("Missing stream suffix.")?;
    super::operations::validate_item_name(name)?;
    Ok(name.to_owned())
}

fn sibling_with_extension(path: &Path, extension: &str) -> Option<PathBuf> {
    let expected = path.with_extension(extension);
    if expected.is_file() {
        return Some(expected);
    }
    let name = expected.file_name()?.to_str()?;
    std::fs::read_dir(
        path.parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .ok()?
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .find(|candidate| {
        candidate.is_file()
            && candidate
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
    })
}

pub fn is_split_zip(path: &Path) -> bool {
    if is_numbered_zip(path) {
        return true;
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("z01") => true,
        Some(extension) if extension.eq_ignore_ascii_case("zip") => {
            sibling_with_extension(path, "z01").is_some()
        }
        _ => false,
    }
}

/// 7zz creates byte-split ZIP volumes; only the .001 entry point is opened.
pub(super) fn is_numbered_zip(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".zip.001"))
}

/// ZIP split sets are opened through the final .zip volume containing the directory.
pub fn resolve_source(path: &Path) -> Result<PathBuf, String> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("z01"))
    {
        sibling_with_extension(path, "zip").ok_or_else(|| {
            "The matching .zip volume is missing. Keep every ZIP volume in the same folder.".into()
        })
    } else {
        Ok(path.to_owned())
    }
}

/// Format-specific I/O stays behind this seam; the browser uses archive entries only.
trait ArchiveBackend {
    fn read(&self, path: &Path) -> Result<ArchiveIndex, String>;
    fn read_with_options(&self, path: &Path, _: &ArchiveOptions) -> Result<ArchiveIndex, String> {
        self.read(path)
    }
    fn verify_password(
        &self,
        path: &Path,
        password: &str,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError>;
    fn apply(
        &self,
        path: &Path,
        operation: &ArchiveOperation,
        options: &ArchiveOptions,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError>;
}

fn backend(path: &Path) -> Result<&'static dyn ArchiveBackend, String> {
    match format(path) {
        Some(ArchiveFormat::Zip) if is_split_zip(path) => Ok(&read_only::ReadOnlyBackend),
        Some(ArchiveFormat::Zip) => Ok(&zip::ZipBackend),
        Some(ArchiveFormat::Rar) => Ok(&rar::RarBackend),
        Some(ArchiveFormat::Stream(_)) => Err(extract_only_message(path)),
        Some(ArchiveFormat::SevenZip) => Ok(&seven_zip::SevenZipBackend),
        Some(
            ArchiveFormat::TarGzip
            | ArchiveFormat::TarBzip2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarLzma
            | ArchiveFormat::TarZstd
            | ArchiveFormat::TarCompress,
        ) => Ok(&tar::TarBackend),
        _ => Err(format!("Unsupported archive format: {}", path.display())),
    }
}

pub fn is_tar(format: ArchiveFormat) -> bool {
    matches!(
        format,
        ArchiveFormat::TarGzip
            | ArchiveFormat::TarBzip2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarLzma
            | ArchiveFormat::TarZstd
            | ArchiveFormat::TarCompress
    )
}

pub fn is_read_only(path: &Path) -> bool {
    is_split(path)
        || matches!(
            format(path),
            Some(ArchiveFormat::TarCompress | ArchiveFormat::Rar | ArchiveFormat::Stream(_))
        )
}

pub fn supports_password(format: ArchiveFormat) -> bool {
    matches!(
        format,
        ArchiveFormat::Zip | ArchiveFormat::SevenZip | ArchiveFormat::Rar
    )
}

pub fn extension(format: ArchiveFormat) -> &'static str {
    match format {
        ArchiveFormat::Zip => "zip",
        ArchiveFormat::SevenZip => "7z",
        ArchiveFormat::TarGzip => "tar.gz",
        ArchiveFormat::TarBzip2 => "tar.bz2",
        ArchiveFormat::TarXz => "tar.xz",
        ArchiveFormat::TarLzma => "tar.lzma",
        ArchiveFormat::TarZstd => "tar.zst",
        ArchiveFormat::TarCompress => "tar.Z",
        ArchiveFormat::Rar => "rar",
        ArchiveFormat::Stream(format) => format.extension(),
    }
}

/// Strip the complete archive suffix, including aliases and split-volume suffixes.
pub fn extraction_directory(path: &Path) -> PathBuf {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return path.with_extension("");
    };
    let lower = name.to_ascii_lowercase();
    let suffix = [
        ".zip.001",
        ".7z.001",
        ".tar.lzma",
        ".tar.zst",
        ".tar.bz2",
        ".tar.gz",
        ".tar.xz",
        ".tar.z",
        ".tbz2",
        ".tgz",
        ".tbz",
        ".txz",
        ".tlz",
        ".tzst",
        ".taz",
        ".zipx",
        ".zip",
        ".z01",
        ".jar",
        ".xpi",
        ".odt",
        ".ods",
        ".docx",
        ".xlsx",
        ".epub",
        ".ipa",
        ".apk",
        ".appx",
        ".rar",
        ".7z",
    ]
    .into_iter()
    .find(|suffix| lower.ends_with(suffix));
    match suffix {
        Some(suffix) if name.len() > suffix.len() => {
            path.with_file_name(&name[..name.len() - suffix.len()])
        }
        _ => path.with_extension(""),
    }
}

pub fn with_format(path: &Path, selected: ArchiveFormat) -> PathBuf {
    if format(path) == Some(selected) && !is_split(path) {
        return path.to_owned();
    }
    if format(path).is_some() {
        let mut name = extraction_directory(path).into_os_string();
        name.push(".");
        name.push(extension(selected));
        PathBuf::from(name)
    } else {
        path.with_extension(extension(selected))
    }
}

pub fn format(path: &Path) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".zip.001")
        || name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| ZIP_EXTENSIONS.contains(&extension))
    {
        Some(ArchiveFormat::Zip)
    } else if name.ends_with(".7z") || name.ends_with(".7z.001") {
        Some(ArchiveFormat::SevenZip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(ArchiveFormat::TarGzip)
    } else if [".tar.bz2", ".tbz", ".tbz2"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        Some(ArchiveFormat::TarBzip2)
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        Some(ArchiveFormat::TarXz)
    } else if name.ends_with(".tar.lzma") || name.ends_with(".tlz") {
        Some(ArchiveFormat::TarLzma)
    } else if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        Some(ArchiveFormat::TarZstd)
    } else if name.ends_with(".tar.z") || name.ends_with(".taz") {
        Some(ArchiveFormat::TarCompress)
    } else if name.ends_with(".rar") {
        Some(ArchiveFormat::Rar)
    } else {
        let (stem, extension) = name.rsplit_once('.')?;
        // Only the explicit TAR combinations above are supported. Do not
        // accidentally route an unknown compressed TAR to single-file extraction.
        if stem.ends_with(".tar") {
            return None;
        }
        let stream = match extension {
            "gz" | "gzip" => StreamFormat::Gzip,
            "bz2" | "bzip2" => StreamFormat::Bzip2,
            "xz" => StreamFormat::Xz,
            "zst" | "zstd" => StreamFormat::Zstd,
            "lzma" => StreamFormat::Lzma,
            "z" => StreamFormat::Compress,
            _ => return None,
        };
        Some(ArchiveFormat::Stream(stream))
    }
}

pub fn is_split(path: &Path) -> bool {
    is_split_zip(path)
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".7z.001"))
}

pub fn format_name(path: &Path) -> &'static str {
    match format(path) {
        Some(ArchiveFormat::Zip) if is_split_zip(path) => "Split ZIP archive",
        Some(ArchiveFormat::Zip) => "ZIP archive",
        Some(ArchiveFormat::Rar) => "RAR archive",
        Some(ArchiveFormat::SevenZip) if is_split(path) => "Split 7z archive",
        Some(ArchiveFormat::SevenZip) => "7z archive",
        Some(ArchiveFormat::TarGzip) => "TAR / gzip archive",
        Some(ArchiveFormat::TarBzip2) => "TAR / bzip2 archive",
        Some(ArchiveFormat::TarXz) => "TAR / xz archive",
        Some(ArchiveFormat::TarLzma) => "TAR / LZMA archive",
        Some(ArchiveFormat::TarZstd) => "TAR / Zstandard archive",
        Some(ArchiveFormat::TarCompress) => "TAR / compress archive",
        Some(ArchiveFormat::Stream(stream)) => match stream {
            StreamFormat::Gzip => "gzip stream",
            StreamFormat::Bzip2 => "bzip2 stream",
            StreamFormat::Xz => "XZ stream",
            StreamFormat::Zstd => "Zstandard stream",
            StreamFormat::Lzma => "LZMA stream",
            StreamFormat::Compress => "Unix compress stream",
        },
        _ => "Archive",
    }
}

pub fn read_with_options(path: &Path, options: &ArchiveOptions) -> Result<ArchiveIndex, String> {
    backend(path)?.read_with_options(path, options)
}

pub fn read(path: &Path) -> Result<ArchiveIndex, String> {
    backend(path)?.read(path)
}

/// Authenticate encrypted entries without creating any filesystem output.
pub fn verify_password(
    path: &Path,
    password: &str,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    logging::info(format!("Verifying archive password: {}", path.display()));
    progress.check()?;
    let result = backend(path)?.verify_password(path, password, progress);
    match &result {
        Ok(()) => logging::info("Archive password verified"),
        Err(OperationError::Cancelled) => logging::info("Archive password verification cancelled"),
        Err(OperationError::Failed(error)) => logging::error(error),
    }
    result
}

pub fn apply(
    path: &Path,
    operation: &ArchiveOperation,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    apply_with_options(path, operation, &ArchiveOptions::default(), progress)
}

pub fn apply_with_options(
    path: &Path,
    operation: &ArchiveOperation,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    logging::info(format!("Archive operation: {}", path.display()));
    logging::detail(format!("{operation:?}"));
    progress.check()?;
    let result = backend(path)?.apply(path, operation, options, progress);
    match &result {
        Ok(()) => logging::info("Archive operation complete"),
        Err(OperationError::Cancelled) => {
            logging::info("Archive operation cancelled; partial results remain")
        }
        Err(OperationError::Failed(error)) => logging::error(error),
    }
    result
}

/// Resolve the archive prefix of a virtual path such as /tmp/a.zip/docs.
pub fn archive_path(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .map(|ancestor| ancestor.components().collect::<PathBuf>())
        .find(|ancestor| ancestor.is_file() && format(ancestor).is_some())
}

pub fn directory_name(archive: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(archive)
        .map_err(|_| "Directory is outside the archive")?;
    normalize_directory(relative.to_str().ok_or("Archive paths must be UTF-8")?)
}

/// Leading slash denotes archive root in the UI. Entry names themselves are relative.
pub fn normalize_directory(name: &str) -> Result<String, String> {
    let name = name.trim_matches('/');
    if name.is_empty() {
        return Ok(String::new());
    }
    validate_name(name)?;
    Ok(name.to_owned())
}

/// Resolve a destination from the archive root, allowing safe new directories.
/// Existing directories remain selectable even when their names use other characters.
pub fn destination_directory(index: &ArchiveIndex, name: &str) -> Result<String, String> {
    if name == "/" {
        return Ok(String::new());
    }
    let directory = name.strip_prefix('/').unwrap_or(name);
    let directory = directory.strip_suffix('/').unwrap_or(directory);
    validate_name(directory)?;
    if let Some(file) = index
        .entries
        .iter()
        .find(|entry| !entry.directory && within(directory, &entry.name))
    {
        return Err(format!(
            "A file blocks the destination directory: /{}",
            file.name
        ));
    }
    if index
        .entries
        .iter()
        .any(|entry| entry.directory && entry.name == directory)
    {
        return Ok(directory.to_owned());
    }
    if directory
        .split('/')
        .any(|part| part.chars().all(|ch| ch == ' ') || part.chars().all(|ch| ch == '.'))
    {
        return Err("Directory names cannot be blank or contain only dots.".into());
    }
    if !directory
        .chars()
        .all(|ch| ch.is_alphanumeric() || " .-_+=/@#".contains(ch))
    {
        return Err(
            "Use letters, numbers, spaces, or . - _ + = @ #, with / between directories.".into(),
        );
    }
    Ok(directory.to_owned())
}

pub(super) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', '\0', ':'])
        || name.chars().any(char::is_control)
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("Unsafe or unsupported archive path: {name:?}"));
    }
    Ok(())
}

pub fn rename_target(index: &ArchiveIndex, entry: &str, name: &str) -> Result<String, String> {
    super::operations::validate_item_name(name)?;
    validate_name(entry)?;
    if !index.entries.iter().any(|item| item.name == entry) {
        return Err(format!("Archive entry no longer exists: {entry}"));
    }
    let parent = entry.rsplit_once('/').map_or("", |(parent, _)| parent);
    let target = join(parent, name);
    if target == entry {
        return Err("Enter a different name.".into());
    }
    if index.entries.iter().any(|item| item.name == target) {
        return Err(format!("Destination already exists: /{target}"));
    }
    Ok(target)
}

pub(super) fn within(name: &str, parent: &str) -> bool {
    name == parent
        || name
            .strip_prefix(parent)
            .is_some_and(|tail| tail.starts_with('/'))
}

pub(super) fn join(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}

pub fn directories(index: &ArchiveIndex) -> Vec<String> {
    let mut result = vec![String::new()];
    result.extend(
        index
            .entries
            .iter()
            .filter(|entry| entry.directory)
            .map(|entry| entry.name.clone()),
    );
    result
}

pub fn children(index: &ArchiveIndex, directory: &str) -> Result<Vec<FileItem>, String> {
    let directory = normalize_directory(directory)?;
    if !directory.is_empty()
        && !index
            .entries
            .iter()
            .any(|e| e.directory && e.name == directory)
    {
        return Err(format!("Archive directory does not exist: /{directory}"));
    }
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    let mut files: Vec<_> = index
        .entries
        .iter()
        .filter_map(|entry| {
            let name = entry.name.strip_prefix(&prefix)?;
            if name.is_empty() || name.contains('/') {
                return None;
            }
            Some(FileItem {
                path: index.path.join(&entry.name),
                name: name.to_owned(),
                kind: if entry.directory {
                    FileKind::Directory
                } else {
                    FileKind::Other
                },
                size: entry.size,
                modified: entry.modified,
            })
        })
        .collect();
    files.sort_by(|a, b| {
        (a.kind != FileKind::Directory)
            .cmp(&(b.kind != FileKind::Directory))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(files)
}

pub(super) fn build_index(
    path: &Path,
    format: ArchiveFormat,
    encrypted: bool,
    entries: Vec<ArchiveEntry>,
) -> Result<ArchiveIndex, String> {
    let mut tree: BTreeMap<String, ArchiveEntry> = BTreeMap::new();
    for entry in entries {
        validate_name(&entry.name)?;
        if let Some(previous) = tree.get(&entry.name)
            && (!previous.directory || !entry.directory)
        {
            return Err(format!("Ambiguous archive entry: {}", entry.name));
        }
        let mut parent = entry.name.as_str();
        while let Some((name, _)) = parent.rsplit_once('/') {
            if tree.get(name).is_some_and(|entry| !entry.directory) {
                return Err(format!("Archive file is also used as a directory: {name}"));
            }
            tree.entry(name.to_owned()).or_insert_with(|| ArchiveEntry {
                name: name.to_owned(),
                directory: true,
                size: 0,
                modified: None,
            });
            parent = name;
        }
        tree.insert(entry.name.clone(), entry);
    }
    Ok(ArchiveIndex {
        read_only: is_read_only(path),
        path: path.to_owned(),
        format,
        encrypted,
        entries: tree.into_values().collect(),
    })
}
