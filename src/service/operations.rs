use crate::model::archive::ArchiveFormat;
use crate::model::archive::ArchiveOptions;
use crate::model::operations::{OperationError, ProgressSnapshot};
use crate::service::logging;
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone, Default)]
pub struct ProgressHandle {
    snapshot: Arc<Mutex<ProgressSnapshot>>,
    cancelled: Arc<AtomicBool>,
    range: Option<(f32, f32)>,
    output: Arc<Mutex<Option<CommandOutput>>>,
}

#[derive(Default)]
struct CommandOutput {
    completed: String,
    current: String,
    stderr_current: String,
}

// Keep the most recent output so a verbose archive cannot grow UI memory forever.
const OUTPUT_LIMIT: usize = 64 * 1024;

fn keep_output_tail(text: &mut String) {
    if text.len() > OUTPUT_LIMIT {
        let mut start = text.len() - OUTPUT_LIMIT;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text.drain(..start);
    }
}

impl ProgressHandle {
    /// Create an operation that displays live CLI output instead of a progress bar.
    pub fn with_output() -> Self {
        let handle = Self::default();
        handle.enable_output();
        handle
    }

    pub(crate) fn enable_output(&self) {
        self.output
            .lock()
            .unwrap()
            .get_or_insert_with(CommandOutput::default);
    }

    /// Recent completed output and the current, possibly unterminated CLI line.
    pub fn output(&self) -> Option<String> {
        self.output.lock().unwrap().as_ref().map(|output| {
            let separator = if !output.current.is_empty() && !output.stderr_current.is_empty() {
                "\n"
            } else {
                ""
            };
            format!(
                "{}{}{separator}{}",
                output.completed, output.current, output.stderr_current
            )
        })
    }

    fn shows_output(&self) -> bool {
        self.output.lock().unwrap().is_some()
    }

    fn output_line(&self, text: &str, complete: bool) {
        if let Some(output) = self.output.lock().unwrap().as_mut() {
            output.current.clear();
            if complete {
                output.completed.push_str(text);
                output.completed.push('\n');
                keep_output_tail(&mut output.completed);
            } else {
                output.current.push_str(text);
                keep_output_tail(&mut output.current);
            }
        }
    }

    fn output_stderr(&self, text: &str) {
        if let Some(output) = self.output.lock().unwrap().as_mut() {
            output.stderr_current.push_str(text);
            if let Some(end) = output.stderr_current.rfind('\n') {
                output.completed.extend(output.stderr_current.drain(..=end));
                keep_output_tail(&mut output.completed);
            }
            keep_output_tail(&mut output.stderr_current);
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        self.snapshot.lock().unwrap().clone()
    }

    pub(crate) fn update(&self, percent: f32, item: impl Into<String>) {
        *self.snapshot.lock().unwrap() = ProgressSnapshot {
            percent: self.mapped_percent(percent),
            item: item.into(),
        };
    }

    fn mapped_percent(&self, percent: f32) -> f32 {
        let (start, end) = self.range.unwrap_or((0., 100.));
        start + percent.clamp(0., 100.) * (end - start) / 100.
    }

    pub(crate) fn phase(&self, start: f32, end: f32) -> Self {
        let (parent_start, parent_end) = self.range.unwrap_or((0., 100.));
        let scale = (parent_end - parent_start) / 100.;
        Self {
            snapshot: self.snapshot.clone(),
            cancelled: self.cancelled.clone(),
            range: Some((parent_start + start * scale, parent_start + end * scale)),
            output: self.output.clone(),
        }
    }

    pub(crate) fn check(&self) -> Result<(), OperationError> {
        if self.is_cancelled() {
            Err(OperationError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl From<String> for OperationError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}

fn failure(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

pub fn validate_item_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name.chars().all(|ch| ch == '.')
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
    {
        return Err(
            "Enter a single file or folder name, without path separators or only dots.".into(),
        );
    }
    Ok(())
}

pub fn rename_destination(source: &Path, name: &str) -> Result<PathBuf, String> {
    validate_item_name(name)?;
    let source_metadata = fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    let parent = source.parent().ok_or("This item cannot be renamed.")?;
    if source.file_name().is_none() {
        return Err("This item cannot be renamed.".into());
    }
    let destination = parent.join(name);
    if destination == source {
        return Err("Enter a different name.".into());
    }
    match fs::symlink_metadata(&destination) {
        Ok(destination_metadata) => {
            // Case-insensitive volumes resolve a casing-only change to the source itself.
            // A distinct directory entry (including a hard link) must still block the rename.
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if source_metadata.dev() == destination_metadata.dev()
                    && source_metadata.ino() == destination_metadata.ino()
                {
                    let entries = fs::read_dir(parent).map_err(|error| error.to_string())?;
                    let mut exists = false;
                    for entry in entries {
                        if entry.map_err(|error| error.to_string())?.file_name() == name {
                            exists = true;
                            break;
                        }
                    }
                    if !exists {
                        return Ok(destination);
                    }
                }
            }
            #[cfg(not(unix))]
            let _ = (source_metadata, destination_metadata);
            return Err(format!(
                "Destination already exists: {}",
                destination.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    Ok(destination)
}

pub fn rename_with_progress(
    source: &Path,
    name: &str,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    let destination = rename_destination(source, name)?;
    logging::info(format!(
        "Renaming {} to {}",
        source.display(),
        destination.display()
    ));
    progress.check()?;
    fs::rename(source, &destination).map_err(failure)?;
    progress.update(100., destination.display().to_string());
    Ok(())
}

struct SevenZipError {
    error: OperationError,
    not_started: bool,
    diagnostics: SevenZipDiagnostics,
}

#[derive(Default)]
struct SevenZipDiagnostics {
    wrong_password: bool,
    damaged_entries: HashSet<String>,
    preserve_output: bool,
}

impl SevenZipDiagnostics {
    fn record(&mut self, line: &str) {
        if line.starts_with("ERROR: Wrong password")
            || line.starts_with("ERROR: Data Error in encrypted file. Wrong password?")
            || line.starts_with("ERROR: CRC Failed in encrypted file. Wrong password?")
        {
            self.wrong_password = true;
        } else if let Some(name) = line
            .strip_prefix("ERROR: CRC Failed : ")
            .or_else(|| line.strip_prefix("ERROR: Data Error : "))
        {
            self.damaged_entries.insert(name.to_owned());
        } else if line.starts_with("ERROR") || line.starts_with("System ERROR") {
            // Disk/write errors and unclassified failures must keep all output.
            self.preserve_output = true;
        }
    }
}

impl From<OperationError> for SevenZipError {
    fn from(error: OperationError) -> Self {
        Self {
            error,
            not_started: false,
            diagnostics: SevenZipDiagnostics {
                preserve_output: true,
                ..Default::default()
            },
        }
    }
}

impl From<SevenZipError> for OperationError {
    fn from(error: SevenZipError) -> Self {
        error.error
    }
}

#[derive(Default)]
struct SevenZipOutput {
    line: Vec<u8>,
    rewinding: bool,
    detail: String,
    diagnostics: SevenZipDiagnostics,
}

impl SevenZipOutput {
    fn publish_progress(&self, progress: &ProgressHandle) -> bool {
        let Ok(text) = std::str::from_utf8(&self.line) else {
            return false;
        };
        let Some((number, rest)) = text.trim().split_once('%') else {
            return false;
        };
        let number = number.trim();
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        let Ok(percent @ 0..=100) = number.parse::<u8>() else {
            return false;
        };
        let previous = progress.snapshot();
        // A pipe read can end immediately after '%', before the item name arrives.
        let item = if rest.trim().is_empty() {
            previous.item.as_str()
        } else {
            rest.trim()
        };
        if previous.percent != progress.mapped_percent(f32::from(percent)) || previous.item != item
        {
            progress.update(f32::from(percent), item);
            logging::detail(format!("7zz progress: {}", text.trim()));
        }
        true
    }

    fn finish_line(&mut self, progress: &ProgressHandle) {
        if self.line.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.line);
        progress.output_line(&text, true);
        // Classify every diagnostic even after the displayed output limit.
        // Preserve trailing spaces in entry names for exact matching.
        self.diagnostics.record(text.trim_start());
        self.diagnostics.preserve_output |= matches!(text, std::borrow::Cow::Owned(_));
        let trimmed = text.trim();
        if !self.publish_progress(progress) && !trimmed.is_empty() {
            logging::detail(format!("7zz: {trimmed}"));
            if self.detail.len() < 4096 {
                self.detail.push_str(trimmed);
                self.detail.push('\n');
            }
        }
        self.line.clear();
    }

    fn push(&mut self, bytes: &[u8], progress: &ProgressHandle) {
        for &byte in bytes {
            match byte {
                b'\r' | b'\n' => {
                    self.finish_line(progress);
                    self.rewinding = false;
                }
                b'\x08' => {
                    // 7-Zip overwrites progress with backspaces, without any newline.
                    // Capture the complete item before its text is erased.
                    if !self.rewinding {
                        self.publish_progress(progress);
                    }
                    self.line.pop();
                    self.rewinding = true;
                }
                _ => {
                    self.rewinding = false;
                    self.line.push(byte);
                    if byte == b'%' {
                        self.publish_progress(progress);
                    }
                }
            }
        }
        // Show a live, unterminated progress record as soon as it arrives.
        // Never publish a partially erased item during a split backspace sequence.
        if !self.rewinding {
            self.publish_progress(progress);
            // A pipe read may split a UTF-8 code point. Keep it pending until complete.
            if let Ok(text) = std::str::from_utf8(&self.line) {
                progress.output_line(text, false);
            }
        }
    }
}

fn run_7z_progress(command: Command, progress: &ProgressHandle) -> Result<(), SevenZipError> {
    run_7z_progress_output(command, progress, None, false)
}

fn run_7z_progress_output(
    command: Command,
    progress: &ProgressHandle,
    output_file: Option<fs::File>,
    capture_stdout: bool,
) -> Result<(), SevenZipError> {
    progress.check()?;
    // Output switches must precede `--`, which terminates 7z option parsing.
    let mut prepared = Command::new(command.get_program());
    let switches = [
        if output_file.is_some() {
            "-bsp2"
        } else {
            "-bsp1"
        },
        if capture_stdout
            || (output_file.is_none() && (logging::is_verbose() || progress.shows_output()))
        {
            "-bso1"
        } else {
            "-bso0"
        },
        if output_file.is_some() {
            "-bse2"
        } else {
            "-bse1"
        },
    ];
    let mut inserted = false;
    for argument in command.get_args() {
        if !inserted && argument == "--" {
            prepared.args(switches);
            inserted = true;
        }
        prepared.arg(argument);
    }
    if !inserted {
        prepared.args(switches);
    }
    if let Some(directory) = command.get_current_dir() {
        prepared.current_dir(directory);
    }
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            prepared.env(key, value);
        } else {
            prepared.env_remove(key);
        }
    }
    let mut command = prepared;
    logging::detail(format!(
        "Running 7zz: {}",
        super::zip::command_description(&command)
    ));
    let binary_output = output_file.is_some();
    command
        .stdin(Stdio::null())
        .stdout(output_file.map_or_else(Stdio::piped, Stdio::from))
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| SevenZipError {
        error: failure(format!("Cannot start 7zz: {error}")),
        not_started: true,
        diagnostics: SevenZipDiagnostics::default(),
    })?;
    let mut output: Box<dyn Read + Send> = if binary_output {
        Box::new(child.stderr.take().expect("stderr was piped"))
    } else {
        Box::new(child.stdout.take().expect("stdout was piped"))
    };
    let stderr = child.stderr.take();
    let progress_for_stderr = progress.clone();
    let stderr_reader = thread::spawn(move || {
        let Some(mut stderr) = stderr else {
            return (String::new(), false);
        };
        let mut detail = Vec::new();
        let mut preserve_output = false;
        let mut buffer = [0u8; 4096];
        let mut pending = Vec::new();
        loop {
            let count = match stderr.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    preserve_output = true;
                    break;
                }
            };
            // -bse1 routes archive diagnostics to stdout. Any additional stderr
            // failure (including write errors) makes automatic deletion unsafe.
            preserve_output |= buffer[..count]
                .iter()
                .any(|byte| !byte.is_ascii_whitespace());
            pending.extend_from_slice(&buffer[..count]);
            let ready = match std::str::from_utf8(&pending) {
                Ok(_) => pending.len(),
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                Err(_) => pending.len(),
            };
            if ready > 0 {
                progress_for_stderr.output_stderr(&String::from_utf8_lossy(&pending[..ready]));
                pending.drain(..ready);
            }
            logging::detail(format!(
                "7zz stderr: {}",
                String::from_utf8_lossy(&buffer[..count])
            ));
            let remaining = 4096usize.saturating_sub(detail.len());
            detail.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        if !pending.is_empty() {
            progress_for_stderr.output_stderr(&String::from_utf8_lossy(&pending));
        }
        (
            String::from_utf8_lossy(&detail).into_owned(),
            preserve_output,
        )
    });
    let progress_for_reader = progress.clone();
    let reader = thread::spawn(move || {
        let mut parsed = SevenZipOutput::default();
        let mut buffer = [0u8; 4096];
        loop {
            let count = match output.read(&mut buffer) {
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    parsed.diagnostics.preserve_output = true;
                    break;
                }
            };
            if count == 0 {
                break;
            }
            parsed.push(&buffer[..count], &progress_for_reader);
        }
        parsed.finish_line(&progress_for_reader);
        (parsed.detail, parsed.diagnostics)
    });
    let status = loop {
        if progress.is_cancelled() {
            logging::detail("Stopping 7zz process");
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let _ = stderr_reader.join();
            return Err(OperationError::Cancelled.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                let _ = stderr_reader.join();
                return Err(failure(error).into());
            }
        }
    };
    let (mut detail, mut diagnostics) = reader.join().unwrap_or_else(|_| {
        (
            String::new(),
            SevenZipDiagnostics {
                preserve_output: true,
                ..Default::default()
            },
        )
    });
    let (stderr_detail, preserve_output) = stderr_reader.join().unwrap_or((String::new(), true));
    diagnostics.preserve_output |= preserve_output;
    if !stderr_detail.is_empty() {
        detail.push('\n');
        detail.push_str(&stderr_detail);
    }
    // 7z reports ordinary archive failures with code 2. Signals, crashes and
    // other exit codes are not reliable evidence for deleting output.
    if status.code() != Some(2) {
        diagnostics.preserve_output = true;
    }
    if status.success() {
        progress.update(100., progress.snapshot().item);
        Ok(())
    } else {
        let message = if diagnostics.wrong_password {
            format!("Wrong password for encrypted archive.\n{}", detail.trim())
        } else {
            format!("7zz failed: {}", detail.trim())
        };
        Err(SevenZipError {
            error: failure(message),
            not_started: false,
            diagnostics,
        })
    }
}

pub(super) fn run_archive_command(
    command: Command,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    run_7z_progress(command, progress).map_err(Into::into)
}

/// Keep binary archive output separate from progress and diagnostics.
pub(super) fn run_archive_command_to_file(
    command: Command,
    output: fs::File,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    run_7z_progress_output(command, progress, Some(output), false).map_err(Into::into)
}

/// Capture textual 7zz output while stderr continues to drive cancellation and progress.
pub(super) fn run_archive_command_capture_to_file(
    command: Command,
    output: fs::File,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    run_7z_progress_output(command, progress, Some(output), true).map_err(Into::into)
}

pub fn add_many_with_progress(
    sources: &[PathBuf],
    destination: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    add_many_with_options(sources, destination, &ArchiveOptions::default(), progress)
}

pub fn add_many_with_options(
    sources: &[PathBuf],
    destination: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    if sources.is_empty() {
        return Err(failure("Choose at least one source."));
    }
    let format = super::archive::format(destination)
        .ok_or_else(|| failure("Choose a supported archive path."))?;
    if format != ArchiveFormat::SevenZip && options.seven_zip != Default::default() {
        return Err(failure("7z compression settings require a 7z archive."));
    }
    if super::archive::is_extract_only(destination) {
        return Err(failure(super::archive::extract_only_message(destination)));
    }
    if super::archive::is_tar(format) {
        return super::archive::tar::create(sources, destination, options, progress);
    }
    if super::archive::is_split(destination) {
        return Err(failure(
            "Split archives cannot be updated here. Create a new .zip or .7z archive with a volume size.",
        ));
    }
    if let Some(size) = options.volume_size {
        if size == 0 || !matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZip) {
            return Err(failure(
                "Volume size must be positive and is supported only for ZIP and 7z.",
            ));
        }
        if destination.exists() {
            return Err(failure("Split compression requires a new archive name."));
        }
        if format == ArchiveFormat::Zip
            && !destination
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            return Err(failure(
                "Use a .zip archive name for split ZIP compression.",
            ));
        }
    }
    // Never overwrite a pre-existing volume family when creating a new archive.
    let parent = destination
        .parent()
        .ok_or_else(|| failure("Choose an archive folder."))?;
    let prefix = format!(
        "{}.",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    if matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZip)
        && fs::read_dir(parent).map_err(failure)?.any(|entry| {
            entry.ok().is_some_and(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| {
                        suffix.len() >= 3 && suffix.chars().all(|ch| ch.is_ascii_digit())
                    })
            })
        })
    {
        return Err(failure(
            "Archive volumes already exist. Choose a different archive name.",
        ));
    }
    let mut command = crate::service::seven_zip_cli::command()?;
    if destination.exists() {
        super::zip::require_password_if_encrypted(destination, options)?;
    }
    command.args([
        "a",
        if format == ArchiveFormat::Zip {
            "-tzip"
        } else {
            "-t7z"
        },
        "-y",
        "-spd",
    ]);
    super::zip::command_options(&mut command, options, true, format);
    if format == ArchiveFormat::SevenZip {
        super::zip::seven_zip_command_options(&mut command, options)?;
    }
    if let Some(size) = options.volume_size {
        command.arg(format!("-v{size}b"));
    }
    command.arg("--").arg(destination);
    for source in sources {
        progress.check()?;
        if !source.exists() {
            return Err(failure(format!(
                "Source no longer exists: {}",
                source.display()
            )));
        }
        check_destination(source, destination)?;
        command.arg(source);
    }
    run_7z_progress(command, progress).map_err(Into::into)
}

pub fn extract_with_progress(
    source: &Path,
    destination: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    extract_with_options(source, destination, &ArchiveOptions::default(), progress)
}

pub fn extract_with_options(
    source: &Path,
    destination: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    match super::archive::format(source) {
        Some(ArchiveFormat::Rar) => {
            return super::archive::rar::extract(source, destination, options, progress);
        }
        Some(ArchiveFormat::Stream(format)) => {
            return super::archive::stream::extract(source, destination, format, options, progress);
        }
        None => {
            return Err(failure(format!(
                "Unsupported archive or compressed TAR format: {}",
                source.display()
            )));
        }
        _ => {}
    }
    if super::archive::format(source).is_some_and(super::archive::is_tar) {
        return super::archive::tar::extract(source, destination, options, progress);
    }
    let source = super::archive::resolve_source(source)?;
    extract_using_command(
        &source,
        destination,
        options,
        progress,
        crate::service::seven_zip_cli::command()?,
    )
}

pub(super) fn extract_using_command(
    source: &Path,
    destination: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
    command: Command,
) -> Result<(), OperationError> {
    progress.check()?;
    super::zip::require_password_if_encrypted(source, options)?;
    let index = if super::archive::format(source).is_some() {
        Some(super::archive::read_with_options(source, options)?)
    } else {
        None
    };
    extract_indexed(source, destination, options, progress, command, index)
}

pub(super) fn extract_tar_with_index(
    source: &Path,
    destination: &Path,
    index: crate::model::archive::ArchiveIndex,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let mut command = crate::service::seven_zip_cli::command()?;
    command.arg("-ttar");
    extract_indexed(
        source,
        destination,
        &ArchiveOptions::default(),
        progress,
        command,
        Some(index),
    )
}

fn extract_indexed(
    source: &Path,
    destination: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
    mut command: Command,
    index: Option<crate::model::archive::ArchiveIndex>,
) -> Result<(), OperationError> {
    progress.check()?;
    if destination.exists() && fs::read_dir(destination).map_err(failure)?.next().is_some() {
        return Err(failure(format!(
            "Destination folder is not empty: {}",
            destination.display()
        )));
    }
    // Track only directories absent before this operation, including new parents.
    let created_directories: Vec<_> = destination
        .ancestors()
        .take_while(|path| !path.exists())
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    progress.check()?;
    fs::create_dir_all(destination).map_err(failure)?;
    super::zip::read_command_options(
        &mut command,
        options,
        index
            .as_ref()
            .map(|index| index.format)
            .or_else(|| super::archive::format(source))
            .unwrap_or(ArchiveFormat::Zip),
        source,
    );
    command
        .args(["x", "-y"])
        .arg(format!("-o{}", destination.display()))
        .arg("--")
        .arg(source);
    match run_7z_progress(command, progress) {
        Err(mut error) => {
            progress.check()?;
            if error.not_started || !error.diagnostics.preserve_output {
                let cleanup_errors = cleanup_extraction(
                    destination,
                    index.as_ref(),
                    &created_directories,
                    &error,
                    progress,
                );
                if !cleanup_errors.is_empty()
                    && let OperationError::Failed(message) = &mut error.error
                {
                    message.push_str("\nSome extraction artifacts could not be removed:\n");
                    message.push_str(&cleanup_errors.join("\n"));
                }
            } else {
                logging::detail(
                    "Extraction cleanup skipped: preserving output after a write, unknown, or abnormal process failure",
                );
            }
            progress.check()?;
            Err(error.into())
        }
        Ok(()) => Ok(()),
    }
}

#[derive(Clone, Copy)]
enum ExtractionCleanup {
    EmptyDirectory,
    EmptyEntry,
    FailedFile,
}

fn cleanup_extraction(
    destination: &Path,
    index: Option<&crate::model::archive::ArchiveIndex>,
    created_directories: &[PathBuf],
    error: &SevenZipError,
    progress: &ProgressHandle,
) -> Vec<String> {
    logging::detail(format!(
        "Preparing extraction cleanup: destination={}, not_started={}, wrong_password={}, damaged_entries={}",
        destination.display(),
        error.not_started,
        error.diagnostics.wrong_password,
        error.diagnostics.damaged_entries.len()
    ));
    let mut paths = Vec::new();
    if error.not_started {
        logging::info("7zz did not start; removing new empty destination directories");
        paths.extend(
            created_directories
                .iter()
                .cloned()
                .map(|path| (path, ExtractionCleanup::EmptyDirectory)),
        );
    } else if let Some(index) = index {
        for entry in &index.entries {
            let cleanup =
                if !entry.directory && error.diagnostics.damaged_entries.contains(&entry.name) {
                    ExtractionCleanup::FailedFile
                } else if error.diagnostics.wrong_password {
                    ExtractionCleanup::EmptyEntry
                } else {
                    continue;
                };
            paths.push((destination.join(&entry.name), cleanup));
        }
        // The index includes implicit parent directories as well as explicit entries.
        paths.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
        if error.diagnostics.wrong_password {
            logging::info("Wrong password; removing empty extraction artifacts");
            paths.extend(
                created_directories
                    .iter()
                    .cloned()
                    .map(|path| (path, ExtractionCleanup::EmptyDirectory)),
            );
        }
    }
    let mut errors = Vec::new();
    logging::detail(format!(
        "Extraction cleanup: {} candidate paths",
        paths.len()
    ));
    for (path, cleanup) in paths {
        if progress.is_cancelled() {
            logging::detail(
                "Extraction cleanup stopped: operation cancelled; remaining output preserved",
            );
            break;
        }
        // Never follow a replaced archive directory through a symbolic link.
        if path
            .ancestors()
            .take_while(|parent| parent.starts_with(destination))
            .any(|parent| parent.is_symlink())
        {
            logging::detail(format!(
                "Skipping extraction cleanup through symbolic link: {}",
                path.display()
            ));
            continue;
        }
        let result = (|| -> std::io::Result<()> {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    logging::detail(format!(
                        "Extraction cleanup skipped missing path: {}",
                        path.display()
                    ));
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            if metadata.is_file() && matches!(cleanup, ExtractionCleanup::FailedFile)
                || metadata.is_file()
                    && matches!(cleanup, ExtractionCleanup::EmptyEntry)
                    && metadata.len() == 0
            {
                logging::detail(format!("Removing extraction file: {}", path.display()));
                fs::remove_file(&path)?;
            } else if metadata.is_dir()
                && !matches!(cleanup, ExtractionCleanup::FailedFile)
                && fs::read_dir(&path)?.next().is_none()
            {
                logging::detail(format!(
                    "Removing empty extraction directory: {}",
                    path.display()
                ));
                fs::remove_dir(&path)?;
            } else {
                logging::detail(format!(
                    "Preserving extraction path outside cleanup criteria: {}",
                    path.display()
                ));
                return Ok(());
            }
            logging::detail(format!("Removed extraction artifact: {}", path.display()));
            Ok(())
        })();
        if let Err(error) = result {
            let message = format!("{}: {error}", path.display());
            logging::error(format!("Extraction cleanup failed: {message}"));
            errors.push(message);
        }
    }
    logging::detail(format!(
        "Extraction cleanup finished: {} failures",
        errors.len()
    ));
    errors
}

pub fn test_archive_with_progress(
    source: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    test_archive_with_options(source, &ArchiveOptions::default(), progress)
}

pub fn test_archive_with_options(
    source: &Path,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    if super::archive::is_extract_only(source) {
        return Err(failure(super::archive::extract_only_message(source)));
    }
    let resolved = super::archive::resolve_source(source)?;
    let source = resolved.as_path();
    if super::archive::format(source).is_some_and(super::archive::is_tar) {
        return super::archive::tar::test(source, options, progress);
    }
    super::zip::require_password_if_encrypted(source, options)?;
    let mut command = crate::service::seven_zip_cli::command()?;
    super::zip::read_command_options(
        &mut command,
        options,
        super::archive::format(source).unwrap_or(ArchiveFormat::Zip),
        source,
    );
    command.arg("t").arg("--").arg(source);
    run_7z_progress(command, progress).map_err(Into::into)
}

pub fn add(source: &Path, destination: &Path) -> Result<(), String> {
    add_many(&[source.to_owned()], destination)
}

pub fn add_many(sources: &[PathBuf], destination: &Path) -> Result<(), String> {
    add_many_with_progress(sources, destination, &ProgressHandle::default()).map_err(|error| {
        match error {
            OperationError::Failed(message) => message,
            OperationError::Cancelled => "Compression cancelled.".into(),
        }
    })
}

pub fn extract(source: &Path, destination: &Path) -> Result<(), String> {
    extract_with_progress(source, destination, &ProgressHandle::default()).map_err(|error| {
        match error {
            OperationError::Failed(message) => message,
            OperationError::Cancelled => "Extraction cancelled.".into(),
        }
    })
}

pub fn test_archive(source: &Path) -> Result<(), String> {
    test_archive_with_progress(source, &ProgressHandle::default()).map_err(|error| match error {
        OperationError::Failed(error) => error,
        OperationError::Cancelled => "Archive test cancelled.".into(),
    })
}

pub fn copy_or_move(source: &Path, destination: &Path, move_file: bool) -> Result<(), String> {
    check_destination(source, destination)?;
    if source.is_symlink() {
        return Err(format!("Cannot copy symbolic link: {}", source.display()));
    }
    if destination.exists() {
        return Err(format!(
            "Destination already exists: {}",
            destination.display()
        ));
    }
    if move_file {
        fs::rename(source, destination).map_err(|e| e.to_string())
    } else if source.is_dir() {
        copy_directory(source, destination)
    } else {
        fs::copy(source, destination)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

pub fn copy_or_move_many(
    sources: &[PathBuf],
    folder: &Path,
    move_file: bool,
) -> Result<(), String> {
    if sources.is_empty() {
        return Err("Choose at least one source.".into());
    }
    if !folder.is_dir() {
        return Err(format!(
            "Destination folder does not exist: {}",
            folder.display()
        ));
    }
    let mut destinations = HashSet::new();
    for source in sources {
        if !source.exists() {
            return Err(format!("Source no longer exists: {}", source.display()));
        }
        if source.is_symlink() {
            return Err(format!("Cannot copy symbolic link: {}", source.display()));
        }
        let name = source
            .file_name()
            .ok_or_else(|| format!("Source has no file name: {}", source.display()))?;
        let destination = folder.join(name);
        check_destination(source, &destination)?;
        if !destinations.insert(destination.clone()) || destination.exists() {
            return Err(format!(
                "Destination already exists or is duplicated: {}",
                destination.display()
            ));
        }
    }
    for (index, source) in sources.iter().enumerate() {
        let destination = folder.join(source.file_name().expect("file name checked above"));
        copy_or_move(source, &destination, move_file).map_err(|error| {
            format!(
                "Completed {index}/{} items; {}: {error}",
                sources.len(),
                source.display()
            )
        })?;
    }
    Ok(())
}

fn check_destination(source: &Path, destination: &Path) -> Result<(), String> {
    if source == destination {
        return Err("Source and destination must differ.".into());
    }
    if source.is_dir() {
        let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        let parent = destination
            .parent()
            .ok_or("Destination needs a parent folder.")?;
        let parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
        if parent.starts_with(source) {
            return Err("Destination must be outside the source folder.".into());
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source = entry.path();
        let destination = destination.join(entry.file_name());
        if source.is_symlink() {
            return Err(format!("Cannot copy symbolic link: {}", source.display()));
        } else if source.is_dir() {
            copy_directory(&source, &destination)?;
        } else {
            fs::copy(&source, &destination).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn delete(source: &Path) -> Result<(), String> {
    if source.is_dir() {
        fs::remove_dir_all(source).map_err(|e| e.to_string())
    } else {
        fs::remove_file(source).map_err(|e| e.to_string())
    }
}

pub fn delete_many(sources: &[PathBuf]) -> Result<(), String> {
    if sources.is_empty() {
        return Err("Choose at least one item.".into());
    }
    for source in sources {
        if fs::symlink_metadata(source).is_err() {
            return Err(format!("Item no longer exists: {}", source.display()));
        }
    }
    for (index, source) in sources.iter().enumerate() {
        delete(source).map_err(|error| {
            format!(
                "Deleted {index}/{} items; {}: {error}",
                sources.len(),
                source.display()
            )
        })?;
    }
    Ok(())
}

fn count_bytes(path: &Path, progress: &ProgressHandle) -> Result<u64, OperationError> {
    let mut pending = vec![path.to_path_buf()];
    let mut bytes = 0u64;
    while let Some(path) = pending.pop() {
        progress.check()?;
        let metadata = fs::symlink_metadata(&path).map_err(failure)?;
        if metadata.file_type().is_symlink() {
            return Err(failure(format!(
                "Cannot copy symbolic link: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(failure)? {
                pending.push(entry.map_err(failure)?.path());
            }
        } else {
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok(bytes)
}

fn copy_path_progress(
    source: &Path,
    destination: &Path,
    total: u64,
    copied: &mut u64,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut buffer = vec![0u8; 128 * 1024];
    while let Some((source, destination)) = pending.pop() {
        progress.check()?;
        logging::detail(format!(
            "Copying {} to {}",
            source.display(),
            destination.display()
        ));
        let metadata = fs::symlink_metadata(&source).map_err(failure)?;
        if metadata.file_type().is_symlink() {
            return Err(failure(format!(
                "Cannot copy symbolic link: {}",
                source.display()
            )));
        }
        if metadata.is_dir() {
            fs::create_dir(&destination).map_err(failure)?;
            for entry in fs::read_dir(&source).map_err(failure)? {
                let entry = entry.map_err(failure)?;
                pending.push((entry.path(), destination.join(entry.file_name())));
            }
            continue;
        }
        let mut input = fs::File::open(&source).map_err(failure)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(failure)?;
        loop {
            progress.check()?;
            let count = input.read(&mut buffer).map_err(failure)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).map_err(failure)?;
            *copied = copied.saturating_add(count as u64);
            if total > 0 {
                progress.update(
                    *copied as f32 * 100. / total as f32,
                    source.display().to_string(),
                );
            }
        }
        output
            .set_permissions(metadata.permissions())
            .map_err(failure)?;
    }
    Ok(())
}

pub fn copy_or_move_with_progress(
    sources: &[PathBuf],
    destination: &Path,
    move_file: bool,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    if sources.is_empty() {
        return Err(failure("Choose at least one source."));
    }
    if sources.len() > 1 && !destination.is_dir() {
        return Err(failure(format!(
            "Destination folder does not exist: {}",
            destination.display()
        )));
    }
    let destinations: Vec<_> = sources
        .iter()
        .map(|source| {
            if sources.len() == 1 {
                destination.to_path_buf()
            } else {
                destination.join(source.file_name().unwrap_or_default())
            }
        })
        .collect();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for (source, target) in sources.iter().zip(&destinations) {
        progress.check()?;
        if !source.exists() {
            return Err(failure(format!(
                "Source no longer exists: {}",
                source.display()
            )));
        }
        check_destination(source, target)?;
        if !seen.insert(target.clone()) || target.exists() {
            return Err(failure(format!(
                "Destination already exists or is duplicated: {}",
                target.display()
            )));
        }
        if !move_file {
            total = total.saturating_add(count_bytes(source, progress)?);
        } else if source.is_symlink() {
            return Err(failure(format!(
                "Cannot move symbolic link: {}",
                source.display()
            )));
        }
    }
    let mut copied = 0u64;
    for (index, (source, target)) in sources.iter().zip(&destinations).enumerate() {
        progress.check()?;
        progress.update(
            if move_file {
                index as f32 * 100. / sources.len() as f32
            } else if total > 0 {
                copied as f32 * 100. / total as f32
            } else {
                0.
            },
            source.display().to_string(),
        );
        if move_file {
            logging::detail(format!(
                "Moving {} to {}",
                source.display(),
                target.display()
            ));
            fs::rename(source, target).map_err(failure)?;
        } else {
            copy_path_progress(source, target, total, &mut copied, progress)?;
        }
        if move_file || total == 0 {
            progress.update(
                (index + 1) as f32 * 100. / sources.len() as f32,
                source.display().to_string(),
            );
        }
    }
    progress.update(100., progress.snapshot().item);
    Ok(())
}

fn count_entries(path: &Path, progress: &ProgressHandle) -> Result<u64, OperationError> {
    progress.check()?;
    let mut count = 1u64;
    if fs::symlink_metadata(path)
        .map_err(failure)?
        .file_type()
        .is_dir()
    {
        for entry in fs::read_dir(path).map_err(failure)? {
            count = count.saturating_add(count_entries(&entry.map_err(failure)?.path(), progress)?);
        }
    }
    Ok(count)
}

fn delete_path_progress(
    path: &Path,
    total: u64,
    deleted: &mut u64,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    if fs::symlink_metadata(path)
        .map_err(failure)?
        .file_type()
        .is_dir()
    {
        for entry in fs::read_dir(path).map_err(failure)? {
            delete_path_progress(&entry.map_err(failure)?.path(), total, deleted, progress)?;
        }
        progress.check()?;
        fs::remove_dir(path).map_err(failure)?;
    } else {
        fs::remove_file(path).map_err(failure)?;
    }
    logging::detail(format!("Deleted {}", path.display()));
    *deleted += 1;
    progress.update(
        *deleted as f32 * 100. / total as f32,
        path.display().to_string(),
    );
    Ok(())
}

pub fn delete_many_with_progress(
    sources: &[PathBuf],
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    if sources.is_empty() {
        return Err(failure("Choose at least one item."));
    }
    let mut total = 0u64;
    for source in sources {
        total = total.saturating_add(count_entries(source, progress)?);
    }
    let mut deleted = 0u64;
    for source in sources {
        delete_path_progress(source, total, &mut deleted, progress)?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "../../tests/support/extraction.rs"]
mod extraction_tests;

#[cfg(test)]
#[path = "../../tests/support/seven_zip_progress.rs"]
mod seven_zip_progress_tests;
