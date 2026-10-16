//! Shared executable discovery for the official 7-Zip CLI.

use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const PROGRAM: &str = if cfg!(windows) { "7zz.exe" } else { "7zz" };

/// Create a UTF-8 command using RS7Z_7ZZ, a bundled executable, PATH, then
/// standard macOS Homebrew locations. RS7Z_7ZZ must be an absolute file path;
/// an invalid override is reported instead of silently selecting another CLI.
pub fn command() -> Result<Command, String> {
    let override_path = env::var_os("RS7Z_7ZZ");
    let executable = env::current_exe().ok();
    let search_path = env::var_os("PATH");
    let fallback: &[&Path] = if cfg!(target_os = "macos") {
        &[Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")]
    } else {
        &[]
    };
    let program = resolve(
        override_path.as_deref(),
        executable.as_deref(),
        search_path.as_deref(),
        fallback,
    )?;
    let mut command = Command::new(program);
    command.arg("-sccUTF-8");
    Ok(command)
}

/// RAR headers are readable even in builds that omit the actual decoders.
/// Check codecs, not the format list, before offering RAR operations.
pub(crate) fn rar_command() -> Result<Command, String> {
    let command = command()?;
    let output = Command::new(command.get_program())
        .args(["i", "-sccUTF-8"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Cannot inspect 7zz RAR support: {error}"))?;
    if !output.status.success() || !has_rar_decoders(&String::from_utf8_lossy(&output.stdout)) {
        return Err(format!(
            "This 7zz build does not include full RAR decoding: {}. Use the complete official 7-Zip distribution from https://7-zip.org/download.html and set RS7Z_7ZZ to its executable. Homebrew builds may omit RAR decoders.",
            Path::new(command.get_program()).display()
        ));
    }
    Ok(command)
}

fn has_rar_decoders(info: &str) -> bool {
    let Some((_, codecs)) = info.split_once("Codecs:") else {
        return false;
    };
    let codecs = codecs.split("Hashers:").next().unwrap_or_default();
    ["Rar1", "Rar2", "Rar3", "Rar5"].iter().all(|name| {
        codecs.lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields.last() == Some(name) && fields.iter().any(|field| field.contains('D'))
        })
    })
}

fn executable(path: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    // Callers change current_dir when repacking; keep resolution independent of it.
    fs::canonicalize(path).ok()
}

fn resolve(
    override_path: Option<&OsStr>,
    app: Option<&Path>,
    search_path: Option<&OsStr>,
    fallback: &[&Path],
) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        let path = Path::new(path);
        if !path.is_absolute() {
            return Err("RS7Z_7ZZ must be an absolute path to the official 7zz executable.".into());
        }
        return executable(path)
            .ok_or_else(|| format!("RS7Z_7ZZ is not an executable file: {}", path.display()));
    }
    if let Some(path) = app
        .and_then(Path::parent)
        .and_then(|directory| executable(&directory.join(PROGRAM)))
    {
        return Ok(path);
    }
    if let Some(path) = search_path {
        for directory in env::split_paths(path) {
            if let Some(path) = executable(&directory.join(PROGRAM)) {
                return Ok(path);
            }
        }
    }
    for directory in fallback {
        if let Some(path) = executable(&directory.join(PROGRAM)) {
            return Ok(path);
        }
    }
    Err("Cannot find the official 7-Zip CLI (7zz). Download the complete distribution from https://7-zip.org/download.html, place 7zz beside rs7z, or set RS7Z_7ZZ to its absolute path.".into())
}

#[cfg(test)]
#[path = "../../tests/support/seven_zip_cli.rs"]
mod tests;
