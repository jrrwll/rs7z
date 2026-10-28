//! Startup-only TOML themes using GPUI Kit's own theme schema.
use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeConfig, try_parse_color};
use std::{fs, path::Path, rc::Rc};

pub const BUILTIN_THEMES: &[(&str, &str)] = &[
    ("apple", "Apple"),
    ("binance", "Binance"),
    ("claude", "Claude"),
    ("coinbase", "Coinbase"),
    ("notion", "Notion"),
    ("nvidia", "NVIDIA"),
    ("opencode.ai", "OpenCode"),
    ("pinterest", "Pinterest"),
    ("playstation", "PlayStation"),
    ("starbucks", "Starbucks"),
    ("tesla", "Tesla"),
    ("x.ai", "X.ai"),
    ("zapier", "Zapier"),
];

/// A validated theme ready to install before opening the application's windows.
pub struct ThemeFile {
    config: Rc<ThemeConfig>,
}

impl ThemeFile {
    pub fn builtin(id: &str) -> Result<Self, String> {
        let content = match id {
            "apple" => include_str!("../themes/apple.toml"),
            "opencode.ai" => include_str!("../themes/opencode.ai.toml"),
            "claude" => include_str!("../themes/claude.toml"),
            "nvidia" => include_str!("../themes/nvidia.toml"),
            "tesla" => include_str!("../themes/tesla.toml"),
            "x.ai" => include_str!("../themes/x.ai.toml"),
            "binance" => include_str!("../themes/binance.toml"),
            "coinbase" => include_str!("../themes/coinbase.toml"),
            "playstation" => include_str!("../themes/playstation.toml"),
            "pinterest" => include_str!("../themes/pinterest.toml"),
            "notion" => include_str!("../themes/notion.toml"),
            "starbucks" => include_str!("../themes/starbucks.toml"),
            "zapier" => include_str!("../themes/zapier.toml"),
            _ => return Err(format!("Unknown built-in theme: {id}")),
        };
        Self::parse(content)
    }

    pub fn parse(content: &str) -> Result<Self, String> {
        let document: toml::Table =
            toml::from_str(content).map_err(|error| format!("Invalid theme TOML: {error}"))?;
        let config: ThemeConfig = document
            .clone()
            .try_into()
            .map_err(|error| format!("Invalid theme: {error}"))?;
        if config.name.trim().is_empty() {
            return Err("Theme name must not be empty".into());
        }
        for (name, value) in [
            ("font.size", config.font_size),
            ("mono_font.size", config.mono_font_size),
        ] {
            if value.is_some_and(|value| !value.is_finite() || value <= 0.) {
                return Err(format!("{name} must be a finite positive number"));
            }
        }
        // GPUI Kit silently falls back for invalid colors and unknown fields.
        // Reject them here so a typo cannot produce a partly applied theme.
        let recognized = toml::Table::try_from(&config).map_err(|error| error.to_string())?;
        for key in document.keys() {
            if !recognized.contains_key(key) {
                return Err(format!("Unknown theme field: {key}"));
            }
        }
        if let Some(toml::Value::Table(colors)) = document.get("colors") {
            let recognized = recognized["colors"]
                .as_table()
                .expect("serialized theme colors");
            for (key, value) in colors {
                if !recognized.contains_key(key) {
                    return Err(format!("Unknown theme color: {key}"));
                }
                let value = value
                    .as_str()
                    .ok_or_else(|| format!("Theme color {key} must be a string"))?;
                try_parse_color(value)
                    .map_err(|error| format!("Invalid theme color {key}: {error}"))?;
            }
        }
        Ok(Self {
            config: Rc::new(config),
        })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|error| format!("Cannot read theme {}: {error}", path.display()))?;
        Self::parse(&content).map_err(|error| format!("{}: {error}", path.display()))
    }

    /// Call after `gpui_kit::init` and before creating component-backed views.
    pub fn apply(&self, cx: &mut App) {
        Theme::update(cx, |theme| theme.apply_config(&self.config));
    }
}
