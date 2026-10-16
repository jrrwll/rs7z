use super::{ArchiveBackend, seven_zip};
use crate::model::{
    archive::{ArchiveFormat, ArchiveIndex, ArchiveOperation, ArchiveOptions},
    operations::OperationError,
};
use crate::service::{
    operations::{self, ProgressHandle},
    seven_zip_cli,
};
use std::path::Path;

pub(super) struct RarBackend;

impl ArchiveBackend for RarBackend {
    fn read(&self, path: &Path) -> Result<ArchiveIndex, String> {
        self.read_with_options(path, &ArchiveOptions::default())
    }

    fn read_with_options(
        &self,
        path: &Path,
        options: &ArchiveOptions,
    ) -> Result<ArchiveIndex, String> {
        seven_zip::read_index(path, options, ArchiveFormat::Rar)
    }

    fn verify_password(
        &self,
        path: &Path,
        password: &str,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError> {
        progress.check()?;
        let mut command = seven_zip_cli::rar_command()?;
        command
            .args(["t", "-y"])
            .arg(format!("-p{password}"))
            .arg("--")
            .arg(path);
        operations::run_archive_command(command, progress)
    }

    fn apply(
        &self,
        _: &Path,
        _: &ArchiveOperation,
        _: &ArchiveOptions,
        _: &ProgressHandle,
    ) -> Result<(), OperationError> {
        Err(OperationError::Failed(
            "RAR archives support extraction only.".into(),
        ))
    }
}

pub(in crate::service) fn extract(
    path: &Path,
    destination: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    progress.enable_output();
    let mut command = seven_zip_cli::rar_command()?;
    // Emit filenames as well as the CLI's live status and final summary.
    command.arg("-bb1");
    operations::extract_using_command(path, destination, options, progress, command)
}
