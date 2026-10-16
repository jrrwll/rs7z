use rs7z::model::config::{TerminalApp, TerminalConfig};
use rs7z::service::config;
use std::{ffi::OsString, fs, path::Path};

#[test]
fn config_values() {
    let path = std::env::temp_dir().join(format!("rs7z-config-{}.toml", std::process::id()));
    fs::write(&path, "custom_terminal = ''\n").unwrap();
    assert_eq!(
        config::from_config(&path).unwrap(),
        TerminalConfig::Disabled
    );
    fs::write(&path, "custom_terminal_cmd = 'gnome-terminal --working-directory=$1'\ncustom_terminal = 'invalid'\n").unwrap();
    assert_eq!(
        config::from_config(&path).unwrap(),
        TerminalConfig::Command(vec![
            "gnome-terminal".into(),
            "--working-directory=$1".into()
        ])
    );
    fs::write(&path, "custom_terminal_cmd = 'gnome-terminal'\n").unwrap();
    assert!(config::from_config(&path).is_err());
    fs::write(&path, "custom_terminal = 1\n").unwrap();
    assert!(config::from_config(&path).is_err());
    fs::write(&path, "default_terminal = 'iTerm2'\n").unwrap();
    let default = if cfg!(target_os = "macos") {
        TerminalConfig::App(TerminalApp::Terminal)
    } else {
        TerminalConfig::Disabled
    };
    assert_eq!(config::from_config(&path).unwrap(), default);
    #[cfg(target_os = "macos")]
    {
        fs::write(&path, "custom_terminal = 'iTerm2'\n").unwrap();
        assert_eq!(
            config::from_config(&path).unwrap(),
            TerminalConfig::App(TerminalApp::ITerm2)
        );
    }
    #[cfg(target_os = "linux")]
    {
        fs::write(&path, "custom_terminal = 'gnome'\n").unwrap();
        assert_eq!(
            config::from_config(&path).unwrap(),
            TerminalConfig::App(TerminalApp::Gnome)
        );
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn verbose_requires_standard_toml_boolean() {
    let path = std::env::temp_dir().join(format!("rs7z-verbose-{}.toml", std::process::id()));
    fs::write(&path, "verbose = true\n").unwrap();
    assert!(config::verbose_from_config(&path).unwrap());
    fs::write(&path, "verbose = false\n").unwrap();
    assert!(!config::verbose_from_config(&path).unwrap());
    fs::write(&path, "custom_terminal = ''\n").unwrap();
    assert!(!config::verbose_from_config(&path).unwrap());
    fs::write(&path, "verbose = 'true'\n").unwrap();
    assert!(config::verbose_from_config(&path).is_err());
    fs::write(&path, "verbose = True\n").unwrap();
    assert!(config::verbose_from_config(&path).is_err());
    fs::remove_file(path).unwrap();
}

#[test]
fn substitutes_path_as_one_argument() {
    let path = Path::new("/tmp/a b;$(touch nope)");
    let terminal = TerminalConfig::Command(vec![
        "gnome-terminal".into(),
        "--working-directory=$1".into(),
    ]);
    let (program, args) = config::command_for(&terminal, path).unwrap();
    assert_eq!(program, "gnome-terminal");
    assert_eq!(
        args,
        [OsString::from("--working-directory=/tmp/a b;$(touch nope)")]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn mac_presets_use_open_with_directory() {
    for (app, expected) in [
        (TerminalApp::Terminal, vec!["-a", "Terminal", "/tmp/a b"]),
        (TerminalApp::ITerm2, vec!["-a", "iTerm", "/tmp/a b"]),
        (TerminalApp::Warp, vec!["-a", "Warp", "/tmp/a b"]),
        (
            TerminalApp::Alacritty,
            vec![
                "-n",
                "-a",
                "Alacritty",
                "--args",
                "--working-directory",
                "/tmp/a b",
            ],
        ),
        (
            TerminalApp::Kitty,
            vec!["-n", "-a", "kitty", "--args", "-d", "/tmp/a b"],
        ),
        (
            TerminalApp::WezTerm,
            vec![
                "-n", "-a", "WezTerm", "--args", "start", "--cwd", "/tmp/a b",
            ],
        ),
        (
            TerminalApp::Ghostty,
            vec![
                "-n",
                "-a",
                "Ghostty",
                "--args",
                "--working-directory",
                "/tmp/a b",
            ],
        ),
    ] {
        let (program, args) =
            config::command_for(&TerminalConfig::App(app), Path::new("/tmp/a b")).unwrap();
        assert_eq!(program, "open");
        assert_eq!(args, expected);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_presets_pass_directory() {
    for (app, expected) in [
        (
            TerminalApp::Gnome,
            vec!["gnome-terminal", "--working-directory=/tmp/a b"],
        ),
        (TerminalApp::Kde, vec!["konsole", "--workdir", "/tmp/a b"]),
        (
            TerminalApp::Xfce4,
            vec!["xfce4-terminal", "--working-directory=/tmp/a b"],
        ),
        (TerminalApp::Kitty, vec!["kitty", "-d", "/tmp/a b"]),
        (
            TerminalApp::Alacritty,
            vec!["alacritty", "--working-directory", "/tmp/a b"],
        ),
        (
            TerminalApp::WezTerm,
            vec!["wezterm", "start", "--cwd", "/tmp/a b"],
        ),
        (
            TerminalApp::Ghostty,
            vec!["ghostty", "--working-directory=/tmp/a b"],
        ),
    ] {
        let (program, args) =
            config::command_for(&TerminalConfig::App(app), Path::new("/tmp/a b")).unwrap();
        assert_eq!(program, expected[0]);
        assert_eq!(args, expected[1..]);
    }
}
