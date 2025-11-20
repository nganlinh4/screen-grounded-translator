use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum UiLanguage {
    English,
    Vietnamese,
    Korean,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub target_language: String, 
    pub hotkey_code: u32,
    pub hotkey_name: String,
    pub dark_mode: bool,
    pub ui_language: UiLanguage,
    pub auto_copy: bool,
}

impl Default for Config {
    fn default() -> Self {
        // Detect system language
        let ui_language = match sys_locale::get_locale() {
            Some(locale) => {
                let lang = locale.to_lowercase();
                if lang.starts_with("vi") {
                    UiLanguage::Vietnamese
                } else if lang.starts_with("ko") {
                    UiLanguage::Korean
                } else {
                    UiLanguage::English
                }
            }
            None => UiLanguage::English,
        };

        // Detect system dark mode (Windows 10/11)
        let dark_mode = is_system_dark_mode();

        Self {
            api_key: "".to_string(),
            target_language: "Vietnamese".to_string(),
            hotkey_code: 192, // VK_OEM_3 (~)
            hotkey_name: "` / ~".to_string(),
            dark_mode,
            ui_language,
            auto_copy: false,
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
    let config_dir = dirs::config_dir().unwrap_or_default().join("screen-grounded-translator");
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

pub const ISO_LANGUAGES: &[&str] = &[
    "Afrikaans", "Albanian", "Amharic", "Arabic", "Armenian", "Azerbaijani", "Basque", "Belarusian", "Bengali", 
    "Bosnian", "Bulgarian", "Catalan", "Cebuano", "Chichewa", "Chinese (Simplified)", "Chinese (Traditional)", 
    "Corsican", "Croatian", "Czech", "Danish", "Dutch", "English", "Esperanto", "Estonian", "Filipino", "Finnish", 
    "French", "Frisian", "Galician", "Georgian", "German", "Greek", "Gujarati", "Haitian Creole", "Hausa", 
    "Hawaiian", "Hebrew", "Hindi", "Hmong", "Hungarian", "Icelandic", "Igbo", "Indonesian", "Irish", "Italian", 
    "Japanese", "Javanese", "Kannada", "Kazakh", "Khmer", "Korean", "Kurdish (Kurmanji)", "Kyrgyz", "Lao", 
    "Latin", "Latvian", "Lithuanian", "Luxembourgish", "Macedonian", "Malagasy", "Malay", "Malayalam", "Maltese", 
    "Maori", "Marathi", "Mongolian", "Myanmar (Burmese)", "Nepali", "Norwegian", "Pashto", "Persian", "Polish", 
    "Portuguese", "Punjabi", "Romanian", "Russian", "Samoan", "Scots Gaelic", "Serbian", "Sesotho", "Shona", 
    "Sindhi", "Sinhala", "Slovak", "Slovenian", "Somali", "Spanish", "Sundanese", "Swahili", "Swedish", "Tajik", 
    "Tamil", "Telugu", "Thai", "Turkish", "Ukrainian", "Urdu", "Uzbek", "Vietnamese", "Welsh", "Xhosa", "Yiddish", 
    "Yoruba", "Zulu"
];