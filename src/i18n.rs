use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static EXTERNAL: OnceLock<HashMap<String, String>> = OnceLock::new();

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    SimplifiedChinese,
    External(PathBuf),
}

#[derive(Deserialize, Serialize)]
struct SavedSettings {
    language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_file: Option<PathBuf>,
}

pub fn settings_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("RS7Z_SETTINGS") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::home_dir().ok_or("Cannot determine the home directory")?;
    let directory = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    };
    Ok(directory.join("rs7z/settings.toml"))
}

pub fn read_settings(path: &Path) -> Result<Language, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Language::English),
        Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
    };
    let settings: SavedSettings = toml::from_str(&content)
        .map_err(|error| format!("Invalid settings in {}: {error}", path.display()))?;
    match settings.language.as_str() {
        "en" => Ok(Language::English),
        "zh-CN" => Ok(Language::SimplifiedChinese),
        "external" => settings
            .external_file
            .filter(|path| !path.as_os_str().is_empty())
            .map(Language::External)
            .ok_or_else(|| "External language file path is missing".into()),
        _ => Err(format!("Unsupported language: {}", settings.language)),
    }
}

pub fn save_settings(path: &Path, language: &Language) -> Result<(), String> {
    let (name, external_file) = match language {
        Language::English => ("en", None),
        Language::SimplifiedChinese => ("zh-CN", None),
        Language::External(path) => ("external", Some(path.clone())),
    };
    let settings = SavedSettings {
        language: name.into(),
        external_file,
    };
    let text = toml::to_string(&settings).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(text.as_bytes())
        .and_then(|()| temporary.flush())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

fn placeholders(value: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut remaining = value;
    while let Some((_, tail)) = remaining.split_once("%{") {
        let Some((name, rest)) = tail.split_once('}') else {
            break;
        };
        names.push(name);
        remaining = rest;
    }
    names.sort_unstable();
    names
}

pub fn parse_external(content: &str) -> Result<HashMap<String, String>, String> {
    let document: toml::Table = toml::from_str(content).map_err(|error| error.to_string())?;
    Ok(document
        .into_iter()
        .filter_map(|(key, value)| {
            let text = value.as_str()?;
            (!text.trim().is_empty() && placeholders(&key) == placeholders(text))
                .then(|| (key, text.to_owned()))
        })
        .collect())
}

pub fn validate_external(path: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path)
        .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    if !path.is_file() {
        return Err(format!("Not a language file: {}", path.display()));
    }
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    parse_external(&content)?;
    Ok(path)
}

pub fn initialize() {
    let language = settings_path().and_then(|path| read_settings(&path));
    match language {
        Ok(Language::SimplifiedChinese) => rust_i18n::set_locale("zh-CN"),
        Ok(Language::External(path)) => {
            match fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|text| parse_external(&text))
            {
                Ok(translations) => {
                    let _ = EXTERNAL.set(translations);
                }
                Err(error) => eprintln!(
                    "rs7z: Cannot load language file {}: {error}",
                    path.display()
                ),
            }
            rust_i18n::set_locale("en");
        }
        Err(error) => {
            eprintln!("rs7z: {error}");
            rust_i18n::set_locale("en");
        }
        Ok(Language::English) => rust_i18n::set_locale("en"),
    }
}

fn translate(key: &str, locale: &str, external: Option<&HashMap<String, String>>) -> String {
    if let Some(value) = external.and_then(|translations| translations.get(key)) {
        return value.clone();
    }
    let translated = rust_i18n::t!(key, locale = locale);
    if translated.trim().is_empty() {
        key.to_owned()
    } else {
        translated.into_owned()
    }
}

pub fn tr(key: &str) -> String {
    translate(key, &rust_i18n::locale(), EXTERNAL.get())
}

pub fn trf(key: &str, values: &[(&str, String)]) -> String {
    let translated = tr(key);
    let names: Vec<_> = values.iter().map(|(name, _)| *name).collect();
    let values: Vec<_> = values.iter().map(|(_, value)| value.clone()).collect();
    rust_i18n::replace_patterns(&translated, &names, &values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_missing_defaults_to_english() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        assert_eq!(read_settings(&path).unwrap(), Language::English);
        let language = Language::External(directory.path().join("language.toml"));
        save_settings(&path, &language).unwrap();
        assert_eq!(read_settings(&path).unwrap(), language);
    }

    #[test]
    fn external_file_uses_valid_entries_and_falls_back_per_text() {
        let values = parse_external(
            "'Add' = 'Ajouter'\n'Copy' = ''\n'Delete' = 2\n'Hello %{name}' = 'Bonjour %{other}'",
        )
        .unwrap();
        assert_eq!(values.get("Add"), Some(&"Ajouter".into()));
        assert!(!values.contains_key("Copy"));
        assert!(!values.contains_key("Delete"));
        assert!(!values.contains_key("Hello %{name}"));
        assert_eq!(translate("Add", "en", Some(&values)), "Ajouter");
        assert_eq!(translate("Delete", "en", Some(&values)), "Delete");
        assert_eq!(translate("Copy", "en", Some(&values)), "Copy");
    }

    #[test]
    fn built_in_chinese_and_missing_keys_fall_back_to_english() {
        assert_eq!(rust_i18n::t!("Settings", locale = "zh-CN"), "设置");
        assert_eq!(translate("Settings", "zh-CN", None), "设置");
        assert_eq!(translate("Settings", "en", None), "Settings");
        assert_eq!(
            translate("A missing English phrase", "zh-CN", None),
            "A missing English phrase"
        );
    }

    #[test]
    fn built_in_catalogs_have_matching_keys_and_placeholders() {
        let english: HashMap<String, String> =
            toml::from_str(include_str!("../locales/en.toml")).unwrap();
        let chinese = parse_external(include_str!("../locales/zh-CN.toml")).unwrap();
        assert_eq!(english.len(), chinese.len());
        for (key, value) in &english {
            assert!(value.is_empty(), "English values should be empty: {key}");
            assert_eq!(translate(key, "en", None), *key);
            assert!(
                chinese.contains_key(key),
                "missing Chinese translation: {key}"
            );
            assert_eq!(translate(key, "zh-CN", None), chinese[key]);
            assert_eq!(translate(key, "unsupported-locale", None), *key);
        }
    }
}
