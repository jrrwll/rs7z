use super::{ArchiveBackend, seven_zip};
use crate::model::{
    archive::{ArchiveDestination, ArchiveFormat, ArchiveIndex, ArchiveOperation, ArchiveOptions},
    operations::OperationError,
};
use crate::service::{
    logging,
    operations::{self, ProgressHandle},
    zip,
};
use std::{fs, path::Path};

pub(super) struct ReadOnlyBackend;

fn failed(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

impl ArchiveBackend for ReadOnlyBackend {
    fn read(&self, path: &Path) -> Result<ArchiveIndex, String> {
        self.read_with_options(path, &ArchiveOptions::default())
    }

    fn read_with_options(
        &self,
        path: &Path,
        options: &ArchiveOptions,
    ) -> Result<ArchiveIndex, String> {
        let path = super::resolve_source(path)?;
        let format = super::format(&path).ok_or("Unsupported archive format")?;
        seven_zip::read_index(&path, options, format)
    }

    fn verify_password(
        &self,
        path: &Path,
        password: &str,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError> {
        let path = super::resolve_source(path)?;
        let mut command = crate::service::seven_zip_cli::command()?;
        if !super::is_numbered_zip(&path) {
            command.arg("-tzip");
        }
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
        if !matches!(
            operation,
            ArchiveOperation::Transfer {
                destination: ArchiveDestination::FileSystem(_),
                move_entries: false,
                ..
            }
        ) {
            return Err(failed(
                "Split ZIP archives support copying to a system folder only.",
            ));
        }
        let index = self.read_with_options(path, options)?;
        if index.encrypted && zip::password(options).is_none() {
            return Err(failed("Password required for this encrypted ZIP archive."));
        }
        seven_zip::validate_operation(&index, operation)?;
        let stage = tempfile::Builder::new()
            .prefix(".rs7z-export-")
            .tempdir()
            .map_err(failed)?
            .keep();
        logging::detail(format!("Archive export workspace: {}", stage.display()));
        let result = (|| {
            let contents = stage.join("contents");
            fs::create_dir(&contents).map_err(failed)?;
            let mut command = crate::service::seven_zip_cli::command()?;
            command
                .args(["x", "-y", "-spd"])
                .arg(format!("-o{}", contents.display()));
            zip::read_command_options(&mut command, options, ArchiveFormat::Zip, &index.path);
            command.arg("--").arg(&index.path);
            operations::run_archive_command(command, &progress.phase(0., 65.))?;
            seven_zip::apply_to_directory(
                &index,
                operation,
                &contents,
                &progress.phase(65., 100.),
            )?;
            progress.check()
        })();
        if result.is_ok() && !progress.is_cancelled() {
            logging::detail(format!(
                "Removing completed export workspace: {}",
                stage.display()
            ));
            if let Err(error) = fs::remove_dir_all(&stage) {
                logging::error(format!(
                    "Cannot remove completed workspace {}: {error}",
                    stage.display()
                ));
            }
        } else {
            logging::info(format!(
                "Partial archive results retained at {}",
                stage.display()
            ));
        }
        result
    }
}
