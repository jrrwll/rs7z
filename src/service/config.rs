use crate::model::config::{TerminalApp, TerminalConfig};
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::{Command, Stdio},
};

pub fn from_config(path: &Path) -> Result<TerminalConfig, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("Invalid TOML in {}: {e}", path.display()))?;

    if let Some(value) = config.get("custom_terminal_cmd") {
        let command = value
            .as_str()
            .ok_or("custom_terminal_cmd must be a string")?;
        if !command.trim().is_empty() {
            let args = shlex::split(command)
                .filter(|args| !args.is_empty())
                .ok_or("custom_terminal_cmd has invalid quoting")?;
            if !args.iter().any(|arg| arg.contains("$1")) {
                return Err("custom_terminal_cmd must contain $1".into());
            }
            return Ok(TerminalConfig::Command(args));
        }
    }

    match config.get("custom_terminal") {
        None => Ok(default_for_platform()),
        Some(toml::Value::String(name)) if name.trim().is_empty() => Ok(TerminalConfig::Disabled),
        Some(toml::Value::String(name)) => parse_app(name).map(TerminalConfig::App),
        Some(_) => Err("custom_terminal must be a string".into()),
    }
}

pub fn from_env() -> Result<TerminalConfig, String> {
    match std::env::var_os("RS7Z_CONFIG") {
        Some(path) => from_config(Path::new(&path)),
        None => Ok(default_for_platform()),
    }
}

pub fn verbose_from_config(path: &Path) -> Result<bool, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("Invalid TOML in {}: {e}", path.display()))?;
    match config.get("verbose") {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "verbose must be a boolean".into()),
    }
}

pub fn verbose_from_env() -> Result<bool, String> {
    let environment = std::env::var("RS7Z_VERBOSE")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    let configured = match std::env::var_os("RS7Z_CONFIG") {
        Some(path) => verbose_from_config(Path::new(&path))?,
        None => false,
    };
    Ok(environment || configured)
}

pub fn is_available(config: &TerminalConfig) -> bool {
    !matches!(config, TerminalConfig::Disabled)
}

pub fn open_directory(config: &TerminalConfig, path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("Not a directory: {}", path.display()));
    }
    let (program, args) = command_for(config, path)?;
    crate::service::logging::detail(format!("Opening terminal: {:?} {:?}", program, args));
    if cfg!(target_os = "macos") && matches!(config, TerminalConfig::App(_)) {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("Cannot open terminal: {e}"))?;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            crate::service::logging::detail(format!("Terminal: {line}"));
        }
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stderr).lines() {
                crate::service::logging::detail(format!("Terminal stderr: {line}"));
            }
            Ok(())
        } else {
            Err(format!(
                "Cannot open terminal: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    } else {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(if crate::service::logging::is_verbose() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Cannot open terminal: {e}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

fn default_for_platform() -> TerminalConfig {
    if cfg!(target_os = "macos") {
        TerminalConfig::App(TerminalApp::Terminal)
    } else {
        TerminalConfig::Disabled
    }
}

fn parse_app(name: &str) -> Result<TerminalApp, String> {
    let app = if cfg!(target_os = "macos") {
        match name {
            "Terminal" => Some(TerminalApp::Terminal),
            "iTerm2" => Some(TerminalApp::ITerm2),
            "Warp" => Some(TerminalApp::Warp),
            "Alacritty" => Some(TerminalApp::Alacritty),
            "Kitty" => Some(TerminalApp::Kitty),
            "WezTerm" => Some(TerminalApp::WezTerm),
            "Ghostty" => Some(TerminalApp::Ghostty),
            _ => None,
        }
    } else if cfg!(target_os = "linux") {
        match name {
            "gnome" => Some(TerminalApp::Gnome),
            "kde" => Some(TerminalApp::Kde),
            "xfce4" => Some(TerminalApp::Xfce4),
            "kitty" => Some(TerminalApp::Kitty),
            "alacritty" => Some(TerminalApp::Alacritty),
            "wezterm" => Some(TerminalApp::WezTerm),
            "ghostty" => Some(TerminalApp::Ghostty),
            _ => None,
        }
    } else {
        None
    };
    app.ok_or_else(|| format!("Unsupported custom_terminal: {name}"))
}

/// Build the command to open a directory without executing it.
pub fn command_for(
    config: &TerminalConfig,
    path: &Path,
) -> Result<(OsString, Vec<OsString>), String> {
    let args = match config {
        TerminalConfig::Disabled => {
            return Err("Configure custom_terminal or custom_terminal_cmd".into());
        }
        TerminalConfig::Command(args) => {
            let mut args = args.iter();
            let program = args.next().ok_or("custom_terminal_cmd is empty")?;
            return Ok((
                substitute_path(program, path),
                args.map(|arg| substitute_path(arg, path)).collect(),
            ));
        }
        TerminalConfig::App(app) if cfg!(target_os = "macos") => mac_command(*app)?,
        TerminalConfig::App(app) if cfg!(target_os = "linux") => linux_command(*app)?,
        TerminalConfig::App(_) => return Err("Terminal is unavailable on this platform".into()),
    };
    let (program, args) = args.split_first().expect("terminal command has a program");
    Ok((
        OsString::from(program),
        args.iter().map(|arg| substitute_path(arg, path)).collect(),
    ))
}

fn mac_command(app: TerminalApp) -> Result<Vec<&'static str>, String> {
    match app {
        TerminalApp::Terminal => Ok(vec!["open", "-a", "Terminal", "$1"]),
        TerminalApp::ITerm2 => Ok(vec!["open", "-a", "iTerm", "$1"]),
        TerminalApp::Warp => Ok(vec!["open", "-a", "Warp", "$1"]),
        TerminalApp::Alacritty => Ok(vec![
            "open",
            "-n",
            "-a",
            "Alacritty",
            "--args",
            "--working-directory",
            "$1",
        ]),
        TerminalApp::Kitty => Ok(vec!["open", "-n", "-a", "kitty", "--args", "-d", "$1"]),
        TerminalApp::WezTerm => Ok(vec![
            "open", "-n", "-a", "WezTerm", "--args", "start", "--cwd", "$1",
        ]),
        TerminalApp::Ghostty => Ok(vec![
            "open",
            "-n",
            "-a",
            "Ghostty",
            "--args",
            "--working-directory",
            "$1",
        ]),
        _ => Err("Terminal is unavailable on this platform".into()),
    }
}

fn linux_command(app: TerminalApp) -> Result<Vec<&'static str>, String> {
    match app {
        TerminalApp::Gnome => Ok(vec!["gnome-terminal", "--working-directory=$1"]),
        TerminalApp::Kde => Ok(vec!["konsole", "--workdir", "$1"]),
        TerminalApp::Xfce4 => Ok(vec!["xfce4-terminal", "--working-directory=$1"]),
        TerminalApp::Kitty => Ok(vec!["kitty", "-d", "$1"]),
        TerminalApp::Alacritty => Ok(vec!["alacritty", "--working-directory", "$1"]),
        TerminalApp::WezTerm => Ok(vec!["wezterm", "start", "--cwd", "$1"]),
        TerminalApp::Ghostty => Ok(vec!["ghostty", "--working-directory=$1"]),
        _ => Err("Terminal is unavailable on this platform".into()),
    }
}

fn substitute_path(argument: &str, path: &Path) -> OsString {
    let mut result = OsString::new();
    for (index, part) in argument.split("$1").enumerate() {
        if index > 0 {
            result.push(path.as_os_str());
        }
        result.push(part);
    }
    result
}
