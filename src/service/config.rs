use crate::model::config::{TerminalApp, TerminalConfig};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use toml_edit::{DocumentMut, value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeSource {
    Builtin(String),
    File(PathBuf),
}

pub struct Preferences {
    terminal: String,
    terminal_command: String,
    theme: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            terminal: if cfg!(target_os = "macos") {
                "Terminal"
            } else {
                ""
            }
            .into(),
            terminal_command: String::new(),
            theme: String::new(),
        }
    }
}

impl Preferences {
    pub fn new(terminal: String, terminal_command: String, theme: String) -> Result<Self, String> {
        let preferences = Self {
            terminal,
            terminal_command,
            theme,
        };
        preferences.terminal_config()?;
        Ok(preferences)
    }

    pub fn terminal(&self) -> &str {
        &self.terminal
    }
    pub fn terminal_command(&self) -> &str {
        &self.terminal_command
    }
    pub fn theme(&self) -> &str {
        &self.theme
    }

    pub fn terminal_config(&self) -> Result<TerminalConfig, String> {
        if !self.terminal_command.trim().is_empty() {
            return parse_command(&self.terminal_command);
        }
        if self.terminal.is_empty() {
            Ok(TerminalConfig::Disabled)
        } else {
            parse_app(&self.terminal).map(TerminalConfig::App)
        }
    }
}

pub fn terminal_names() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &[
            "Terminal",
            "iTerm2",
            "Warp",
            "Alacritty",
            "Kitty",
            "WezTerm",
            "Ghostty",
        ]
    } else if cfg!(target_os = "linux") {
        &[
            "gnome",
            "kde",
            "xfce4",
            "kitty",
            "alacritty",
            "wezterm",
            "ghostty",
        ]
    } else {
        &[]
    }
}

pub fn config_path() -> Result<PathBuf, String> {
    let home = std::env::home_dir().ok_or("Cannot determine the home directory")?;
    let directory = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    };
    Ok(directory.join("rs7z/config.toml"))
}

pub fn preferences_from_config(path: &Path) -> Result<Preferences, String> {
    let document = read_config(path)?;
    let string = |key: &str| match document.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a string")),
    };
    Ok(Preferences {
        terminal: string("custom_terminal")?
            .unwrap_or(if cfg!(target_os = "macos") {
                "Terminal"
            } else {
                ""
            })
            .into(),
        terminal_command: string("custom_terminal_cmd")?.unwrap_or("").into(),
        theme: string("theme")?.unwrap_or("").into(),
    })
}

pub fn preferences_from_env() -> Result<Preferences, String> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Preferences::default());
    }
    preferences_from_config(&path)
}

pub fn save_preferences(path: &Path, preferences: &Preferences) -> Result<(), String> {
    preferences.terminal_config()?;
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
    };
    let mut document: DocumentMut = content
        .parse()
        .map_err(|error| format!("Invalid TOML in {}: {error}", path.display()))?;
    set_string(&mut document, "custom_terminal", preferences.terminal());
    if !preferences.terminal_command().is_empty() || document.contains_key("custom_terminal_cmd") {
        set_string(
            &mut document,
            "custom_terminal_cmd",
            preferences.terminal_command(),
        );
    }
    if !preferences.theme().is_empty() || document.contains_key("theme") {
        set_string(&mut document, "theme", preferences.theme());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(document.to_string().as_bytes())
        .and_then(|()| temporary.flush())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

fn set_string(document: &mut DocumentMut, key: &str, new_value: &str) {
    let decoration = document
        .get(key)
        .and_then(|item| item.as_value())
        .map(|value| value.decor().clone());
    document[key] = value(new_value);
    if let Some(decoration) = decoration {
        *document[key]
            .as_value_mut()
            .expect("string value")
            .decor_mut() = decoration;
    }
}

fn read_config(path: &Path) -> Result<toml::Value, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    toml::from_str(&content).map_err(|e| format!("Invalid TOML in {}: {e}", path.display()))
}

fn parse_command(command: &str) -> Result<TerminalConfig, String> {
    let args = shlex::split(command)
        .filter(|args| !args.is_empty())
        .ok_or("custom_terminal_cmd has invalid quoting")?;
    if !args.iter().any(|arg| arg.contains("$1")) {
        return Err("custom_terminal_cmd must contain $1".into());
    }
    Ok(TerminalConfig::Command(args))
}

pub fn from_config(path: &Path) -> Result<TerminalConfig, String> {
    let config = read_config(path)?;

    if let Some(value) = config.get("custom_terminal_cmd") {
        let command = value
            .as_str()
            .ok_or("custom_terminal_cmd must be a string")?;
        if !command.trim().is_empty() {
            return parse_command(command);
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
    let path = config_path()?;
    if !path.exists() {
        Ok(default_for_platform())
    } else {
        from_config(&path)
    }
}

pub fn verbose_from_config(path: &Path) -> Result<bool, String> {
    let config = read_config(path)?;
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
    // Keep diagnostics available even when the configuration cannot be read.
    if environment {
        return Ok(true);
    }
    let path = config_path()?;
    let configured = if !path.exists() {
        false
    } else {
        verbose_from_config(&path)?
    };
    Ok(configured)
}

/// Relative theme paths belong to the config file, independent of the browsed directory.
pub fn theme_from_config(path: &Path) -> Result<Option<ThemeSource>, String> {
    let config = read_config(path)?;
    match config.get("theme") {
        None => Ok(None),
        Some(toml::Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(toml::Value::String(value)) if value.starts_with("builtin:") => {
            Ok(Some(ThemeSource::Builtin(value[8..].to_owned())))
        }
        Some(toml::Value::String(value)) => {
            let theme = Path::new(value);
            Ok(Some(ThemeSource::File(if theme.is_absolute() {
                theme.to_owned()
            } else {
                path.parent().unwrap_or(Path::new(".")).join(theme)
            })))
        }
        Some(_) => Err("theme must be a theme file path string".into()),
    }
}

pub fn theme_from_env() -> Result<Option<ThemeSource>, String> {
    let path = config_path()?;
    if !path.exists() {
        Ok(None)
    } else {
        theme_from_config(&path)
    }
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
