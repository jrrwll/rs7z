use rs7z::model::config::TerminalConfig;
use rs7z::service::config::{self, Preferences, ThemeSource};
use std::fs;

#[test]
fn saving_preferences_preserves_handwritten_config_and_verbose() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rs7z.toml");
    fs::write(
        &path,
        "# Keep this note\ncustom_terminal = 'Terminal' # chosen by hand\ntheme = 'themes/apple.toml' # theme note\nverbose = true\nextra = 42\n",
    )
    .unwrap();
    let preferences =
        Preferences::new("iTerm2".into(), String::new(), "builtin:apple".into()).unwrap();
    config::save_preferences(&path, &preferences).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# Keep this note"));
    assert!(saved.contains("# chosen by hand"));
    assert!(saved.contains("# theme note"));
    assert!(saved.contains("verbose = true"));
    assert!(saved.contains("extra = 42"));
    assert_eq!(
        config::from_config(&path).unwrap(),
        TerminalConfig::App(rs7z::model::config::TerminalApp::ITerm2)
    );
    assert_eq!(
        config::theme_from_config(&path).unwrap(),
        Some(ThemeSource::Builtin("apple".into()))
    );
    assert_eq!(config::verbose_from_config(&path).unwrap(), true);
    let loaded = config::preferences_from_config(&path).unwrap();
    assert_eq!(loaded.terminal(), "iTerm2");
    assert_eq!(loaded.terminal_command(), "");
    assert_eq!(loaded.theme(), "builtin:apple");
}

#[test]
fn preferences_can_create_a_config_and_validate_custom_commands() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/config.toml");
    let command = "alacritty --working-directory '$1'";
    let preferences = Preferences::new(String::new(), command.into(), String::new()).unwrap();
    config::save_preferences(&path, &preferences).unwrap();
    assert_eq!(
        config::from_config(&path).unwrap(),
        TerminalConfig::Command(vec![
            "alacritty".into(),
            "--working-directory".into(),
            "$1".into()
        ])
    );
    let before = fs::read_to_string(&path).unwrap();
    assert!(Preferences::new(String::new(), "alacritty --cwd".into(), String::new()).is_err());
    assert!(Preferences::new(String::new(), "alacritty '".into(), String::new()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}
