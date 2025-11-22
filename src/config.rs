use isolang::Language;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hotkey {
    pub code: u32,
    pub name: String,
    pub modifiers: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub gemini_api_key: String,
    pub target_language: String,
    pub hotkeys: Vec<Hotkey>,
    pub dark_mode: bool,
    pub ui_language: String,
    pub auto_copy: bool,
    pub preferred_model: String,
    #[serde(default = "default_streaming_enabled")]
    pub streaming_enabled: bool,
}

fn default_streaming_enabled() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        // Detect system language
        let ui_language = match sys_locale::get_locale() {
            Some(locale) => {
                let lang = locale.to_lowercase();
                // Extract language code (e.g., "vi" from "vi_VN")
                lang.split('_').next().unwrap_or("en").to_string()
            }
            None => "en".to_string(),
        };

        // Detect system dark mode (Windows 10/11)
        let dark_mode = is_system_dark_mode();

        Self {
            api_key: "".to_string(),
            gemini_api_key: "".to_string(),
            target_language: "vi".to_string(),
            hotkeys: vec![Hotkey {
                code: 192, // VK_OEM_3 (~)
                name: "` / ~".to_string(),
                modifiers: 0,
            }],
            dark_mode,
            ui_language,
            auto_copy: false,
            preferred_model: "scout".to_string(),
            streaming_enabled: false,
        }
    }
}

fn is_system_dark_mode() -> bool {
    // Check Windows registry for AppsUseLightTheme (0 = dark, 1 = light)
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize") {
        Ok(key) => {
            key.get_value::<u32, _>("AppsUseLightTheme")
                .map(|val| val == 0)
                .unwrap_or(true) // Default to dark if can't read
        }
        Err(_) => true, // Default to dark
    }
}

pub fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("screen-grounded-translator");
    let _ = std::fs::create_dir_all(&config_dir);
    config_dir.join("config.json")
}

pub fn load_config() -> Config {
    let path = get_config_path();
    if path.exists() {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Config::default()
    }
}

pub fn save_config(config: &Config) {
    let path = get_config_path();
    let data = serde_json::to_string_pretty(config).unwrap();
    let _ = std::fs::write(path, data);
}

/// Get all available languages as a vector of language name strings
pub fn get_all_languages() -> Vec<String> {
    // Use Language::from_usize to iterate through all languages
    // ISO 639-3 has ~7000+ language codes, so we iterate up to a safe upper bound
    let mut languages = Vec::new();
    for i in 0..10000 {
        if let Some(lang) = Language::from_usize(i) {
            languages.push(lang.to_name().to_string());
        }
    }
    // Remove duplicates and sort
    languages.sort();
    languages.dedup();
    languages
}

/// Check if a language code is valid (supports ISO 639-1 and 639-3)
pub fn is_valid_language_code(code: &str) -> bool {
    Language::from_639_1(code).is_some() || Language::from_639_3(code).is_some()
}

/// Get language name from ISO 639-1 or 639-3 code
pub fn get_language_name(code: &str) -> Option<String> {
    Language::from_639_1(code)
        .or_else(|| Language::from_639_3(code))
        .map(|lang| lang.to_name().to_string())
}
