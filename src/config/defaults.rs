//! Config Default implementation using presets from defaults modules.

use super::config_struct::Config;
use super::defaults_image::create_image_presets;
use super::defaults_text::create_text_presets;
use super::defaults_audio::{create_audio_presets, create_master_presets};
use super::types::{
    ThemeMode, get_system_ui_language, DEFAULT_HISTORY_LIMIT,
    default_tts_voice, default_tts_speed, default_tts_language_conditions,
};

impl Default for Config {
    fn default() -> Self {
        // Get all preset groups
        let image_presets = create_image_presets();
        let text_presets = create_text_presets();
        let audio_presets = create_audio_presets();
        let master_presets = create_master_presets();
        
        // Extract specific presets by ID for custom ordering
        // Image presets: p1, p7, p2, p3g, p4, p4b, p6, p6b, p8, p9, p10, p14b, p14c, pm1
        let p1 = image_presets.iter().find(|p| p.id == "preset_translate").cloned().unwrap();
        let p7 = image_presets.iter().find(|p| p.id == "preset_extract_retranslate").cloned().unwrap();
        let p2 = image_presets.iter().find(|p| p.id == "preset_translate_auto_paste").cloned().unwrap();
        let p3g = image_presets.iter().find(|p| p.id == "preset_extract_table").cloned().unwrap();
        let p4 = image_presets.iter().find(|p| p.id == "preset_translate_retranslate").cloned().unwrap();
        let p4b = image_presets.iter().find(|p| p.id == "preset_extract_retrans_retrans").cloned().unwrap();
        let p6 = image_presets.iter().find(|p| p.id == "preset_ocr").cloned().unwrap();
        let p6b = image_presets.iter().find(|p| p.id == "preset_ocr_read").cloned().unwrap();
        let p8 = image_presets.iter().find(|p| p.id == "preset_summarize").cloned().unwrap();
        let p9 = image_presets.iter().find(|p| p.id == "preset_desc").cloned().unwrap();
        let p10 = image_presets.iter().find(|p| p.id == "preset_ask_image").cloned().unwrap();
        let p14b = image_presets.iter().find(|p| p.id == "preset_fact_check").cloned().unwrap();
        let p14c = image_presets.iter().find(|p| p.id == "preset_omniscient_god").cloned().unwrap();
        
        // Text presets: p2b, p3, p3h, p3b, p3c, p3d, p3e, p3f, p3f2, p3f3, pm2, p5, p5a, p5b, p5c, pm3
        let p2b = text_presets.iter().find(|p| p.id == "preset_read_aloud").cloned().unwrap();
        let p3 = text_presets.iter().find(|p| p.id == "preset_translate_select").cloned().unwrap();
        let p3h = text_presets.iter().find(|p| p.id == "preset_trans_retrans_select").cloned().unwrap();
        let p3b = text_presets.iter().find(|p| p.id == "preset_select_translate_replace").cloned().unwrap();
        let p3c = text_presets.iter().find(|p| p.id == "preset_fix_grammar").cloned().unwrap();
        let p3d = text_presets.iter().find(|p| p.id == "preset_rephrase").cloned().unwrap();
        let p3e = text_presets.iter().find(|p| p.id == "preset_make_formal").cloned().unwrap();
        let p3f = text_presets.iter().find(|p| p.id == "preset_explain").cloned().unwrap();
        let p3f2 = text_presets.iter().find(|p| p.id == "preset_ask_text").cloned().unwrap();
        let p3f3 = text_presets.iter().find(|p| p.id == "preset_edit_as_follows").cloned().unwrap();
        let p5 = text_presets.iter().find(|p| p.id == "preset_trans_retrans_typing").cloned().unwrap();
        let p5a = text_presets.iter().find(|p| p.id == "preset_ask_ai").cloned().unwrap();
        let p5b = text_presets.iter().find(|p| p.id == "preset_internet_search").cloned().unwrap();
        let p5c = text_presets.iter().find(|p| p.id == "preset_make_game").cloned().unwrap();
        
        // Audio presets: p11, p11b, p13, p14, p16b, p16c, pm4, p12, pm5, p16
        let p11 = audio_presets.iter().find(|p| p.id == "preset_transcribe").cloned().unwrap();
        let p11b = audio_presets.iter().find(|p| p.id == "preset_fix_pronunciation").cloned().unwrap();
        let p13 = audio_presets.iter().find(|p| p.id == "preset_transcribe_retranslate").cloned().unwrap();
        let p14 = audio_presets.iter().find(|p| p.id == "preset_quicker_foreigner_reply").cloned().unwrap();
        let p16b = audio_presets.iter().find(|p| p.id == "preset_quick_ai_question").cloned().unwrap();
        let p16c = audio_presets.iter().find(|p| p.id == "preset_voice_search").cloned().unwrap();
        let p12 = audio_presets.iter().find(|p| p.id == "preset_study_language").cloned().unwrap();
        let p16 = audio_presets.iter().find(|p| p.id == "preset_realtime_audio_translate").cloned().unwrap();
        
        // Master presets
        let pm1 = master_presets.iter().find(|p| p.id == "preset_image_master").cloned().unwrap();
        let pm2 = master_presets.iter().find(|p| p.id == "preset_text_select_master").cloned().unwrap();
        let pm3 = master_presets.iter().find(|p| p.id == "preset_text_type_master").cloned().unwrap();
        let pm4 = master_presets.iter().find(|p| p.id == "preset_audio_mic_master").cloned().unwrap();
        let pm5 = master_presets.iter().find(|p| p.id == "preset_audio_device_master").cloned().unwrap();

        Self {
            api_key: "".to_string(),
            gemini_api_key: "".to_string(),
            openrouter_api_key: "".to_string(),
            presets: vec![
                // Column 1: Image presets
                p1, p7, p2, p3g, p4, p4b, p6, p6b, p8, p9, p10, p14b, p14c, pm1,
                // Column 2: Text presets (Bôi MASTER after Giải thích code, Gõ MASTER after Internet search)
                p2b, p3, p3h, p3b, p3c, p3d, p3e, p3f, p3f2, p3f3, pm2, p5, p5a, p5b, p5c, pm3,
                // Column 3: Audio presets (Mic presets first, then device audio presets at end)
                p11, p11b, p13, p14, p16b, p16c, pm4, p12, pm5, p16,
            ],
            active_preset_idx: 0,
            theme_mode: ThemeMode::System,
            ui_language: get_system_ui_language(),
            max_history_items: DEFAULT_HISTORY_LIMIT,
            graphics_mode: "standard".to_string(),
            start_in_tray: false,
            run_as_admin_on_startup: false,
            use_groq: true,
            use_gemini: true,
            use_openrouter: false,
            realtime_translation_model: "groq-llama".to_string(),
            realtime_font_size: 16,
            realtime_transcription_size: (500, 180),
            realtime_translation_size: (500, 180),
            realtime_audio_source: "device".to_string(),
            realtime_target_language: "Vietnamese".to_string(),
            // Ollama defaults
            use_ollama: false,
            ollama_base_url: "http://localhost:11434".to_string(),
            ollama_vision_model: String::new(),
            ollama_text_model: String::new(),
            tts_voice: default_tts_voice(),
            tts_speed: default_tts_speed(),
            tts_output_device: String::new(),
            tts_language_conditions: default_tts_language_conditions(),
            // Favorite bubble defaults
            show_favorite_bubble: false,
            favorite_bubble_position: None,
        }
    }
}
