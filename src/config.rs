use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hotkey {
    pub code: u32,
    pub name: String,
    pub modifiers: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub selected_language: String, // Used if {language} is in prompt
    pub model: String,
    pub streaming_enabled: bool,
    pub auto_copy: bool,
    pub hotkeys: Vec<Hotkey>,
    pub retranslate: bool,
    pub retranslate_to: String, // Target language for retranslation
    pub retranslate_model: String,
    pub retranslate_streaming_enabled: bool,
    pub hide_overlay: bool,
    #[serde(default = "default_preset_type")]
    pub preset_type: String,
    #[serde(default)]
    pub is_upcoming: bool,
}

fn default_preset_type() -> String {
    "image".to_string()
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            name: "New Preset".to_string(),
            prompt: "Extract text from this image.".to_string(),
            selected_language: "Vietnamese".to_string(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            hide_overlay: false,
            preset_type: "image".to_string(),
            is_upcoming: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub gemini_api_key: String,
    pub presets: Vec<Preset>,
    pub active_preset_idx: usize, // For UI selection
    pub dark_mode: bool,
    pub ui_language: String,
}

impl Default for Config {
    fn default() -> Self {
        let default_lang = "Vietnamese".to_string(); // Default target
        
        // 1. Translation Preset
        let trans_preset = Preset {
            id: "preset_translate".to_string(),
            name: "Translation".to_string(),
            prompt: "Extract text from this image and translate it to {language}. Output ONLY the translation text directly.".to_string(),
            selected_language: default_lang.clone(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![Hotkey { code: 192, name: "` / ~".to_string(), modifiers: 0 }], // Tilde
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            hide_overlay: false,
            preset_type: "image".to_string(),
            is_upcoming: false,
        };

        // 2. OCR Preset
        let ocr_preset = Preset {
            id: "preset_ocr".to_string(),
            name: "Extract Text (OCR)".to_string(),
            prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
            selected_language: "English".to_string(), // Irrelevant for pure OCR but kept
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            hide_overlay: false,
            preset_type: "image".to_string(),
            is_upcoming: false,
        };

        // 3. Summarize Preset
        let sum_preset = Preset {
            id: "preset_summarize".to_string(),
            name: "Summarize Content".to_string(),
            prompt: "Analyze this image and summarize its content in {language}. Only return the summary text, super concisely.".to_string(),
            selected_language: default_lang.clone(),
            model: "maverick".to_string(), // Use better model
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            hide_overlay: false,
            preset_type: "image".to_string(),
            is_upcoming: false,
        };

        // 4. Description Preset
        let desc_preset = Preset {
            id: "preset_desc".to_string(),
            name: "Image Description".to_string(),
            prompt: "Describe this image in detail in {language}.".to_string(),
            selected_language: default_lang.clone(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            hide_overlay: false,
            preset_type: "image".to_string(),
            is_upcoming: false,
        };

        // 5. Transcribe (Upcoming)
        let upcoming_preset = Preset {
            id: "preset_transcribe".to_string(),
            name: "Transcribe (upcoming)".to_string(),
            prompt: "".to_string(),
            selected_language: default_lang.clone(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: default_lang.clone(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            is_upcoming: true,
        };

        Self {
            api_key: "".to_string(),
            gemini_api_key: "".to_string(),
            presets: vec![trans_preset, ocr_preset, sum_preset, desc_preset, upcoming_preset],
            active_preset_idx: 0,
            dark_mode: true,
            ui_language: "vi".to_string(),
        }
    }
}

pub fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("screen-grounded-translator");
    let _ = std::fs::create_dir_all(&config_dir);
    config_dir.join("config_v2.json") // Changed filename to avoid conflict/migration issues for now
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
    // Use isolang crate to iterate through all languages
    // ISO 639-3 has ~7000+ language codes, so we iterate up to a safe upper bound
    let mut languages = Vec::new();
    for i in 0..10000 {
        if let Some(lang) = isolang::Language::from_usize(i) {
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
    isolang::Language::from_639_1(code).is_some() || isolang::Language::from_639_3(code).is_some()
}

/// Get language name from ISO 639-1 or 639-3 code
pub fn get_language_name(code: &str) -> Option<String> {
    isolang::Language::from_639_1(code)
        .or_else(|| isolang::Language::from_639_3(code))
        .map(|lang| lang.to_name().to_string())
}
