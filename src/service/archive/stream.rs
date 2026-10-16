//! Single-layer decoding with a source-derived name and exclusive output publication.

use super::stream_output_name;
use crate::{
    model::{
        archive::{ArchiveOptions, StreamFormat},
        operations::OperationError,
    },
    service::{
        logging,
        operations::{self, ProgressHandle},
        seven_zip_cli,
    },
};
use std::{fs, io, path::Path, process::Command};

fn failed(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

pub(in crate::service) fn extract(
    source: &Path,
    destination: &Path,
    format: StreamFormat,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    if options
        .password
        .as_ref()
        .is_some_and(|password| !password.is_empty())
        || options.volume_size.is_some()
        || options.compression_level.is_some()
    {
        return Err(failed(
            "Standalone streams do not support passwords, volumes, or compression settings.",
        ));
    }
    let command = seven_zip_cli::command()?;
    extract_using_command(source, destination, format, progress, command)
}

fn extract_using_command(
    source: &Path,
    destination: &Path,
    format: StreamFormat,
    progress: &ProgressHandle,
    mut command: Command,
) -> Result<(), OperationError> {
    progress.check()?;
    let name = stream_output_name(source)?;
    let source = fs::canonicalize(source).map_err(failed)?;
    if !source.is_file() {
        return Err(failed("Choose a standalone stream file."));
    }
    fs::create_dir_all(destination).map_err(failed)?;
    let destination = fs::canonicalize(destination).map_err(failed)?;
    let output = destination.join(&name);
    match output.symlink_metadata() {
        Ok(_) => {
            return Err(failed(format!(
                "Output already exists: {}. Choose another destination folder.",
                output.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(error)),
    }
    progress.check()?;
    // The partial file stays on every error path, including I/O and publication
    // failures. A randomized suffix also allows retrying without clobbering it.
    let partial = tempfile::Builder::new()
        .prefix("rs7z-")
        .suffix(".partial")
        .disable_cleanup(true)
        .tempfile_in(&destination)
        .map_err(failed)?;
    let partial_path = partial.path().to_owned();
    let codec = match format {
        StreamFormat::Gzip => "-tgzip",
        StreamFormat::Bzip2 => "-tbzip2",
        StreamFormat::Xz => "-txz",
        StreamFormat::Zstd => "-tzstd",
        StreamFormat::Lzma => "-tlzma",
        StreamFormat::Compress => "-tZ",
    };
    command
        .args(["x", codec, "-so", "-y", "-spd", "--"])
        .arg(&source);
    logging::info(format!(
        "Extracting {} to {}",
        source.display(),
        output.display()
    ));
    progress.update(0., output.display().to_string());
    let result = (|| {
        operations::run_archive_command_to_file(
            command,
            partial.as_file().try_clone().map_err(failed)?,
            progress,
        )?;
        progress.check()?;
        partial.as_file().sync_all().map_err(failed)?;
        progress.check()?;
        // Unlike rename, persist_noclobber never replaces a file created by
        // another process after our initial existence check.
        partial.persist_noclobber(&output).map_err(failed)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            progress.update(100., output.display().to_string());
            Ok(())
        }
        Err(error) => {
            let retained = format!("Partial output retained at: {}", partial_path.display());
            logging::info(&retained);
            progress.update(progress.snapshot().percent, &retained);
            match error {
                OperationError::Cancelled => Err(OperationError::Cancelled),
                OperationError::Failed(error) => Err(failed(format!("{error}\n\n{retained}"))),
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/support/streams.rs"]
mod tests;
