//! Preset struct definition.

use serde::{Deserialize, Serialize};

use super::types::{
    Hotkey, ProcessingBlock,
    default_preset_type, default_audio_source, default_prompt_mode,
    default_text_input_mode, default_auto_paste_newline, default_audio_processing_mode,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Preset {
    pub id: String,
    pub name: String,
    
    // Chain of processing steps
    #[serde(default)]
    pub blocks: Vec<ProcessingBlock>,
    
    // Graph connections: (from_block_idx, to_block_idx)
    // Allows branching: one block can connect to multiple downstream blocks
    #[serde(default)]
    pub block_connections: Vec<(usize, usize)>,

    // Legacy/Global Preset Settings
    #[serde(default = "default_prompt_mode")]
    pub prompt_mode: String, // "fixed" or "dynamic" (Only applies to first block if Image)
    
    #[serde(default)]
    pub auto_paste: bool,
    #[serde(default = "default_auto_paste_newline")]
    pub auto_paste_newline: bool,
    
    pub hotkeys: Vec<Hotkey>,
    
    #[serde(default = "default_preset_type")]
    pub preset_type: String, // "image", "audio", "video", "text" (Defines type of Block 0)
    
    // --- Audio Fields ---
    #[serde(default = "default_audio_source")]
    pub audio_source: String,
    #[serde(default)]
    pub hide_recording_ui: bool,
    #[serde(default)]
    pub auto_stop_recording: bool, // Silence-based auto-stop
    #[serde(default = "default_audio_processing_mode")]
    pub audio_processing_mode: String, // "record_then_process" or "realtime"

    // --- Video Fields ---
    #[serde(default)]
    pub video_capture_method: String,

    // --- Text Fields ---
    #[serde(default = "default_text_input_mode")]
    pub text_input_mode: String,
    
    // Continuous input mode: if true, input window stays open after submit
    // and result overlays spawn below the input window
    #[serde(default)]
    pub continuous_input: bool,

    #[serde(default)]
    pub is_upcoming: bool,

    // --- MASTER Preset Fields ---
    // If true, this preset is a MASTER preset that shows the preset wheel for selection
    #[serde(default)]
    pub is_master: bool,
    
    // Controller UI mode: when enabled, hides advanced UI elements (nodegraph, paste controls, etc.)
    // Default: true for MASTER presets, false for regular presets
    #[serde(default)]
    pub show_controller_ui: bool,
    
    // Whether this preset is marked as a favorite for quick access via the floating bubble
    #[serde(default)]
    pub is_favorite: bool,
}

impl Default for Preset {
    fn default() -> Self {
        // Create a default image chain
        Self {
            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            name: "New Preset".to_string(),
            blocks: vec![
                ProcessingBlock {
                    block_type: "image".to_string(),
                    model: "maverick".to_string(),
                    prompt: "Extract text.".to_string(),
                    show_overlay: true,
                    ..Default::default()
                }
            ],
            block_connections: vec![], // Will be populated from snarl graph
            prompt_mode: "fixed".to_string(),
            auto_paste: false,
            auto_paste_newline: false,
            hotkeys: vec![],
            preset_type: "image".to_string(),
            audio_source: "mic".to_string(),
            hide_recording_ui: false,
            auto_stop_recording: false,
            audio_processing_mode: "record_then_process".to_string(),
            video_capture_method: "region".to_string(),
            text_input_mode: "select".to_string(),
            continuous_input: false,
            is_upcoming: false,
            is_master: false,
            show_controller_ui: false,
            is_favorite: false,
        }
    }
}

