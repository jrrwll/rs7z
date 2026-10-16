use super::{
    ArchiveBackend, build_index, destination_directory, join, rename_target, validate_name, within,
};
use crate::model::{
    archive::{ArchiveDestination, ArchiveEntry, ArchiveFormat, ArchiveIndex, ArchiveOperation},
    archive::{ArchiveEntryMetadata, ArchiveOptions},
    operations::OperationError,
};
use crate::service::{
    logging,
    operations::{self, ProgressHandle},
    zip,
};
use chrono::{Local, NaiveDateTime, TimeZone};
use std::{
    collections::HashSet,
    fs::{self, File},
    path::Path,
    process::Stdio,
};

pub(super) struct SevenZipBackend;

fn failed(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

impl ArchiveBackend for SevenZipBackend {
    fn read(&self, path: &Path) -> Result<ArchiveIndex, String> {
        self.read_with_options(path, &ArchiveOptions::default())
    }

    fn read_with_options(
        &self,
        path: &Path,
        options: &ArchiveOptions,
    ) -> Result<ArchiveIndex, String> {
        read_index(path, options, ArchiveFormat::SevenZip)
    }

    fn verify_password(
        &self,
        path: &Path,
        password: &str,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError> {
        let mut command = crate::service::seven_zip_cli::command()?;
        command
            .args(["t", "-y"])
            .arg(format!("-p{password}"))
            .arg("--")
            .arg(path);
        operations::run_archive_command(command, progress)
    }

    fn apply(
        &self,
        path: &Path,
        operation: &ArchiveOperation,
        options: &ArchiveOptions,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError> {
        progress.check()?;
        let index = self.read_with_options(path, options)?;
        apply_index(&index, operation, options, progress)
    }
}

pub(super) fn apply_index(
    index: &ArchiveIndex,
    operation: &ArchiveOperation,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let path = &index.path;
    if index.encrypted && zip::password(options).is_none() {
        return Err(failed("Password required for this encrypted archive."));
    }
    let export_only = matches!(
        operation,
        ArchiveOperation::Transfer {
            destination: ArchiveDestination::FileSystem(_),
            move_entries: false,
            ..
        }
    );
    if super::is_split(path) && !export_only {
        return Err(failed(
            "Split archives cannot be modified. Extract and create a new archive.",
        ));
    }
    if options.volume_size.is_some() {
        return Err(failed(
            "Volume size can only be set when creating a new ZIP or 7z archive.",
        ));
    }
    validate_operation(index, operation)?;
    let version = fs::metadata(path).map_err(failed)?;
    // Keep the workspace on cancellation/failure so partial results are never cleaned up.
    let stage = tempfile::Builder::new()
        .prefix(".rs7z-7z-")
        .tempdir_in(path.parent().unwrap_or(Path::new("/")))
        .map_err(failed)?
        .keep();
    logging::detail(format!("7z operation workspace: {}", stage.display()));
    let result = apply_staged(index, operation, options, &stage, &version, progress);
    match &result {
        Ok(()) if !progress.is_cancelled() => {
            logging::detail(format!(
                "Removing completed 7z workspace: {}",
                stage.display()
            ));
            if let Err(error) = fs::remove_dir_all(&stage) {
                logging::error(format!(
                    "Cannot remove completed workspace {}: {error}",
                    stage.display()
                ));
            }
        }
        _ => logging::info(format!(
            "Partial 7z results retained at {}",
            stage.display()
        )),
    }
    result
}

pub(super) fn read_index(
    path: &Path,
    options: &ArchiveOptions,
    format: ArchiveFormat,
) -> Result<ArchiveIndex, String> {
    let mut command = if format == ArchiveFormat::Rar {
        crate::service::seven_zip_cli::rar_command()?
    } else {
        crate::service::seven_zip_cli::command()?
    };
    command.args(["l", "-slt", "-ba"]);
    if format == ArchiveFormat::Zip && !super::is_numbered_zip(path) {
        command.arg("-tzip");
    }
    command.arg(format!("-p{}", zip::password(options).unwrap_or("")));
    command.arg("--").arg(path);
    logging::detail(format!(
        "Reading 7zz metadata: {}",
        zip::command_description(&command)
    ));
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Cannot inspect archive with 7zz: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines() {
        logging::detail(format!("7zz metadata: {line}"));
    }
    for line in stderr.lines() {
        logging::detail(format!("7zz metadata stderr: {line}"));
    }
    if !output.status.success() {
        let detail = format!("{}\n{}", stdout.trim(), stderr.trim());
        if zip::password(options).is_none()
            && detail.to_ascii_lowercase().contains("encrypted archive")
        {
            // Encrypted headers hide the tree until the navigation password is verified.
            return build_index(path, format, true, Vec::new());
        }
        return Err(format!("Cannot read archive: {}", detail.trim()));
    }
    parse_listing(path, format, &stdout)
}

pub(super) fn parse_listing(
    path: &Path,
    format: ArchiveFormat,
    listing: &str,
) -> Result<ArchiveIndex, String> {
    let mut entries = Vec::new();
    let mut encrypted = false;
    for block in listing
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
    {
        let mut fields = std::collections::HashMap::new();
        for line in block.lines() {
            let (key, value) = line
                .split_once(" = ")
                .ok_or("Ambiguous multiline archive entry metadata")?;
            if fields.insert(key, value).is_some() {
                return Err("Ambiguous duplicate archive entry metadata".into());
            }
        }
        let name = fields
            .get("Path")
            .ok_or("Invalid archive entry metadata: missing path")?;
        let attributes = fields.get("Attributes").copied().unwrap_or("");
        let mode = fields.get("Mode").copied().unwrap_or("");
        if attributes
            .split_whitespace()
            .any(|part| part.starts_with('l'))
            || fields
                .get("Symbolic Link")
                .is_some_and(|value| !value.is_empty())
            || fields
                .get("Hard Link")
                .is_some_and(|value| !value.is_empty())
            || fields
                .get("Copy Link")
                .is_some_and(|value| !value.is_empty())
            || attributes
                .split_whitespace()
                .any(|part| part.len() == 10 && !part.starts_with(['d', '-']))
            || (!mode.is_empty() && !mode.starts_with(['d', '-']))
        {
            return Err(format!(
                "Links or special files in archives are not supported: {name}"
            ));
        }
        let directory = fields.get("Folder") == Some(&"+") || attributes.starts_with('D');
        let name = if super::is_tar(format) {
            let mut name = *name;
            while let Some(relative) = name.strip_prefix("./") {
                name = relative;
            }
            if directory && (name == "." || name.is_empty()) {
                continue;
            }
            name
        } else {
            *name
        };
        let entry_encrypted = match fields.get("Encrypted").copied() {
            Some("+") => Some(true),
            Some("-") => Some(false),
            _ => None,
        };
        encrypted |= entry_encrypted == Some(true);
        let size = fields
            .get("Size")
            .ok_or("Invalid 7z entry metadata: missing size")?
            .parse::<u64>()
            .map_err(|_| "Invalid 7z entry size")?;
        let timestamp = |key| {
            fields
                .get(key)
                .and_then(|value| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f").ok())
                .and_then(|time| Local.from_local_datetime(&time).single())
                .map(Into::into)
        };
        entries.push(ArchiveEntry {
            name: name.trim_end_matches('/').to_owned(),
            directory,
            size,
            modified: timestamp("Modified"),
            created: timestamp("Created"),
            accessed: timestamp("Accessed"),
            metadata: ArchiveEntryMetadata {
                // TAR's packed size describes the inner TAR record, not the
                // individual entry's share of the outer compressed stream.
                compressed_size: (!super::is_tar(format))
                    .then(|| {
                        fields
                            .get("Packed Size")
                            .and_then(|value| value.parse().ok())
                    })
                    .flatten(),
                encrypted: entry_encrypted,
                comment: fields.get("Comment").map(|value| (*value).to_owned()),
                crc32: fields.get("CRC").and_then(|value| {
                    (value.len() == 8)
                        .then(|| u32::from_str_radix(value, 16).ok())
                        .flatten()
                }),
            },
        });
    }
    build_index(path, format, encrypted, entries)
}

fn roots(index: &ArchiveIndex, entries: &[String]) -> Result<Vec<String>, OperationError> {
    if entries.is_empty() {
        return Err(failed("Choose at least one archive entry."));
    }
    for entry in entries {
        validate_name(entry)?;
        if !index.entries.iter().any(|item| &item.name == entry) {
            return Err(failed(format!("Archive entry no longer exists: {entry}")));
        }
    }
    let mut entries = entries.to_owned();
    entries.sort();
    entries.dedup();
    let mut roots = Vec::new();
    for entry in entries {
        if !roots.iter().any(|root: &String| within(&entry, root)) {
            roots.push(entry);
        }
    }
    Ok(roots)
}

fn target_available(index: &ArchiveIndex, target: &str) -> Result<(), OperationError> {
    if index.entries.iter().any(|entry| entry.name == target) {
        return Err(failed(format!("Destination already exists: /{target}")));
    }
    Ok(())
}

pub(super) fn validate_operation(
    index: &ArchiveIndex,
    operation: &ArchiveOperation,
) -> Result<(), OperationError> {
    match operation {
        ArchiveOperation::Add { sources, directory } => {
            let directory = destination_directory(index, directory)?;
            if sources.is_empty() {
                return Err(failed("Choose files or folders to add."));
            }
            let mut targets = HashSet::new();
            for source in sources {
                let source = fs::canonicalize(source).map_err(failed)?;
                if index.path.starts_with(&source) {
                    return Err(failed(
                        "Cannot add the archive or a folder containing it to itself.",
                    ));
                }
                let name = source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| failed("Source name must be UTF-8."))?;
                let target = join(&directory, name);
                validate_name(&target)?;
                target_available(index, &target)?;
                if !targets.insert(target) {
                    return Err(failed("Source names are duplicated."));
                }
            }
        }
        ArchiveOperation::Delete { entries } => {
            roots(index, entries)?;
        }
        ArchiveOperation::Rename { entry, name } => {
            rename_target(index, entry, name)?;
        }
        ArchiveOperation::Transfer {
            entries,
            destination,
            ..
        } => {
            let roots = roots(index, entries)?;
            let mut targets = HashSet::new();
            match destination {
                ArchiveDestination::Directory(directory) => {
                    let directory = destination_directory(index, directory)?;
                    for root in roots {
                        if within(&directory, &root) {
                            return Err(failed(
                                "Destination must be outside the selected directory.",
                            ));
                        }
                        let target = join(&directory, root.rsplit('/').next().unwrap());
                        target_available(index, &target)?;
                        if !targets.insert(target) {
                            return Err(failed("Destination names are duplicated."));
                        }
                    }
                }
                ArchiveDestination::FileSystem(folder) => {
                    if !folder.is_dir() {
                        return Err(failed("Choose an existing destination folder."));
                    }
                    for root in roots {
                        let target = folder.join(root.rsplit('/').next().unwrap());
                        if target.symlink_metadata().is_ok()
                            || !targets.insert(target.display().to_string())
                        {
                            return Err(failed(format!(
                                "Destination already exists or is duplicated for {root}"
                            )));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn apply_staged(
    index: &ArchiveIndex,
    operation: &ArchiveOperation,
    options: &ArchiveOptions,
    stage: &Path,
    version: &fs::Metadata,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let contents = stage.join("contents");
    fs::create_dir(&contents).map_err(failed)?;
    let mut extract = crate::service::seven_zip_cli::command()?;
    extract
        .args(["x", "-y", "-spd"])
        .arg(format!("-o{}", contents.display()));
    zip::read_command_options(&mut extract, options, index.format, &index.path);
    extract.arg("--").arg(&index.path);
    operations::run_archive_command(extract, progress)?;
    progress.check()?;
    if !apply_to_directory(index, operation, &contents, progress)? {
        return Ok(());
    }
    progress.check()?;
    let updated = stage.join("updated.7z");
    let mut command = crate::service::seven_zip_cli::command()?;
    command.current_dir(&contents).args([
        "a",
        if index.format == ArchiveFormat::Zip {
            "-tzip"
        } else {
            "-t7z"
        },
        "-y",
        "-spd",
    ]);
    zip::command_options(&mut command, options, true, index.format);
    if index.format == ArchiveFormat::SevenZip {
        zip::seven_zip_command_options(&mut command, options)?;
    } else if options.seven_zip != Default::default() {
        return Err(failed("7z compression settings require a 7z archive."));
    }
    command.arg("--").arg(&updated).arg(".");
    operations::run_archive_command(command, progress)?;
    progress.check()?;
    let current = fs::metadata(&index.path).map_err(failed)?;
    if current.len() != version.len()
        || current.modified().map_err(failed)? != version.modified().map_err(failed)?
    {
        return Err(failed(
            "Archive changed during the operation; the original was not replaced.",
        ));
    }
    fs::set_permissions(&updated, version.permissions()).map_err(failed)?;
    File::open(&updated)
        .map_err(failed)?
        .sync_all()
        .map_err(failed)?;
    progress.check()?;
    fs::rename(&updated, &index.path).map_err(failed)?;
    progress.update(100., "Archive updated");
    Ok(())
}

/// Apply validated changes to a private workspace; false means export only.
pub(super) fn apply_to_directory(
    index: &ArchiveIndex,
    operation: &ArchiveOperation,
    contents: &Path,
    progress: &ProgressHandle,
) -> Result<bool, OperationError> {
    match operation {
        ArchiveOperation::Add { sources, directory } => {
            let directory = destination_directory(index, directory)?;
            let destination = contents.join(directory);
            fs::create_dir_all(&destination).map_err(failed)?;
            for (ix, source) in sources.iter().enumerate() {
                let target = destination.join(
                    source
                        .file_name()
                        .ok_or_else(|| failed("Invalid source name"))?,
                );
                operations::copy_or_move_with_progress(
                    std::slice::from_ref(source),
                    &target,
                    false,
                    &progress.phase(
                        ix as f32 * 100. / sources.len() as f32,
                        (ix + 1) as f32 * 100. / sources.len() as f32,
                    ),
                )?;
            }
        }
        ArchiveOperation::Delete { entries } => {
            let paths = roots(index, entries)?
                .iter()
                .map(|entry| contents.join(entry))
                .collect::<Vec<_>>();
            operations::delete_many_with_progress(&paths, progress)?;
        }
        ArchiveOperation::Rename { entry, name } => {
            operations::rename_with_progress(&contents.join(entry), name, progress)?;
        }
        ArchiveOperation::Transfer {
            entries,
            destination,
            move_entries,
        } => {
            let roots = roots(index, entries)?;
            let folder = match destination {
                ArchiveDestination::Directory(directory) => {
                    contents.join(destination_directory(index, directory)?)
                }
                ArchiveDestination::FileSystem(folder) => {
                    fs::canonicalize(folder).map_err(failed)?
                }
            };
            fs::create_dir_all(&folder).map_err(failed)?;
            let count = roots.len();
            let export = matches!(destination, ArchiveDestination::FileSystem(_));
            for (ix, root) in roots.into_iter().enumerate() {
                let source = contents.join(&root);
                let target = folder.join(root.rsplit('/').next().unwrap());
                let phase = progress.phase(
                    ix as f32 * 100. / count as f32,
                    (ix + 1) as f32 * 100. / count as f32,
                );
                let transfer_progress = if export && *move_entries {
                    phase.phase(0., 95.)
                } else {
                    phase.clone()
                };
                // System destinations can be on another filesystem. Remove only
                // the staged source after copying; the archive commits after repacking.
                operations::copy_or_move_with_progress(
                    std::slice::from_ref(&source),
                    &target,
                    *move_entries && !export,
                    &transfer_progress,
                )?;
                if export && *move_entries {
                    operations::delete_many_with_progress(&[source], &phase.phase(95., 100.))?;
                }
            }
            if export && !move_entries {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
