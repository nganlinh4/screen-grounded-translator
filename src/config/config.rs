//! Main Config struct definition.

use serde::{Deserialize, Serialize};

use crate::config::preset::{get_default_presets, Preset};
use crate::config::types::{
    default_tts_language_conditions, get_system_ui_language, EdgeTtsSettings, ThemeMode,
    TtsLanguageCondition, TtsMethod, DEFAULT_HISTORY_LIMIT,
};

// ============================================================================
// SERDE DEFAULT FUNCTIONS
// ============================================================================

fn default_true() -> bool {
    true
}

fn default_history_limit() -> usize {
    DEFAULT_HISTORY_LIMIT
}

fn default_graphics_mode() -> String {
    "standard".to_string()
}

fn default_tts_voice() -> String {
    "Aoede".to_string()
}

fn default_tts_speed() -> String {
    "Fast".to_string()
}

fn default_tts_method() -> TtsMethod {
    TtsMethod::GeminiLive
}

fn default_edge_tts_settings() -> EdgeTtsSettings {
    EdgeTtsSettings::default()
}

fn default_realtime_translation_model() -> String {
    "groq-llama".to_string()
}

fn default_realtime_font_size() -> u32 {
    16
}

fn default_realtime_window_size() -> (i32, i32) {
    (500, 180)
}

fn default_realtime_target_language() -> String {
    "Vietnamese".to_string()
}

fn default_ollama_base_url() -> String {
    "http://localhost:11434".to_string()
}

// ============================================================================
// CONFIG STRUCT
// ============================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    // -------------------------------------------------------------------------
    // API Keys
    // -------------------------------------------------------------------------
    /// Groq API key
    pub api_key: String,

    /// Google Gemini API key
    pub gemini_api_key: String,

    /// OpenRouter API key
    #[serde(default)]
    pub openrouter_api_key: String,

    /// Cerebras AI API key
    #[serde(default)]
    pub cerebras_api_key: String,

    // -------------------------------------------------------------------------
    // Presets
    // -------------------------------------------------------------------------
    /// All configured presets
    pub presets: Vec<Preset>,

    /// Index of the currently active preset
    pub active_preset_idx: usize,

    // -------------------------------------------------------------------------
    // UI Settings
    // -------------------------------------------------------------------------
    /// Theme mode: System, Dark, or Light
    #[serde(default)]
    pub theme_mode: ThemeMode,

    /// UI language code: "en", "vi", "ko"
    pub ui_language: String,

    /// Maximum history items to keep
    #[serde(default = "default_history_limit")]
    pub max_history_items: usize,

    /// Graphics mode: "standard" or "low"
    #[serde(default = "default_graphics_mode")]
    pub graphics_mode: String,

    // -------------------------------------------------------------------------
    // Startup Behavior
    // -------------------------------------------------------------------------
    /// Start minimized to system tray
    #[serde(default)]
    pub start_in_tray: bool,

    /// Request admin privileges on startup
    #[serde(default)]
    pub run_as_admin_on_startup: bool,

    // -------------------------------------------------------------------------
    // API Provider Toggles
    // -------------------------------------------------------------------------
    /// Enable Groq models
    #[serde(default = "default_true")]
    pub use_groq: bool,

    /// Enable Google Gemini models
    #[serde(default = "default_true")]
    pub use_gemini: bool,

    /// Enable OpenRouter models
    #[serde(default)]
    pub use_openrouter: bool,

    /// Enable Cerebras AI models
    #[serde(default)]
    pub use_cerebras: bool,

    /// Enable local Ollama models
    #[serde(default)]
    pub use_ollama: bool,

    // -------------------------------------------------------------------------
    // Ollama Configuration
    // -------------------------------------------------------------------------
    /// Ollama server base URL
    #[serde(default = "default_ollama_base_url")]
    pub ollama_base_url: String,

    /// Ollama model for vision tasks
    #[serde(default)]
    pub ollama_vision_model: String,

    /// Ollama model for text tasks
    #[serde(default)]
    pub ollama_text_model: String,

    // -------------------------------------------------------------------------
    // Realtime Audio Settings
    // -------------------------------------------------------------------------
    /// Model for realtime translation: "groq-llama" or "google-gemma"
    #[serde(default = "default_realtime_translation_model")]
    pub realtime_translation_model: String,

    /// Font size for realtime overlay
    #[serde(default = "default_realtime_font_size")]
    pub realtime_font_size: u32,

    /// Realtime transcription window size
    #[serde(default = "default_realtime_window_size")]
    pub realtime_transcription_size: (i32, i32),

    /// Realtime translation window size
    #[serde(default = "default_realtime_window_size")]
    pub realtime_translation_size: (i32, i32),

    /// Realtime audio source: "mic" or "device"
    #[serde(default)]
    pub realtime_audio_source: String,

    /// Target language for realtime translation
    #[serde(default = "default_realtime_target_language")]
    pub realtime_target_language: String,

    // -------------------------------------------------------------------------
    // TTS Settings
    // -------------------------------------------------------------------------
    /// TTS method: GeminiLive, GoogleTranslate, or EdgeTTS
    #[serde(default = "default_tts_method")]
    pub tts_method: TtsMethod,

    /// TTS voice name
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,

    /// TTS speed: "Normal", "Slow", "Fast"
    #[serde(default = "default_tts_speed")]
    pub tts_speed: String,

    /// TTS output device ID
    #[serde(default)]
    pub tts_output_device: String,

    /// Language-specific TTS instructions
    #[serde(default = "default_tts_language_conditions")]
    pub tts_language_conditions: Vec<TtsLanguageCondition>,

    /// Edge TTS specific settings
    #[serde(default = "default_edge_tts_settings")]
    pub edge_tts_settings: EdgeTtsSettings,

    // -------------------------------------------------------------------------
    // Favorite Bubble Settings
    // -------------------------------------------------------------------------
    /// Show floating favorite bubble
    #[serde(default)]
    pub show_favorite_bubble: bool,

    /// Bubble position (physical pixels)
    #[serde(default)]
    pub favorite_bubble_position: Option<(i32, i32)>,

    /// Keep the favorites panel open after selecting a preset
    #[serde(default)]
    pub favorites_keep_open: bool,

    // -------------------------------------------------------------------------
    // Maintenance Flags
    // -------------------------------------------------------------------------
    /// Clear WebView data on next startup (for MIDI permission reset)
    #[serde(default)]
    pub clear_webview_on_startup: bool,
}

// ============================================================================
// CONFIG DEFAULT IMPL
// ============================================================================

impl Default for Config {
    fn default() -> Self {
        Self {
            // API Keys
            api_key: String::new(),
            gemini_api_key: String::new(),
            openrouter_api_key: String::new(),
            cerebras_api_key: String::new(),

            // Presets - use the centralized ordered list
            presets: get_default_presets(),
            active_preset_idx: 0,

            // UI Settings
            theme_mode: ThemeMode::System,
            ui_language: get_system_ui_language(),
            max_history_items: DEFAULT_HISTORY_LIMIT,
            graphics_mode: "standard".to_string(),

            // Startup
            start_in_tray: false,
            run_as_admin_on_startup: false,

            // API Providers
            use_groq: true,
            use_gemini: true,
            use_openrouter: false,
            use_cerebras: false,
            use_ollama: false,

            // Ollama
            ollama_base_url: "http://localhost:11434".to_string(),
            ollama_vision_model: String::new(),
            ollama_text_model: String::new(),

            // Realtime Audio
            realtime_translation_model: "groq-llama".to_string(),
            realtime_font_size: 16,
            realtime_transcription_size: (500, 180),
            realtime_translation_size: (500, 180),
            realtime_audio_source: "device".to_string(),
            realtime_target_language: "Vietnamese".to_string(),

            // TTS
            tts_method: TtsMethod::GeminiLive,
            tts_voice: "Aoede".to_string(),
            tts_speed: "Fast".to_string(),
            tts_output_device: String::new(),
            tts_language_conditions: default_tts_language_conditions(),
            edge_tts_settings: EdgeTtsSettings::default(),

            // Favorite Bubble
            show_favorite_bubble: false,
            favorite_bubble_position: None,
            favorites_keep_open: false,

            // Maintenance
            clear_webview_on_startup: false,
        }
    }
}
