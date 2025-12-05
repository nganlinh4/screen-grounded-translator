use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;

fn get_system_ui_language() -> String {
    let sys_locale = sys_locale::get_locale().unwrap_or_default();
    let lang_code = sys_locale.split('-').next().unwrap_or("en").to_lowercase();
    
    match lang_code.as_str() {
        "vi" => "vi".to_string(),
        "ko" => "ko".to_string(),
        "en" => "en".to_string(),
        _ => "en".to_string(), // Default to English for unsupported languages
    }
}

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
    #[serde(default = "default_prompt_mode")]
    pub prompt_mode: String, // "fixed" or "dynamic"
    pub selected_language: String, 
    #[serde(default)]
    pub language_vars: HashMap<String, String>,
    pub model: String,
    pub streaming_enabled: bool,
    pub auto_copy: bool,
    pub hotkeys: Vec<Hotkey>,
    pub retranslate: bool,
    pub retranslate_to: String,
    pub retranslate_model: String,
    pub retranslate_streaming_enabled: bool,
    #[serde(default)]
    pub retranslate_auto_copy: bool,
    pub hide_overlay: bool,
    #[serde(default = "default_preset_type")]
    pub preset_type: String, // "image", "audio", "video"
    
    // --- Audio Fields ---
    #[serde(default = "default_audio_source")]
    pub audio_source: String, // "mic" or "device"
    #[serde(default)]
    pub hide_recording_ui: bool,

    // --- Video Fields ---
    #[serde(default)]
    pub video_capture_method: String, // "region" or "monitor:DeviceName"

    #[serde(default)]
    pub is_upcoming: bool,
}

fn default_preset_type() -> String { "image".to_string() }
fn default_audio_source() -> String { "mic".to_string() }
fn default_prompt_mode() -> String { "fixed".to_string() }

impl Default for Preset {
    fn default() -> Self {
        Self {
            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            name: "New Preset".to_string(),
            prompt: "Extract text from this image.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
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
    #[serde(default = "default_history_limit")]
    pub max_history_items: usize, // NEW
    
    // --- NEW FIELDS ---
    #[serde(default)]
    pub start_in_tray: bool,
    #[serde(default)]
    pub run_as_admin_on_startup: bool, 
    // ------------------
}

fn default_history_limit() -> usize { 100 }

    impl Default for Config {
    fn default() -> Self {
        let system_ui_lang = get_system_ui_language();
        let _default_lang = match system_ui_lang.as_str() {
            "vi" => "Vietnamese".to_string(),
            "ko" => "Korean".to_string(),
            _ => "English".to_string(),
        }; 
        
        // 1. Translation Preset
        let mut trans_lang_vars = HashMap::new();
        trans_lang_vars.insert("language1".to_string(), "Vietnamese".to_string());
        
        let trans_preset = Preset {
            id: "preset_translate".to_string(),
            name: "Translate".to_string(),
            prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: trans_lang_vars.clone(),
            model: "maverick".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![Hotkey { code: 192, name: "` / ~".to_string(), modifiers: 0 }], // Tilde
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 1.5. Translate+Retranslate Preset
        let mut trans_retrans_lang_vars = HashMap::new();
        trans_retrans_lang_vars.insert("language1".to_string(), "Korean".to_string());

        let trans_retrans_preset = Preset {
            id: "preset_translate_retranslate".to_string(),
            name: "Translate+Retranslate".to_string(),
            prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Korean".to_string(),
            language_vars: trans_retrans_lang_vars,
            model: "gemini-flash-lite".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "text_accurate_kimi".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 2. OCR Preset
        let ocr_preset = Preset {
            id: "preset_ocr".to_string(),
            name: "Extract text (OCR)".to_string(),
            prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "English".to_string(),
            language_vars: HashMap::new(), // No language tags
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: true, 
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 2.5. Extract text+Retranslate Preset
        let extract_retrans_preset = Preset {
            id: "preset_extract_retranslate".to_string(),
            name: "Extract text+Retranslate".to_string(),
            prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "English".to_string(),
            language_vars: HashMap::new(),
            model: "maverick".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "text_accurate_kimi".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 3. Summarize Preset
        let mut sum_lang_vars = HashMap::new();
        sum_lang_vars.insert("language1".to_string(), "Vietnamese".to_string());
        
        let sum_preset = Preset {
            id: "preset_summarize".to_string(),
            name: "Summarize content".to_string(),
            prompt: "Analyze this image and summarize its content in {language1}. Only return the summary text, super concisely.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: sum_lang_vars,
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 4. Description Preset
        let mut desc_lang_vars = HashMap::new();
        desc_lang_vars.insert("language1".to_string(), "Vietnamese".to_string());
        
        let desc_preset = Preset {
            id: "preset_desc".to_string(),
            name: "Image description".to_string(),
            prompt: "Describe this image in {language1}.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: desc_lang_vars,
            model: "scout".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 4.5. Ask about image (Dynamic Prompt Mode)
        let mut ask_lang_vars = HashMap::new();
        ask_lang_vars.insert("language1".to_string(), "Vietnamese".to_string());

        let ask_preset = Preset {
            id: "preset_ask_image".to_string(),
            name: "Ask about image".to_string(),
            prompt: "".to_string(),
            prompt_mode: "dynamic".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: ask_lang_vars,
            model: "gemini-flash-lite".to_string(),
            streaming_enabled: true,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 5. Transcribe (Audio)
        let audio_preset = Preset {
            id: "preset_transcribe".to_string(),
            name: "Transcribe speech".to_string(),
            prompt: "".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            model: "whisper-accurate".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 6. Study language Preset
        let study_lang_preset = Preset {
            id: "preset_study_language".to_string(),
            name: "Study language".to_string(),
            prompt: "".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            model: "whisper-accurate".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "text_accurate_kimi".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            audio_source: "device".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 7. Quick foreigner reply
        let transcribe_retrans_preset = Preset {
            id: "preset_transcribe_retranslate".to_string(),
            name: "Quick foreigner reply".to_string(),
            prompt: "".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Korean".to_string(),
            language_vars: HashMap::new(),
            model: "whisper-accurate".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: true,
            retranslate_to: "Korean".to_string(),
            retranslate_model: "text_accurate_kimi".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: true,
            hide_overlay: false,
            preset_type: "audio".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 8. Quicker foreigner reply Preset (new 4th audio preset with gemini-audio)
        let mut quicker_reply_lang_vars = HashMap::new();
        quicker_reply_lang_vars.insert("language1".to_string(), "Korean".to_string());

        let quicker_reply_preset = Preset {
            id: "preset_quicker_foreigner_reply".to_string(),
            name: "Quicker foreigner reply".to_string(),
            prompt: "Translate the audio to {language1}. Only output the translated text.".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Korean".to_string(),
            language_vars: quicker_reply_lang_vars,
            model: "gemini-audio".to_string(),
            streaming_enabled: false,
            auto_copy: true,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "fast_text".to_string(),
            retranslate_streaming_enabled: true,
            retranslate_auto_copy: false,
            hide_overlay: true,
            preset_type: "audio".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: false,
        };

        // 9. Video Summarize Placeholder (NEW)
        let video_placeholder_preset = Preset {
            id: "preset_video_summary_placeholder".to_string(),
            name: "Summarize video (upcoming)".to_string(),
            prompt: "".to_string(),
            prompt_mode: "fixed".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            model: "".to_string(),
            streaming_enabled: false,
            auto_copy: false,
            hotkeys: vec![],
            retranslate: false,
            retranslate_to: "Vietnamese".to_string(),
            retranslate_model: "".to_string(),
            retranslate_streaming_enabled: false,
            retranslate_auto_copy: false,
            hide_overlay: false,
            preset_type: "video".to_string(),
            audio_source: "".to_string(),
            hide_recording_ui: false,
            video_capture_method: "region".to_string(),
            is_upcoming: true, // Mark as upcoming to gray out in sidebar
        };

        Self {
            api_key: "".to_string(),
            gemini_api_key: "".to_string(),
            presets: vec![
                trans_preset, trans_retrans_preset, ocr_preset, extract_retrans_preset, 
                sum_preset, desc_preset, ask_preset, audio_preset, study_lang_preset, 
                transcribe_retrans_preset, quicker_reply_preset, video_placeholder_preset
            ],
            active_preset_idx: 0,
            dark_mode: true,
            ui_language: get_system_ui_language(),
            max_history_items: 100,
            
            // --- NEW DEFAULTS ---
            start_in_tray: false,
            run_as_admin_on_startup: false,
            // --------------------
        }
    }
}

pub fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("screen-goated-toolbox");
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

lazy_static::lazy_static! {
    static ref ALL_LANGUAGES: Vec<String> = {
        let mut languages = Vec::new();
        for i in 0..10000 {
            if let Some(lang) = isolang::Language::from_usize(i) {
                languages.push(lang.to_name().to_string());
            }
        }
        languages.sort();
        languages.dedup();
        languages
    };
}

/// Get all available languages as a vector of language name strings
pub fn get_all_languages() -> &'static Vec<String> {
    &ALL_LANGUAGES
}

