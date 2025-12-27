//! Config struct definition.

use serde::{Deserialize, Serialize};

use super::preset::Preset;
use super::types::{
    default_graphics_mode, default_history_limit, default_ollama_base_url,
    default_realtime_font_size, default_realtime_target_language,
    default_realtime_translation_model, default_realtime_window_size, default_theme_mode,
    default_true, default_tts_language_conditions, default_tts_method, default_tts_speed,
    default_tts_voice, ThemeMode, TtsLanguageCondition, TtsMethod,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub gemini_api_key: String,
    #[serde(default)]
    pub openrouter_api_key: String,
    pub presets: Vec<Preset>,
    pub active_preset_idx: usize,
    #[serde(default = "default_theme_mode")]
    pub theme_mode: ThemeMode,
    pub ui_language: String,
    #[serde(default = "default_history_limit")]
    pub max_history_items: usize,
    #[serde(default = "default_graphics_mode")]
    pub graphics_mode: String,
    #[serde(default)]
    pub start_in_tray: bool,
    #[serde(default)]
    pub run_as_admin_on_startup: bool,
    #[serde(default = "default_true")]
    pub use_groq: bool,
    #[serde(default = "default_true")]
    pub use_gemini: bool,
    #[serde(default)]
    pub use_openrouter: bool,
    /// Model for realtime translation: "groq-llama" or "google-gemma"
    #[serde(default = "default_realtime_translation_model")]
    pub realtime_translation_model: String,

    // --- Realtime Overlay Persistence ---
    #[serde(default = "default_realtime_font_size")]
    pub realtime_font_size: u32,
    #[serde(default = "default_realtime_window_size")]
    pub realtime_transcription_size: (i32, i32),
    #[serde(default = "default_realtime_window_size")]
    pub realtime_translation_size: (i32, i32),
    #[serde(default)]
    pub realtime_audio_source: String, // "mic" or "device"
    #[serde(default = "default_realtime_target_language")]
    pub realtime_target_language: String,

    // --- Ollama Configuration ---
    #[serde(default)]
    pub use_ollama: bool,
    #[serde(default = "default_ollama_base_url")]
    pub ollama_base_url: String,
    #[serde(default)]
    pub ollama_vision_model: String,
    #[serde(default)]
    pub ollama_text_model: String,

    // --- TTS Settings ---
    #[serde(default = "default_tts_method")]
    pub tts_method: TtsMethod,
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    #[serde(default = "default_tts_speed")]
    pub tts_speed: String, // "Normal", "Slow", "Fast"
    #[serde(default)]
    pub tts_output_device: String, // Device ID
    #[serde(default = "default_tts_language_conditions")]
    pub tts_language_conditions: Vec<TtsLanguageCondition>, // Language-specific TTS conditions

    // --- Favorite Bubble Settings ---
    #[serde(default)]
    pub show_favorite_bubble: bool,
    #[serde(default)]
    pub favorite_bubble_position: Option<(i32, i32)>, // Screen position (physical pixels)
}
