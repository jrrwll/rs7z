use super::{
    ArchiveBackend, destination_directory, join, rename_target, seven_zip, validate_name, within,
};
use crate::model::{
    archive::{ArchiveDestination, ArchiveFormat, ArchiveIndex, ArchiveOperation, ArchiveOptions},
    operations::OperationError,
};
use crate::service::{
    logging,
    operations::{self, ProgressHandle},
    seven_zip_cli, zip,
};
use std::{
    collections::HashSet,
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};

pub(super) struct ZipBackend;

fn failed(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

impl ArchiveBackend for ZipBackend {
    fn read(&self, path: &Path) -> Result<ArchiveIndex, String> {
        self.read_with_options(path, &ArchiveOptions::default())
    }

    fn read_with_options(
        &self,
        path: &Path,
        options: &ArchiveOptions,
    ) -> Result<ArchiveIndex, String> {
        seven_zip::read_index(path, options, ArchiveFormat::Zip)
    }

    fn verify_password(
        &self,
        path: &Path,
        password: &str,
        progress: &ProgressHandle,
    ) -> Result<(), OperationError> {
        let options = ArchiveOptions {
            password: Some(password.to_owned()),
            ..Default::default()
        };
        let mut command = command("t", &options)?;
        command.arg("--").arg(path);
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
        let path = fs::canonicalize(path).map_err(failed)?;
        let version = fs::metadata(&path).map_err(failed)?;
        let index = self.read_with_options(&path, options)?;
        if options.volume_size.is_some() {
            return Err(failed(
                "Volume size can only be set when creating a new ZIP or 7z archive.",
            ));
        }
        seven_zip::validate_operation(&index, operation)?;
        if let ArchiveOperation::Add { sources, directory } = operation {
            validate_sources(
                &path,
                sources,
                &destination_directory(&index, directory)?,
                progress,
            )?;
        }
        let export_only = matches!(
            operation,
            ArchiveOperation::Transfer {
                destination: ArchiveDestination::FileSystem(_),
                move_entries: false,
                ..
            }
        );
        // Exports also work from archives in folders the user cannot write to.
        let mut builder = tempfile::Builder::new();
        builder.prefix(".rs7z-zip-");
        let stage = if export_only {
            builder.tempdir()
        } else {
            builder.tempdir_in(path.parent().unwrap_or(Path::new(".")))
        }
        .map_err(failed)?
        .keep();
        logging::detail(format!("ZIP operation workspace: {}", stage.display()));
        let result = (|| {
            let updated = stage.join("updated.zip.partial");
            if !export_only {
                operations::copy_or_move_with_progress(
                    std::slice::from_ref(&path),
                    &updated,
                    false,
                    &progress.phase(0., 15.),
                )?;
            }
            edit(
                &index,
                operation,
                options,
                &stage,
                &updated,
                &progress.phase(15., 90.),
            )?;
            progress.check()?;
            if !export_only {
                // Check the resulting directory before committing, without decoding unchanged payloads.
                self.read_with_options(&updated, options)?;
                fs::set_permissions(&updated, version.permissions()).map_err(failed)?;
                File::open(&updated)
                    .map_err(failed)?
                    .sync_all()
                    .map_err(failed)?;
                check_version(&path, &version)?;
                progress.check()?;
                fs::rename(&updated, &path).map_err(failed)?;
            }
            Ok(())
        })();
        if result.is_ok() {
            if let Err(error) = fs::remove_dir_all(&stage) {
                logging::error(format!(
                    "Cannot remove completed ZIP workspace {}: {error}",
                    stage.display()
                ));
            }
            progress.update(
                100.,
                if export_only {
                    "Files exported"
                } else {
                    "Archive updated"
                },
            );
        } else {
            logging::info(format!(
                "Partial ZIP results retained at {}",
                stage.display()
            ));
        }
        result
    }
}

fn command(action: &str, options: &ArchiveOptions) -> Result<Command, OperationError> {
    let mut command = seven_zip_cli::command()?;
    command.args([action, "-tzip", "-y", "-spd", "-ssc"]);
    zip::command_options(&mut command, options, false, ArchiveFormat::Zip);
    // The process runner closes stdin. Bare `-p` would prompt even when renaming
    // an unencrypted archive, so only pass it with an actual password.
    Ok(command)
}

fn roots(index: &ArchiveIndex, names: &[String]) -> Vec<String> {
    index
        .entries
        .iter()
        .filter(|entry| names.contains(&entry.name))
        .filter(|entry| {
            !names
                .iter()
                .any(|name| name != &entry.name && within(&entry.name, name))
        })
        .map(|entry| entry.name.clone())
        .collect()
}

// Use literal arguments after `--`: 7zz list files trim whitespace from names.
// Fill each batch by argument length so a large selection does not rewrite the
// archive once per few files. Leave room for the executable, switches and paths.
fn batches<T>(items: &[T], size: impl Fn(&T) -> usize) -> Vec<&[T]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut length = 0;
    for (ix, item) in items.iter().enumerate() {
        let size = size(item);
        if ix > start && length + size > 24 * 1024 {
            batches.push(&items[start..ix]);
            start = ix;
            length = 0;
        }
        length += size;
    }
    if start < items.len() {
        batches.push(&items[start..]);
    }
    batches
}

fn selected_command(
    action: &str,
    archive: &Path,
    names: &[String],
    options: &ArchiveOptions,
    output: Option<&Path>,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let batches = batches(names, |name| name.len() + 3);
    for (ix, batch) in batches.iter().enumerate() {
        let mut command = command(action, options)?;
        if let Some(output) = output {
            command.arg(format!("-o{}", output.display()));
        }
        command.arg("--").arg(archive).args(*batch);
        let count = batches.len() as f32;
        operations::run_archive_command(
            command,
            &progress.phase(ix as f32 * 100. / count, (ix + 1) as f32 * 100. / count),
        )?;
    }
    Ok(())
}

fn rename(
    archive: &Path,
    mapping: &[(String, String)],
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let batches = batches(mapping, |(source, target)| source.len() + target.len() + 6);
    for (ix, batch) in batches.iter().enumerate() {
        let mut command = command("rn", options)?;
        command.arg("--").arg(archive);
        for (source, target) in *batch {
            command.arg(source).arg(target);
        }
        let count = batches.len() as f32;
        operations::run_archive_command(
            command,
            &progress.phase(ix as f32 * 100. / count, (ix + 1) as f32 * 100. / count),
        )?;
    }
    Ok(())
}

fn add(
    archive: &Path,
    input: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let mut command = seven_zip_cli::command()?;
    command
        .current_dir(input)
        .args(["a", "-tzip", "-y", "-spd", "-ssc", "-sse"]);
    zip::command_options(&mut command, options, true, ArchiveFormat::Zip);
    command.arg("--").arg(archive).arg(".");
    operations::run_archive_command(command, progress)
}

fn edit(
    index: &ArchiveIndex,
    operation: &ArchiveOperation,
    options: &ArchiveOptions,
    stage: &Path,
    updated: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let input = stage.join("input");
    fs::create_dir(&input).map_err(failed)?;
    let mut removed = Vec::new();
    let mut mapping = Vec::new();
    let mut destination = String::new();
    match operation {
        ArchiveOperation::Add { sources, directory } => {
            destination = destination_directory(index, directory)?;
            let folder = input.join(&destination);
            fs::create_dir_all(&folder).map_err(failed)?;
            for (ix, source) in sources.iter().enumerate() {
                let target = folder.join(
                    source
                        .file_name()
                        .ok_or_else(|| failed("Invalid source name"))?,
                );
                copy(
                    source,
                    &target,
                    &progress.phase(
                        ix as f32 * 40. / sources.len() as f32,
                        (ix + 1) as f32 * 40. / sources.len() as f32,
                    ),
                )?;
            }
            add(updated, &input, options, &progress.phase(40., 75.))?;
        }
        ArchiveOperation::Delete { entries } => {
            removed = roots(index, entries);
            selected_command(
                "d",
                updated,
                &removed,
                options,
                None,
                &progress.phase(0., 75.),
            )?;
        }
        ArchiveOperation::Rename { entry, name } => {
            removed.push(entry.clone());
            mapping.push((entry.clone(), rename_target(index, entry, name)?));
            rename(updated, &mapping, options, &progress.phase(0., 75.))?;
        }
        ArchiveOperation::Transfer {
            entries,
            destination: target,
            move_entries,
        } => {
            let selected = roots(index, entries);
            match target {
                ArchiveDestination::Directory(directory) => {
                    destination = destination_directory(index, directory)?;
                    mapping = selected
                        .iter()
                        .map(|root| {
                            (
                                root.clone(),
                                join(&destination, root.rsplit('/').next().unwrap()),
                            )
                        })
                        .collect();
                    if *move_entries {
                        removed = selected;
                        rename(updated, &mapping, options, &progress.phase(0., 75.))?;
                    } else {
                        let contents =
                            extract(index, &selected, options, stage, &progress.phase(0., 40.))?;
                        for (source, target) in &mapping {
                            let target = input.join(target);
                            fs::create_dir_all(target.parent().unwrap()).map_err(failed)?;
                            if target.symlink_metadata().is_ok() {
                                return Err(failed(format!(
                                    "Destination names collide on this filesystem: {}",
                                    target.display()
                                )));
                            }
                            // Both paths are in this private workspace, on the same filesystem.
                            fs::rename(contents.join(source), target).map_err(failed)?;
                        }
                        add(updated, &input, options, &progress.phase(40., 75.))?;
                    }
                }
                ArchiveDestination::FileSystem(folder) => {
                    let contents =
                        extract(index, &selected, options, stage, &progress.phase(0., 40.))?;
                    let folder = fs::canonicalize(folder).map_err(failed)?;
                    for (ix, root) in selected.iter().enumerate() {
                        copy(
                            &contents.join(root),
                            &folder.join(root.rsplit('/').next().unwrap()),
                            &progress.phase(
                                40. + ix as f32 * 25. / selected.len() as f32,
                                40. + (ix + 1) as f32 * 25. / selected.len() as f32,
                            ),
                        )?;
                    }
                    if !move_entries {
                        return Ok(());
                    }
                    removed = selected;
                    selected_command(
                        "d",
                        updated,
                        &removed,
                        options,
                        None,
                        &progress.phase(65., 75.),
                    )?;
                }
            }
        }
    }
    preserve_directories(
        index,
        updated,
        &removed,
        &mapping,
        &destination,
        stage,
        &progress.phase(75., 100.),
    )
}

fn extract(
    index: &ArchiveIndex,
    roots: &[String],
    options: &ArchiveOptions,
    stage: &Path,
    progress: &ProgressHandle,
) -> Result<PathBuf, OperationError> {
    let contents = stage.join("contents");
    fs::create_dir(&contents).map_err(failed)?;
    // Reserve the exact paths first. Case/normalization collisions on the host
    // filesystem must fail instead of silently overwriting another selected entry.
    for entry in &index.entries {
        progress.check()?;
        let selected = roots.iter().any(|root| within(&entry.name, root));
        let ancestor = entry.directory && roots.iter().any(|root| within(root, &entry.name));
        if selected || ancestor {
            let path = contents.join(&entry.name);
            if entry.directory {
                fs::create_dir(&path).map_err(failed)?;
            } else {
                File::create_new(&path).map_err(failed)?;
            }
        }
    }
    selected_command("x", &index.path, roots, options, Some(&contents), progress)?;
    Ok(contents)
}

// Preserve implicit directories when their last child is removed or moved, and
// create explicit parent records for newly chosen destinations. No payloads are repacked.
#[allow(clippy::too_many_arguments)]
fn preserve_directories(
    index: &ArchiveIndex,
    updated: &Path,
    removed: &[String],
    mapping: &[(String, String)],
    destination: &str,
    stage: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let options = ArchiveOptions::default();
    let current = seven_zip::read_index(updated, &options, ArchiveFormat::Zip)?;
    let existing: HashSet<_> = current
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    let skeleton = stage.join("directories");
    fs::create_dir(&skeleton).map_err(failed)?;
    let mut needed = false;
    let mut created = HashSet::new();
    for entry in index.entries.iter().filter(|entry| entry.directory) {
        progress.check()?;
        let mut names = Vec::new();
        if !removed.iter().any(|root| within(&entry.name, root)) {
            names.push(entry.name.clone());
        }
        if let Some((root, target)) = mapping.iter().find(|(root, _)| within(&entry.name, root)) {
            names.push(format!("{target}{}", &entry.name[root.len()..]));
        }
        for name in names {
            if !existing.contains(name.as_str()) {
                create_directories(&skeleton, &name, &mut created)?;
                needed = true;
            }
        }
    }
    if !destination.is_empty() {
        create_directories(&skeleton, destination, &mut created)?;
        needed = true;
    }
    if needed {
        add(updated, &skeleton, &options, progress)?;
    }
    Ok(())
}

fn create_directories(
    root: &Path,
    name: &str,
    created: &mut HashSet<String>,
) -> Result<(), OperationError> {
    let mut parent = String::new();
    for part in name.split('/') {
        parent = join(&parent, part);
        if created.insert(parent.clone()) {
            // Unlike create_dir_all, this detects case/normalization collisions.
            fs::create_dir(root.join(&parent)).map_err(failed)?;
        }
    }
    Ok(())
}

fn validate_sources(
    archive: &Path,
    sources: &[PathBuf],
    directory: &str,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let mut pending: Vec<_> = sources
        .iter()
        .map(|source| {
            let name = source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| failed("Source name must be UTF-8."))?;
            Ok((source.clone(), join(directory, name)))
        })
        .collect::<Result<_, OperationError>>()?;
    while let Some((path, name)) = pending.pop() {
        progress.check()?;
        validate_name(&name)?;
        let metadata = fs::symlink_metadata(&path).map_err(failed)?;
        if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
            return Err(failed(format!("Unsupported source: {}", path.display())));
        }
        let canonical = fs::canonicalize(&path).map_err(failed)?;
        if canonical == archive || (metadata.is_dir() && archive.starts_with(&canonical)) {
            return Err(failed(
                "Cannot add the archive or a directory containing it to itself.",
            ));
        }
        if metadata.is_dir() {
            for child in fs::read_dir(&path).map_err(failed)? {
                let child = child.map_err(failed)?;
                let child_name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| failed("Source name must be UTF-8."))?;
                pending.push((child.path(), join(&name, &child_name)));
            }
        }
    }
    Ok(())
}

fn copy(
    source: &Path,
    destination: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    // The shared copier uses create_new and checks symlinks; preserve file times
    // as well, since these become ZIP timestamps or exported file timestamps.
    operations::copy_or_move_with_progress(&[source.to_owned()], destination, false, progress)?;
    let mut pending = vec![(source.to_owned(), destination.to_owned())];
    while let Some((source, target)) = pending.pop() {
        progress.check()?;
        let metadata = fs::symlink_metadata(&source).map_err(failed)?;
        if metadata.is_dir() {
            for child in fs::read_dir(&source).map_err(failed)? {
                let child = child.map_err(failed)?;
                pending.push((child.path(), target.join(child.file_name())));
            }
        } else if let Ok(modified) = metadata.modified() {
            let output = File::open(&target).map_err(failed)?;
            output
                .set_times(fs::FileTimes::new().set_modified(modified))
                .map_err(failed)?;
            output.sync_all().map_err(failed)?;
        }
    }
    Ok(())
}

fn check_version(path: &Path, version: &fs::Metadata) -> Result<(), OperationError> {
    let current = fs::metadata(path).map_err(failed)?;
    if current.len() != version.len()
        || current.modified().map_err(failed)? != version.modified().map_err(failed)?
    {
        return Err(failed(
            "Archive changed during the operation; the original was not replaced.",
        ));
    }
    Ok(())
}
