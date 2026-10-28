use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{TestAppContext, px, rgb};
use rs7z::service::config::{self, ThemeSource};
use rs7z::theme::{BUILTIN_THEMES, ThemeFile};
use std::fs;

const APPLE: &str = include_str!("../themes/apple.toml");

#[test]
fn theme_paths_are_relative_to_the_config_and_optional() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rs7z.toml");
    fs::write(
        &path,
        "theme = 'themes/apple.toml'\ncustom_terminal = 'invalid'",
    )
    .unwrap();
    assert_eq!(
        config::theme_from_config(&path).unwrap(),
        Some(ThemeSource::File(
            directory.path().join("themes/apple.toml")
        ))
    );
    let absolute = directory.path().join("my theme.toml");
    let mut document = toml::Table::new();
    document.insert("theme".into(), absolute.display().to_string().into());
    fs::write(&path, toml::to_string(&document).unwrap()).unwrap();
    assert_eq!(
        config::theme_from_config(&path).unwrap(),
        Some(ThemeSource::File(absolute))
    );
    fs::write(&path, "theme = 'builtin:apple'").unwrap();
    assert_eq!(
        config::theme_from_config(&path).unwrap(),
        Some(ThemeSource::Builtin("apple".into()))
    );
    for text in ["", "theme = ''", "theme = '  '", "verbose = true"] {
        fs::write(&path, text).unwrap();
        assert_eq!(config::theme_from_config(&path).unwrap(), None);
    }
    fs::write(&path, "theme = 1").unwrap();
    assert!(config::theme_from_config(&path).is_err());
    fs::write(&path, "theme = [").unwrap();
    assert!(config::theme_from_config(&path).is_err());
}

#[test]
fn theme_files_reject_invalid_values_instead_of_partly_applying() {
    ThemeFile::parse(APPLE).unwrap();
    for (id, _) in BUILTIN_THEMES {
        ThemeFile::builtin(id).unwrap();
    }
    assert!(ThemeFile::builtin("missing").is_err());
    for text in [
        "",
        "name = ''",
        "name = 'Broken'\nmode = 'system'",
        "name = 'Broken'\nfont.size = 16", // Dotted keys must be quoted.
        "name = 'Broken'\n'font.size' = 0",
        "name = 'Broken'\n'font.size' = nan",
        "name = 'Broken'\n'mono_font.size' = -1",
        "name = 'Broken'\nradius = -1",
        "name = 'Broken'\nshadow = 'false'",
        "name = 'Broken'\n[colors]\nforeground = 'oops'",
        "name = 'Broken'\n[colors]\nforegrounnd = '#ffffff'",
        "name = 'Broken'\n[colors]\nbackground = 1",
    ] {
        assert!(ThemeFile::parse(text).is_err(), "accepted: {text}");
    }
    let directory = tempfile::tempdir().unwrap();
    assert!(ThemeFile::load(&directory.path().join("missing.toml")).is_err());
}

#[gpui_kit::test]
fn apple_reaches_components_and_base_and_changes_only_when_loaded(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("apple.toml");
    fs::write(&path, APPLE).unwrap();
    let theme = ThemeFile::load(&path).unwrap();
    // Editing the file has no effect until the next load (normally the next startup).
    fs::write(&path, APPLE.replace("#0066cc", "#885522")).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        theme.apply(cx);
        let active = Theme::global(cx);
        assert_eq!(active.theme_name().as_ref(), "Apple");
        assert_eq!(active.mode, ThemeMode::Light);
        assert_eq!(active.primary, rgb(0x0066cc).into());
        assert_eq!(active.sidebar, rgb(0xf5f5f7).into());
        assert_eq!(active.foreground, rgb(0x1d1d1f).into());
        assert_eq!(active.font_size, px(16.));
        assert_eq!(active.radius, px(8.));
        assert_eq!(active.radius_lg, px(18.));
        assert!(!active.shadow);
        assert!(active.focus_ring);
        assert_eq!(active.tokens.sidebar.color, active.sidebar);
        assert_eq!(active.tokens.primary.color, active.primary);
        let base = gpui_kit::base::Theme::global(cx);
        assert_eq!(base.tokens.colors.primary, active.primary);
        assert_eq!(base.tokens.radius.md, px(8.));
        ThemeFile::load(&path).unwrap().apply(cx);
        assert_eq!(Theme::global(cx).primary, rgb(0x885522).into());
        // A different, partial theme inherits the matching framework mode, not Apple colors.
        ThemeFile::parse("name = 'Another'\nmode = 'dark'")
            .unwrap()
            .apply(cx);
        assert_eq!(Theme::global(cx).mode, ThemeMode::Dark);
        assert_ne!(Theme::global(cx).background, rgb(0xffffff).into());
    });
}

#[gpui_kit::test]
fn bundled_brand_themes_apply_their_own_mode_and_selection_palette(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for (id, name, mode, background, primary, selected_text) in [
            (
                "opencode.ai",
                "OpenCode",
                ThemeMode::Light,
                0xfdfcfc,
                0x201d1d,
                0xfdfcfc,
            ),
            (
                "claude",
                "Claude",
                ThemeMode::Light,
                0xfaf9f5,
                0xcc785c,
                0x141413,
            ),
            (
                "nvidia",
                "NVIDIA",
                ThemeMode::Light,
                0xffffff,
                0x76b900,
                0x000000,
            ),
            (
                "tesla",
                "Tesla",
                ThemeMode::Light,
                0xffffff,
                0x3e6ae1,
                0xffffff,
            ),
            (
                "x.ai",
                "x.ai",
                ThemeMode::Dark,
                0x0a0a0a,
                0xffffff,
                0x0a0a0a,
            ),
            (
                "binance",
                "Binance",
                ThemeMode::Dark,
                0x0b0e11,
                0xfcd535,
                0x181a20,
            ),
            (
                "coinbase",
                "Coinbase",
                ThemeMode::Light,
                0xffffff,
                0x0052ff,
                0xffffff,
            ),
            (
                "playstation",
                "PlayStation",
                ThemeMode::Light,
                0xffffff,
                0x0070d1,
                0xffffff,
            ),
            (
                "pinterest",
                "Pinterest",
                ThemeMode::Light,
                0xffffff,
                0xe60023,
                0xffffff,
            ),
            (
                "notion",
                "Notion",
                ThemeMode::Light,
                0xffffff,
                0x5645d4,
                0xffffff,
            ),
            (
                "starbucks",
                "Starbucks",
                ThemeMode::Light,
                0xf2f0eb,
                0x00754a,
                0xffffff,
            ),
            (
                "zapier",
                "Zapier",
                ThemeMode::Light,
                0xfffefb,
                0xff4f00,
                0x201515,
            ),
        ] {
            let theme = ThemeFile::builtin(id).unwrap();
            theme.apply(cx);
            let active = Theme::global(cx);
            assert_eq!(active.theme_name().as_ref(), name);
            assert_eq!(active.mode, mode, "{name}");
            assert_eq!(active.background, rgb(background).into(), "{name}");
            assert_eq!(active.primary, rgb(primary).into(), "{name}");
            assert_eq!(
                active.primary_foreground,
                rgb(selected_text).into(),
                "{name}"
            );
            assert_eq!(active.sidebar_accent, active.primary, "{name}");
            // GPUI Kit caps list/table highlights at 20% opacity.
            assert_eq!(active.list_active, active.primary.alpha(0.2), "{name}");
            assert_eq!(active.table_active, active.primary.alpha(0.2), "{name}");
            assert_eq!(active.tokens.primary.color, active.primary, "{name}");
            assert_eq!(
                gpui_kit::base::Theme::global(cx).tokens.colors.primary,
                active.primary,
                "{name}"
            );
        }
    });
}
