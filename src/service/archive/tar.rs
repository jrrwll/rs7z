use super::{ArchiveBackend, build_index, seven_zip};
use crate::model::archive::{
    ArchiveDestination, ArchiveFormat, ArchiveIndex, ArchiveOperation, ArchiveOptions, TarFormat,
};
use crate::model::operations::OperationError;
use crate::service::{
    logging,
    operations::{self, ProgressHandle},
    zip,
};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

pub(super) struct TarBackend;

fn failed(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

fn format(path: &Path) -> Result<ArchiveFormat, OperationError> {
    super::format(path)
        .filter(|format| super::is_tar(*format))
        .ok_or_else(|| failed("Choose a supported TAR archive."))
}

fn codec(format: ArchiveFormat) -> &'static str {
    match format {
        ArchiveFormat::TarGzip => "-tgzip",
        ArchiveFormat::TarBzip2 => "-tbzip2",
        ArchiveFormat::TarXz => "-txz",
        ArchiveFormat::TarCompress => "-tZ",
        _ => unreachable!("compressed TAR format required"),
    }
}

fn validate_options(options: &ArchiveOptions) -> Result<(), OperationError> {
    if zip::password(options).is_some() {
        return Err(failed("Compressed TAR archives do not support passwords."));
    }
    if options.volume_size.is_some() {
        return Err(failed(
            "Volume size is supported only for new ZIP or 7z archives.",
        ));
    }
    if options.seven_zip != Default::default() {
        return Err(failed("7z compression settings require a 7z archive."));
    }
    Ok(())
}

/// Only successful operations remove their private staging files. Cancellation
/// and failures preserve all partial results, as with other archive backends.
fn workspace<T>(
    parent: Option<&Path>,
    progress: &ProgressHandle,
    work: impl FnOnce(&Path) -> Result<T, OperationError>,
) -> Result<T, OperationError> {
    progress.check()?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".rs7z-tar-");
    let temporary = match parent {
        Some(parent) => builder.tempdir_in(parent),
        None => builder.tempdir(),
    }
    .map_err(failed)?
    .keep();
    logging::detail(format!("TAR operation workspace: {}", temporary.display()));
    let result = work(&temporary);
    if result.is_ok() && !progress.is_cancelled() {
        logging::detail(format!(
            "Removing completed TAR workspace: {}",
            temporary.display()
        ));
        if let Err(error) = fs::remove_dir_all(&temporary) {
            logging::error(format!(
                "Cannot remove completed workspace {}: {error}",
                temporary.display()
            ));
        }
    } else {
        logging::info(format!(
            "Partial TAR results retained at {}",
            temporary.display()
        ));
    }
    progress.check()?;
    result
}

fn unpack_layer(
    path: &Path,
    stage: &Path,
    progress: &ProgressHandle,
) -> Result<PathBuf, OperationError> {
    progress.check()?;
    let tar = stage.join("contents.tar");
    let format = format(path)?;
    if format == ArchiveFormat::Tar {
        return Ok(path.to_path_buf());
    }
    if matches!(format, ArchiveFormat::TarLzma | ArchiveFormat::TarZstd) {
        super::compression::decode(path, &tar, format, progress)?;
        return Ok(tar);
    }
    // stdout is the binary stream; never trust or extract the outer filename.
    let mut command = crate::service::seven_zip_cli::command()?;
    command
        .args(["x", codec(format), "-so", "-y", "--"])
        .arg(path);
    operations::run_archive_command_to_file(
        command,
        File::create(&tar).map_err(failed)?,
        progress,
    )?;
    progress.check()?;
    Ok(tar)
}

fn read_tar(
    path: &Path,
    tar: &Path,
    stage: &Path,
    progress: &ProgressHandle,
) -> Result<ArchiveIndex, OperationError> {
    progress.check()?;
    let mut command = crate::service::seven_zip_cli::command()?;
    command.args(["l", "-ttar", "-slt", "-ba", "--"]).arg(tar);
    logging::detail(format!(
        "Reading TAR metadata: {}",
        zip::command_description(&command)
    ));
    // File-backed metadata avoids full pipes while keeping cancellation responsive.
    let listing = stage.join("contents.listing");
    let errors = stage.join("contents.errors");
    let mut child = command
        .stdin(Stdio::null())
        .stdout(File::create(&listing).map_err(failed)?)
        .stderr(File::create(&errors).map_err(failed)?)
        .spawn()
        .map_err(failed)?;
    let status = loop {
        if progress.is_cancelled() {
            logging::detail("Stopping TAR metadata process");
            let _ = child.kill();
            let _ = child.wait();
            return Err(OperationError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(error));
            }
        }
    };
    progress.check()?;
    let stdout = fs::read_to_string(listing).map_err(failed)?;
    let stderr = fs::read_to_string(errors).map_err(failed)?;
    for line in stdout.lines() {
        logging::detail(format!("TAR metadata: {line}"));
    }
    for line in stderr.lines() {
        logging::detail(format!("TAR metadata stderr: {line}"));
    }
    if !status.success() {
        return Err(failed(format!(
            "Cannot read TAR archive: {}\n{}",
            stdout.trim(),
            stderr.trim()
        )));
    }
    seven_zip::parse_listing(path, format(path)?, &stdout).map_err(failed)
}

fn extract_tar(
    tar: &Path,
    destination: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let mut command = crate::service::seven_zip_cli::command()?;
    command
        .args(["x", "-ttar", "-y", "-spd"])
        .arg(format!("-o{}", destination.display()))
        .arg("--")
        .arg(tar);
    operations::run_archive_command(command, progress)
}

fn pack_posix_tar(
    contents: &Path,
    tar: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    let errors = tar.with_extension("errors");
    let mut child = Command::new("tar")
        .current_dir(contents)
        .env_remove("TAR_OPTIONS")
        .args(["-c", "--format=pax", "-f"])
        .arg(tar)
        .arg(".")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(File::create(&errors).map_err(failed)?)
        .spawn()
        .map_err(|error| failed(format!("Cannot start system tar for POSIX format: {error}")))?;
    let status = loop {
        if progress.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OperationError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(error));
            }
        }
    };
    progress.check()?;
    if !status.success() {
        return Err(failed(format!(
            "Cannot create POSIX TAR archive: {}",
            fs::read_to_string(errors).map_err(failed)?.trim()
        )));
    }
    progress.update(100., "Packed POSIX TAR");
    Ok(())
}

fn pack(
    contents: &Path,
    stage: &Path,
    format: ArchiveFormat,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<PathBuf, OperationError> {
    if format == ArchiveFormat::TarCompress {
        return Err(failed(
            "TAR / compress archives are read-only. Create a tar.gz, tar.bz2, or tar.xz archive instead.",
        ));
    }
    super::compression::require_tool(format)?;
    let tar = stage.join("updated.tar");
    if options.tar_format == TarFormat::Posix {
        pack_posix_tar(contents, &tar, &progress.phase(0., 35.))?;
    } else {
        let mut command = crate::service::seven_zip_cli::command()?;
        command
            .current_dir(contents)
            .args(["a", "-ttar", "-y", "-spd", "--"])
            .arg(&tar)
            .arg(".");
        operations::run_archive_command(command, &progress.phase(0., 35.))?;
    }
    if format == ArchiveFormat::Tar {
        return Ok(tar);
    }
    let updated = stage.join(format!("updated.{}", super::extension(format)));
    if matches!(format, ArchiveFormat::TarLzma | ArchiveFormat::TarZstd) {
        super::compression::encode(&tar, &updated, format, options, &progress.phase(35., 100.))?;
        return Ok(updated);
    }
    let mut command = crate::service::seven_zip_cli::command()?;
    command.args(["a", codec(format), "-y", "-spd"]);
    zip::command_options(&mut command, options, true, format);
    command.arg("--").arg(&updated).arg(&tar);
    operations::run_archive_command(command, &progress.phase(35., 100.))?;
    Ok(updated)
}

fn replace(
    updated: &Path,
    path: &Path,
    version: Option<&fs::Metadata>,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    if let Some(version) = version {
        let current = fs::metadata(path).map_err(failed)?;
        if current.len() != version.len()
            || current.modified().map_err(failed)? != version.modified().map_err(failed)?
        {
            return Err(failed(
                "Archive changed during the operation; the original was not replaced.",
            ));
        }
        fs::set_permissions(updated, version.permissions()).map_err(failed)?;
    } else if path.symlink_metadata().is_ok() {
        return Err(failed(
            "Archive destination appeared during the operation; it was not replaced.",
        ));
    }
    File::open(updated)
        .map_err(failed)?
        .sync_all()
        .map_err(failed)?;
    progress.check()?;
    if version.is_some() {
        fs::rename(updated, path).map_err(failed)?;
    } else {
        // Exclusive creation also works on filesystems without hard links.
        // Cancellation keeps the partially copied new archive, as requested.
        operations::copy_or_move_with_progress(
            &[updated.to_owned()],
            path,
            false,
            &progress.phase(98., 100.),
        )?;
        File::open(path)
            .map_err(failed)?
            .sync_all()
            .map_err(failed)?;
    }
    progress.update(100., path.display().to_string());
    Ok(())
}

impl ArchiveBackend for TarBackend {
    fn read(&self, path: &Path) -> Result<ArchiveIndex, String> {
        let progress = ProgressHandle::default();
        workspace(None, &progress, |stage| {
            let tar = unpack_layer(path, stage, &progress)?;
            read_tar(path, &tar, stage, &progress)
        })
        .map_err(|error| match error {
            OperationError::Failed(error) => error,
            OperationError::Cancelled => "Reading TAR archive cancelled.".into(),
        })
    }

    fn verify_password(&self, _: &Path, _: &str, _: &ProgressHandle) -> Result<(), OperationError> {
        Err(failed("Compressed TAR archives do not support passwords."))
    }

    fn apply(
        &self,
        path: &Path,
        operation: &ArchiveOperation,
        options: &ArchiveOptions,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError> {
        progress.check()?;
        validate_options(options)?;
        let export_only = matches!(
            operation,
            ArchiveOperation::Transfer {
                destination: ArchiveDestination::FileSystem(_),
                move_entries: false,
                ..
            }
        );
        if !export_only {
            super::compression::require_tool(format(path)?)?;
        }
        if super::is_read_only(path) && !export_only {
            return Err(failed(
                "This archive is read-only; copy entries to a system folder to edit them.",
            ));
        }
        let path = fs::canonicalize(path).map_err(failed)?;
        let version = fs::metadata(&path).map_err(failed)?;
        workspace(
            if export_only { None } else { path.parent() },
            progress,
            |stage| {
                let tar = unpack_layer(&path, stage, &progress.phase(0., 15.))?;
                let index = read_tar(&path, &tar, stage, progress)?;
                progress.check()?;
                seven_zip::validate_operation(&index, operation)?;
                let contents = stage.join("contents");
                fs::create_dir(&contents).map_err(failed)?;
                extract_tar(&tar, &contents, &progress.phase(15., 40.))?;
                let end = if export_only { 100. } else { 60. };
                if seven_zip::apply_to_directory(
                    &index,
                    operation,
                    &contents,
                    &progress.phase(40., end),
                )? {
                    let updated = pack(
                        &contents,
                        stage,
                        index.format,
                        options,
                        &progress.phase(60., 98.),
                    )?;
                    replace(&updated, &path, Some(&version), progress)?;
                }
                Ok(())
            },
        )
    }
}

pub(in crate::service) fn create(
    sources: &[PathBuf],
    path: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    validate_options(options)?;
    let format = format(path)?;
    if format == ArchiveFormat::TarCompress {
        return Err(failed("TAR / compress archives are read-only."));
    }
    super::compression::require_tool(format)?;
    let parent = fs::canonicalize(path.parent().unwrap_or(Path::new("."))).map_err(failed)?;
    let path = parent.join(
        path.file_name()
            .ok_or_else(|| failed("Choose an archive name."))?,
    );
    let operation = ArchiveOperation::Add {
        sources: sources.to_vec(),
        directory: "/".into(),
    };
    if path.exists() {
        return TarBackend.apply(&path, &operation, options, progress);
    }
    let index = build_index(&path, format, false, Vec::new())?;
    seven_zip::validate_operation(&index, &operation)?;
    workspace(Some(&parent), progress, |stage| {
        let contents = stage.join("contents");
        fs::create_dir(&contents).map_err(failed)?;
        seven_zip::apply_to_directory(&index, &operation, &contents, &progress.phase(0., 30.))?;
        let updated = pack(&contents, stage, format, options, &progress.phase(30., 98.))?;
        replace(&updated, &path, None, progress)
    })
}

pub(in crate::service) fn extract(
    path: &Path,
    destination: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    validate_options(options)?;
    workspace(None, progress, |stage| {
        let tar = unpack_layer(path, stage, &progress.phase(0., 35.))?;
        let index = read_tar(path, &tar, stage, progress)?;
        operations::extract_tar_with_index(&tar, destination, index, &progress.phase(35., 100.))
    })
}

pub(in crate::service) fn test(
    path: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    validate_options(options)?;
    workspace(None, progress, |stage| {
        // Decompression validates the outer checksum before testing the inner TAR.
        let tar = unpack_layer(path, stage, &progress.phase(0., 70.))?;
        read_tar(path, &tar, stage, progress)?;
        let mut command = crate::service::seven_zip_cli::command()?;
        command.args(["t", "-ttar", "--"]).arg(tar);
        operations::run_archive_command(command, &progress.phase(70., 100.))
    })
}
