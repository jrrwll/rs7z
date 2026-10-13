use crate::model::{
    archive::{ArchiveFormat, ArchiveOptions},
    operations::OperationError,
};
use crate::service::{
    logging,
    operations::{self, ProgressHandle},
    seven_zip_cli, zip,
};
use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[derive(Clone, Copy)]
enum Tool {
    Xz,
    Zstd,
}

impl Tool {
    fn for_format(format: ArchiveFormat) -> Self {
        match format {
            ArchiveFormat::TarLzma => Self::Xz,
            ArchiveFormat::TarZstd => Self::Zstd,
            _ => unreachable!("system stream format required"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Xz => "xz",
            Self::Zstd => "zstd",
        }
    }

    fn command(self) -> Result<Command, OperationError> {
        let fallback: &[&Path] = if cfg!(target_os = "macos") {
            &[Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")]
        } else {
            &[]
        };
        let path = find_executable(self, env::var_os("PATH").as_deref(), fallback)
            .ok_or_else(|| self.missing())?;
        let mut command = Command::new(path);
        // User shell defaults must not alter the requested format or level.
        for key in ["XZ_DEFAULTS", "XZ_OPT", "ZSTD_CLEVEL", "ZSTD_NBTHREADS"] {
            command.env_remove(key);
        }
        Ok(command)
    }

    fn missing(self) -> OperationError {
        let release = if cfg!(target_os = "linux") {
            fs::read_to_string("/etc/os-release").unwrap_or_default()
        } else {
            String::new()
        };
        OperationError::Failed(missing_message(self, env::consts::OS, &release))
    }
}

fn find_executable(tool: Tool, search_path: Option<&OsStr>, fallback: &[&Path]) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{}.exe", tool.name())
    } else {
        tool.name().to_owned()
    };
    let directories = search_path
        .map(env::split_paths)
        .into_iter()
        .flatten()
        .chain(fallback.iter().map(|path| path.to_path_buf()));
    for directory in directories {
        let path = directory.join(&name);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        // Keep resolution stable even if a caller changes the working directory.
        if let Ok(path) = fs::canonicalize(path) {
            return Some(path);
        }
    }
    None
}

fn missing_message(tool: Tool, os: &str, release: &str) -> String {
    let name = tool.name();
    let install = match os {
        "macos" => "brew install xz zstd".to_owned(),
        "linux" => {
            let ids: Vec<_> = release
                .lines()
                .filter_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    matches!(key, "ID" | "ID_LIKE").then(|| value.trim_matches(['\"', '\'']))
                })
                .flat_map(str::split_whitespace)
                .collect();
            let package = if matches!(tool, Tool::Xz) {
                "xz-utils"
            } else {
                "zstd"
            };
            if ids.iter().any(|id| matches!(*id, "debian" | "ubuntu")) {
                format!("sudo apt install {package}")
            } else if ids
                .iter()
                .any(|id| matches!(*id, "fedora" | "rhel" | "centos" | "rocky" | "almalinux"))
            {
                format!("sudo dnf install {name}")
            } else if ids.iter().any(|id| matches!(*id, "arch" | "manjaro")) {
                format!("sudo pacman -S {name}")
            } else if ids.iter().any(|id| {
                matches!(
                    *id,
                    "suse" | "opensuse" | "opensuse-leap" | "opensuse-tumbleweed"
                )
            }) {
                format!("sudo zypper install {name}")
            } else if ids.contains(&"alpine") {
                format!("sudo apk add {name}")
            } else {
                format!(
                    "Choose the command for your distribution:\n\nDebian / Ubuntu:\nsudo apt install {package}\n\nFedora / RHEL:\nsudo dnf install {name}\n\nArch Linux:\nsudo pacman -S {name}"
                )
            }
        }
        "windows" => "With Scoop installed:\nscoop install xz zstd".to_owned(),
        _ => format!("Install {name} with your system package manager."),
    };
    format!(
        "Required command '{name}' is not installed or could not be found.\n\nInstall it, then try again:\n\n{install}\n\nMake sure '{name}' is available in PATH. Restart rs7z if PATH changes."
    )
}

/// Resolve dependencies before creating a compression workspace or rewriting TAR.
pub(super) fn require_tool(format: ArchiveFormat) -> Result<(), OperationError> {
    if matches!(format, ArchiveFormat::TarLzma | ArchiveFormat::TarZstd) {
        Tool::for_format(format).command()?;
    }
    Ok(())
}

pub(super) fn decode(
    path: &Path,
    output: &Path,
    format: ArchiveFormat,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    let codec = match format {
        ArchiveFormat::TarLzma => "-tlzma",
        ArchiveFormat::TarZstd => "-tzstd",
        _ => unreachable!("compressed stream format required"),
    };
    let mut command = seven_zip_cli::command()?;
    command.args(["x", codec, "-so", "-y", "--"]).arg(path);
    operations::run_archive_command_to_file(
        command,
        File::create(output).map_err(failed)?,
        progress,
    )
}

pub(super) fn encode(
    path: &Path,
    output: &Path,
    format: ArchiveFormat,
    options: &ArchiveOptions,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    let tool = Tool::for_format(format);
    let mut command = tool.command()?;
    let level = options.compression_level.map(zip::compression_level);
    match tool {
        Tool::Xz => {
            command.args(["--compress", "--stdout", "--format=lzma"]);
            command.arg(format!("-{}", level.unwrap_or(6)));
        }
        Tool::Zstd => {
            command.args(["--compress", "--stdout", "--quiet", "--check"]);
            // The UI's Store level (0) mapped to zstd's default in the native codec.
            command.arg(format!(
                "-{}",
                level.filter(|level| *level > 0).unwrap_or(3)
            ));
        }
    }
    command.arg("--").arg(path);
    run_stream(command, tool, output, progress)
}

fn failed(error: impl std::fmt::Display) -> OperationError {
    OperationError::Failed(error.to_string())
}

fn run_stream(
    mut command: Command,
    tool: Tool,
    output: &Path,
    progress: &ProgressHandle,
) -> Result<(), OperationError> {
    progress.check()?;
    logging::detail(format!(
        "Running system codec: {}",
        zip::command_description(&command)
    ));
    // File-backed stderr cannot deadlock a child and keeps binary stdout separate.
    let mut errors = tempfile::tempfile().map_err(failed)?;
    command
        .stdin(Stdio::null())
        .stdout(File::create(output).map_err(failed)?)
        .stderr(errors.try_clone().map_err(failed)?);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            tool.missing()
        } else {
            failed(format!("Cannot start '{}': {error}", tool.name()))
        }
    })?;
    progress.update(0., output.display().to_string());
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
    let length = errors.metadata().map_err(failed)?.len();
    errors
        .seek(SeekFrom::Start(length.saturating_sub(16 * 1024)))
        .map_err(failed)?;
    let mut bytes = Vec::new();
    errors
        .take(16 * 1024)
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    let detail = String::from_utf8_lossy(&bytes);
    if !detail.trim().is_empty() {
        logging::detail(format!("{}: {}", tool.name(), detail.trim()));
    }
    if !status.success() {
        return Err(failed(format!(
            "{} failed ({status}):\n{}",
            tool.name(),
            detail.trim()
        )));
    }
    progress.update(100., output.display().to_string());
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/support/compression.rs"]
mod tests;
