use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;

// --- THEME MODE ENUM ---
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

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

// --- NEW: PROCESSING BLOCK ---
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProcessingBlock {
    #[serde(default = "generate_id")]
    pub id: String,
    pub block_type: String, // "image", "audio", "text"
    pub model: String,
    pub prompt: String,
    pub selected_language: String, // Context var {language1}
    #[serde(default)]
    pub language_vars: HashMap<String, String>, // Context vars {language1}, etc.
    pub streaming_enabled: bool,
    #[serde(default = "default_render_mode")]
    pub render_mode: String, // "stream", "plain", "markdown"
    
    // UI Behavior
    #[serde(default = "default_true")]
    pub show_overlay: bool,
    #[serde(default)]
    pub auto_copy: bool, // Only one block in chain should have this true
}

fn default_true() -> bool { true }
fn default_render_mode() -> String { "stream".to_string() }
fn generate_id() -> String { format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()) }

impl Default for ProcessingBlock {
    fn default() -> Self {
        Self {
            id: generate_id(),
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            streaming_enabled: true,
            render_mode: "stream".to_string(),
            show_overlay: true,
            auto_copy: false,
        }
    }
}

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
}

fn default_preset_type() -> String { "image".to_string() }
fn default_audio_source() -> String { "mic".to_string() }
fn default_prompt_mode() -> String { "fixed".to_string() }
fn default_text_input_mode() -> String { "select".to_string() }
fn default_theme_mode() -> ThemeMode { ThemeMode::System }
fn default_auto_paste_newline() -> bool { true }
fn default_history_limit() -> usize { 100 }
fn default_graphics_mode() -> String { "standard".to_string() }


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
            video_capture_method: "region".to_string(),
            text_input_mode: "select".to_string(),
            continuous_input: false,
            is_upcoming: false,
            is_master: false,
            show_controller_ui: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub gemini_api_key: String,
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
}

impl Default for Config {
    fn default() -> Self {
        // 1. Standard Translate Preset (Image -> Text)
        let mut p1 = Preset::default();
        p1.id = "preset_translate".to_string();
        p1.name = "Translate".to_string();
        p1.preset_type = "image".to_string();
        p1.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 2. Translate (Auto paste) Preset
        let mut p2 = Preset::default();
        p2.id = "preset_translate_auto_paste".to_string();
        p2.name = "Translate (Auto paste)".to_string();
        p2.preset_type = "image".to_string();
        p2.auto_paste = true;
        p2.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 3. Trans (Select text)
        let mut p3 = Preset::default();
        p3.id = "preset_translate_select".to_string();
        p3.name = "Trans (Select text)".to_string();
        p3.preset_type = "text".to_string();
        p3.text_input_mode = "select".to_string();
        p3.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate the following text to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 3b. Select-Trans-Replace (Bôi-Dịch-Thay)
        let mut p3b = Preset::default();
        p3b.id = "preset_select_translate_replace".to_string();
        p3b.name = "Select-Trans-Replace".to_string();
        p3b.preset_type = "text".to_string();
        p3b.text_input_mode = "select".to_string();
        p3b.auto_paste = true; // Replace original text
        p3b.auto_paste_newline = true; // Add newline after pasting
        p3b.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate the following text to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: false, // Background processing
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 3c. Fix Grammar (Sửa ngữ pháp)
        let mut p3c = Preset::default();
        p3c.id = "preset_fix_grammar".to_string();
        p3c.name = "Fix Grammar".to_string();
        p3c.preset_type = "text".to_string();
        p3c.text_input_mode = "select".to_string();
        p3c.auto_paste = true; // Replace original text
        p3c.auto_paste_newline = true;
        p3c.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Fix grammar, spelling, and punctuation errors in the following text. Preserve the original meaning and tone. Output ONLY the corrected text.".to_string(),
                selected_language: "Vietnamese".to_string(), // Not used but required
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 3d. Rephrase (Viết lại)
        let mut p3d = Preset::default();
        p3d.id = "preset_rephrase".to_string();
        p3d.name = "Rephrase".to_string();
        p3d.preset_type = "text".to_string();
        p3d.text_input_mode = "select".to_string();
        p3d.auto_paste = true;
        p3d.auto_paste_newline = true;
        p3d.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Rephrase the following text using different words while preserving the exact same meaning. Keep the same language. Output ONLY the rephrased text.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 3e. Make Formal (Chuyên nghiệp hóa)
        let mut p3e = Preset::default();
        p3e.id = "preset_make_formal".to_string();
        p3e.name = "Make Formal".to_string();
        p3e.preset_type = "text".to_string();
        p3e.text_input_mode = "select".to_string();
        p3e.auto_paste = true;
        p3e.auto_paste_newline = true;
        p3e.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Rewrite the following text in a more formal, professional tone suitable for business communication. Keep the same language and meaning. Output ONLY the formal version.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 3f. Explain Code (Giải thích code)
        let mut p3f = Preset::default();
        p3f.id = "preset_explain_code".to_string();
        p3f.name = "Explain Code".to_string();
        p3f.preset_type = "text".to_string();
        p3f.text_input_mode = "select".to_string();
        p3f.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Explain what this code does in {language1}. Be concise but thorough. Mention the purpose, key logic, and any important patterns or techniques used. Format the output as a markdown. Only OUTPUT the markdown, no file indicator or triple backticks.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 3g. Extract Table (Trích bảng) - IMAGE preset
        let mut p3g = Preset::default();
        p3g.id = "preset_extract_table".to_string();
        p3g.name = "Extract Table".to_string();
        p3g.preset_type = "image".to_string();
        p3g.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract all data from any tables, forms, or structured content in this image. Format the output as a markdown table. Output ONLY the table, no explanations.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 4. Chain: OCR -> Translate
        let mut p4 = Preset::default();
        p4.id = "preset_translate_retranslate".to_string();
        p4.name = "Translate+Retranslate".to_string();
        p4.preset_type = "image".to_string();
        p4.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract text from this image and translate it to {language1}. Output ONLY the translation text directly.".to_string(),
                selected_language: "Korean".to_string(),
                streaming_enabled: false,
                show_overlay: true,
                auto_copy: true,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 4b. Chain: OCR (Accurate) -> Translate Korean -> Translate Vietnamese
        let mut p4b = Preset::default();
        p4b.id = "preset_extract_retrans_retrans".to_string();
        p4b.name = "Translate (Accurate)+Retranslate".to_string();
        p4b.preset_type = "image".to_string();
        p4b.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
                selected_language: "English".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: false,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Korean".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: true,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 5. Trans+Retrans (Type)
        let mut p5 = Preset::default();
        p5.id = "preset_trans_retrans_typing".to_string();
        p5.name = "Trans+Retrans (Type)".to_string();
        p5.preset_type = "text".to_string();
        p5.text_input_mode = "type".to_string();
        p5.continuous_input = true; // Keep input window open for repeated translations
        p5.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate the following text to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Korean".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: true,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 5b. Internet Search (Tìm kiếm internet)
        let mut p5b = Preset::default();
        p5b.id = "preset_internet_search".to_string();
        p5b.name = "Internet Search".to_string();
        p5b.preset_type = "text".to_string();
        p5b.text_input_mode = "type".to_string();
        p5b.blocks = vec![
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "compound_mini".to_string(),
                prompt: "Search the internet for information about the following query and provide a comprehensive summary. Include key facts, recent developments, and relevant details with clickable links to sources if possible. Format the output as markdown creatively. Only OUTPUT the markdown, no file indicator or triple backticks.".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 6. OCR Preset
        let mut p6 = Preset::default();
        p6.id = "preset_ocr".to_string();
        p6.name = "Extract text".to_string();
        p6.preset_type = "image".to_string();
        p6.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "scout".to_string(),
                prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
                selected_language: "English".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 7. Translate (High accuracy)
        let mut p7 = Preset::default();
        p7.id = "preset_extract_retranslate".to_string();
        p7.name = "Translate (High accuracy)".to_string();
        p7.preset_type = "image".to_string();
        p7.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract all text from this image exactly as it appears. Output ONLY the text.".to_string(),
                selected_language: "English".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: false,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];
        p7.hotkeys.push(Hotkey { code: 192, name: "` / ~".to_string(), modifiers: 0 });

        // 8. Summarize Preset
        let mut p8 = Preset::default();
        p8.id = "preset_summarize".to_string();
        p8.name = "Summarize content".to_string();
        p8.preset_type = "image".to_string();
        p8.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Analyze this image and summarize its content in {language1}. Only return the summary text, super concisely. Format the output as a markdown. Only OUTPUT the markdown, no file indicator or triple backticks.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 9. Image description Preset
        let mut p9 = Preset::default();
        p9.id = "preset_desc".to_string();
        p9.name = "Image description".to_string();
        p9.preset_type = "image".to_string();
        p9.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Describe this image in {language1}. Format the output as a markdown. Only OUTPUT the markdown, no file indicator or triple backticks.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 10. Ask about image
        let mut p10 = Preset::default();
        p10.id = "preset_ask_image".to_string();
        p10.name = "Ask about image".to_string();
        p10.preset_type = "image".to_string();
        p10.prompt_mode = "dynamic".to_string();
        p10.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "gemini-pro".to_string(),
                prompt: "".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 11. Transcribe (Audio)
        let mut p11 = Preset::default();
        p11.id = "preset_transcribe".to_string();
        p11.name = "Transcribe speech".to_string();
        p11.preset_type = "audio".to_string();
        p11.audio_source = "mic".to_string();
        p11.auto_paste = true;
        p11.blocks = vec![
            ProcessingBlock {
                block_type: "audio".to_string(),
                model: "whisper-accurate".to_string(),
                prompt: "".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 12. Study language Preset
        let mut p12 = Preset::default();
        p12.id = "preset_study_language".to_string();
        p12.name = "Study language".to_string();
        p12.preset_type = "audio".to_string();
        p12.audio_source = "device".to_string();
        p12.blocks = vec![
            ProcessingBlock {
                block_type: "audio".to_string(),
                model: "whisper-accurate".to_string(),
                prompt: "".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 13. Quick 4NR reply
        let mut p13 = Preset::default();
        p13.id = "preset_transcribe_retranslate".to_string();
        p13.name = "Quick 4NR reply".to_string();
        p13.preset_type = "audio".to_string();
        p13.audio_source = "mic".to_string();
        p13.auto_paste = true;
        p13.blocks = vec![
            ProcessingBlock {
                block_type: "audio".to_string(),
                model: "whisper-accurate".to_string(),
                prompt: "".to_string(),
                selected_language: "Korean".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: false,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Korean".to_string(),
                streaming_enabled: true,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 14. Quicker foreigner reply Preset
        let mut p14 = Preset::default();
        p14.id = "preset_quicker_foreigner_reply".to_string();
        p14.name = "Quicker foreigner reply".to_string();
        p14.preset_type = "audio".to_string();
        p14.audio_source = "mic".to_string();
        p14.auto_paste = true;
        p14.blocks = vec![
            ProcessingBlock {
                block_type: "audio".to_string(),
                model: "gemini-audio".to_string(),
                prompt: "Translate the audio to {language1}. Only output the translated text.".to_string(),
                selected_language: "Korean".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: true,
                ..Default::default()
            }
        ];

        // 14b. Kiểm chứng thông tin (Fact Check) - IMAGE preset with chain
        let mut p14b = Preset::default();
        p14b.id = "preset_fact_check".to_string();
        p14b.name = "Kiểm chứng thông tin".to_string();
        p14b.preset_type = "image".to_string();
        p14b.blocks = vec![
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract and describe all text, claims, statements, and information visible in this image. Include any context that might be relevant for fact-checking. Output the content clearly.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: false,
                show_overlay: false,
                auto_copy: false,
                ..Default::default()
            },
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "compound_mini".to_string(),
                prompt: "Fact-check the following claims/information. Search the internet to verify accuracy. Provide a clear verdict (TRUE/FALSE/PARTIALLY TRUE/UNVERIFIABLE) for each claim with evidence and sources. Respond in {language1}. Format as markdown. Only OUTPUT the markdown, no file indicator or triple backticks.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];

        // 14c. Thần Trí tuệ (Omniscient God)
        let mut p14c = Preset::default();
        p14c.id = "preset_omniscient_god".to_string();
        p14c.name = "Thần Trí tuệ (Omniscient God)".to_string();
        p14c.preset_type = "image".to_string();
        p14c.blocks = vec![
            // Node 1 (0): 
            ProcessingBlock {
                block_type: "image".to_string(),
                model: "maverick".to_string(),
                prompt: "Extract and describe all text, claims, statements, and information visible in this image.".to_string(),
                selected_language: "English".to_string(),
                streaming_enabled: false,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            },
            // Node 4 (3 -> 1): Make a learning HTML (1->4)
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Create a standalone INTERACTIVE HTML learning card/game for the following text. Use internal CSS for a beautiful, modern, colored design, game-like and comprehensive interface. Output ONLY the raw HTML code, no file indicator or triple backticks.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false, // Don't auto copy
                ..Default::default()
            },
            // Node 3 (2): Summarize into markdown (2->3)
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "compound_mini".to_string(),
                prompt: "Search the internet to ensure of the accuracy of the following text as well as getting as much source information as possible. Summarize the following text into a detailed markdown summary with clickable links to the sources. Structure it clearly. Only OUTPUT the markdown, no file indicator or triple backticks.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                render_mode: "markdown".to_string(),
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            },
            // Node 2 (1 -> 3): Translate (1->2)
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Translate the following text to {language1}. Output ONLY the translation.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            },
             // Node 5 (4): Summarize into several words (2->5)
            ProcessingBlock {
                block_type: "text".to_string(),
                model: "text_accurate_kimi".to_string(),
                prompt: "Summarize the essence of this text into 3-5 keywords or a short phrase in {language1}.".to_string(),
                selected_language: "Vietnamese".to_string(),
                streaming_enabled: true,
                show_overlay: true,
                auto_copy: false,
                ..Default::default()
            }
        ];
        // Swapped indices: 1 <-> 3
        // Old Logic:
        // 0 -> 1 (OCR -> Trans) -> Now: 0 -> 3
        // 1 -> 2 (Trans -> Summ) -> Now: 3 -> 2
        // 0 -> 3 (OCR -> HTML) -> Now: 0 -> 1
        // 1 -> 4 (Trans -> Keys) -> Now: 3 -> 4
        // To fix visual layout:
        // Left (Middle): Translate (3) Top, HTML (1) Bottom.
        // Right: Keywords (4) Top, Search (2) Bottom.
        p14c.block_connections = vec![(0, 3), (0, 1), (3, 4), (3, 2)];

        // 15. Video Summarize Placeholder
        let mut p15 = Preset::default();
        p15.id = "preset_video_summary_placeholder".to_string();
        p15.name = "Summarize video (upcoming)".to_string();
        p15.preset_type = "video".to_string();
        p15.is_upcoming = true;
        p15.blocks = vec![];

        // 16. Realtime Audio Translation (Dịch cabin) Placeholder
        let mut p16 = Preset::default();
        p16.id = "preset_realtime_audio_translate".to_string();
        p16.name = "Realtime Audio Translate (upcoming)".to_string();
        p16.preset_type = "audio".to_string();
        p16.is_upcoming = true;
        p16.blocks = vec![];

        // === MASTER PRESETS ===
        // These presets show a preset wheel for the user to choose which preset to use

        // 17. Image MASTER (Ảnh MASTER)
        let mut pm1 = Preset::default();
        pm1.id = "preset_image_master".to_string();
        pm1.name = "Image MASTER".to_string();
        pm1.preset_type = "image".to_string();
        pm1.is_master = true;
        pm1.show_controller_ui = true;
        pm1.blocks = vec![]; // MASTER presets don't have their own blocks

        // 18. Text-Select MASTER (Bôi MASTER)
        let mut pm2 = Preset::default();
        pm2.id = "preset_text_select_master".to_string();
        pm2.name = "Text-Select MASTER".to_string();
        pm2.preset_type = "text".to_string();
        pm2.text_input_mode = "select".to_string();
        pm2.is_master = true;
        pm2.show_controller_ui = true;
        pm2.blocks = vec![];

        // 19. Text-Type MASTER (Gõ MASTER)
        let mut pm3 = Preset::default();
        pm3.id = "preset_text_type_master".to_string();
        pm3.name = "Text-Type MASTER".to_string();
        pm3.preset_type = "text".to_string();
        pm3.text_input_mode = "type".to_string();
        pm3.is_master = true;
        pm3.show_controller_ui = true;
        pm3.blocks = vec![];

        // 20. Mic MASTER (Mic MASTER)
        let mut pm4 = Preset::default();
        pm4.id = "preset_audio_mic_master".to_string();
        pm4.name = "Mic MASTER".to_string();
        pm4.preset_type = "audio".to_string();
        pm4.audio_source = "mic".to_string();
        pm4.is_master = true;
        pm4.show_controller_ui = true;
        pm4.blocks = vec![];

        // 21. Device Audio MASTER (Tiếng MASTER)
        let mut pm5 = Preset::default();
        pm5.id = "preset_audio_device_master".to_string();
        pm5.name = "Device Audio MASTER".to_string();
        pm5.preset_type = "audio".to_string();
        pm5.audio_source = "device".to_string();
        pm5.is_master = true;
        pm5.show_controller_ui = true;
        pm5.blocks = vec![];

        Self {
            api_key: "".to_string(),
            gemini_api_key: "".to_string(),
            presets: vec![
                pm1, pm2, pm3, pm4, pm5, // MASTER presets first
                p1, p7, p2, p3, p3b, p3c, p3d, p3e, p3f, p3g, p4, p4b, p5, p5b, p6, p8, p9, p10, p11, p12, p13, p14, p14b, p14c, p15, p16
            ],
            active_preset_idx: 0,
            theme_mode: ThemeMode::System,
            ui_language: get_system_ui_language(),
            max_history_items: 100,
            graphics_mode: "standard".to_string(),
            start_in_tray: false,
            run_as_admin_on_startup: false,
        }
    }
}

pub fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_default()
        .join("screen-goated-toolbox");
    let _ = std::fs::create_dir_all(&config_dir);
    config_dir.join("config_v3.json")
}

pub fn load_config() -> Config {
    let path = get_config_path();
    if path.exists() {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        let mut config: Config = serde_json::from_str(&data).unwrap_or_default();
        
        // Safety check: Ensure every preset has at least one block matching its type
        for preset in &mut config.presets {
            // If empty, add default block based on preset type
            if preset.blocks.is_empty() {
                preset.blocks.push(ProcessingBlock {
                    block_type: preset.preset_type.clone(),
                    ..Default::default()
                });
            }
        }
        config
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

pub fn get_all_languages() -> &'static Vec<String> {
    &ALL_LANGUAGES
}
