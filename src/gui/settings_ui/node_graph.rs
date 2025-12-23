// Node Graph UI for Processing Chain
// Uses egui-snarl for visual node editing

use eframe::egui;
use egui_snarl::{Snarl, InPin, InPinId, OutPin, OutPinId, NodeId};
use egui_snarl::ui::{SnarlStyle, PinInfo, SnarlViewer};
use crate::config::{ProcessingBlock, get_all_languages};
use crate::model_config::{get_all_models_with_ollama, ModelType, get_model_by_id, trigger_ollama_model_scan, is_ollama_scan_in_progress};
use crate::gui::icons::{Icon, icon_button};
use std::collections::HashMap;

/// Check if a model supports search capabilities (grounding/web search)
fn model_supports_search(model_id: &str) -> bool {
    if let Some(model_config) = get_model_by_id(model_id) {
        // gemma-3-27b-it model doesn't support grounding
        if model_config.full_name.contains("gemma-3-27b-it") {
            return false;
        }
        // Gemini models support search
        if model_id.contains("gemini") || model_id.contains("gemma") {
            return true;
        }
        // Groq compound models support search
        if model_id.contains("compound") {
            return true;
        }
    }
    false
}

/// Request a node graph view reset (scale=1.0, centered)
/// This sets a flag that the patched egui-snarl library will check
pub fn request_node_graph_view_reset(ctx: &egui::Context) {
    let reset_id = egui::Id::new("snarl_reset_view");
    ctx.data_mut(|d| d.insert_temp(reset_id, true));
}

/// Node type for the processing chain
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum ChainNode {
    /// Input node (audio/image/text source)
    Input {
        id: String,
        block_type: String, // "audio", "image", "text"
        model: String,
        prompt: String,
        language_vars: HashMap<String, String>,
        show_overlay: bool,
        streaming_enabled: bool,
        render_mode: String,
        auto_copy: bool,
    },
    /// Processing node (transforms text)
    Process {
        id: String,
        model: String,
        prompt: String,
        language_vars: HashMap<String, String>,
        show_overlay: bool,
        streaming_enabled: bool,
        render_mode: String,
        auto_copy: bool,
    },
}

impl Default for ChainNode {
    fn default() -> Self {
        ChainNode::Process {
            id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
            language_vars: HashMap::new(),
            show_overlay: true,
            streaming_enabled: true,
            render_mode: "stream".to_string(),
            auto_copy: false,
        }
    }
}

impl ChainNode {
    pub fn is_input(&self) -> bool {
        matches!(self, ChainNode::Input { .. })
    }

    /// Convert to ProcessingBlock for execution
    pub fn to_block(&self) -> ProcessingBlock {
        match self {
            ChainNode::Input { id, block_type, model, prompt, language_vars, show_overlay, streaming_enabled, render_mode, auto_copy } => {
                ProcessingBlock {
                    id: id.clone(),
                    block_type: block_type.clone(),
                    model: model.clone(),
                    prompt: prompt.clone(),
                    selected_language: language_vars.get("language1").cloned().unwrap_or_default(),
                    language_vars: language_vars.clone(),
                    show_overlay: *show_overlay,
                    streaming_enabled: *streaming_enabled,
                    render_mode: render_mode.clone(),
                    auto_copy: *auto_copy,
                }
            }
            ChainNode::Process { id, model, prompt, language_vars, show_overlay, streaming_enabled, render_mode, auto_copy } => {
                ProcessingBlock {
                    id: id.clone(),
                    block_type: "text".to_string(),
                    model: model.clone(),
                    prompt: prompt.clone(),
                    selected_language: language_vars.get("language1").cloned().unwrap_or_default(),
                    language_vars: language_vars.clone(),
                    show_overlay: *show_overlay,
                    streaming_enabled: *streaming_enabled,
                    render_mode: render_mode.clone(),
                    auto_copy: *auto_copy,
                }
            }
        }
    }

    /// Create from ProcessingBlock
    pub fn from_block(block: &ProcessingBlock, is_first: bool) -> Self {
        // Populate language_vars from selected_language if missing (legacy support)
        let mut language_vars = block.language_vars.clone();
        if !language_vars.contains_key("language1") && !block.selected_language.is_empty() {
             language_vars.insert("language1".to_string(), block.selected_language.clone());
        }

        if is_first {
            ChainNode::Input {
                id: block.id.clone(),
                block_type: block.block_type.clone(),
                model: block.model.clone(),
                prompt: block.prompt.clone(),
                language_vars,
                show_overlay: block.show_overlay,
                streaming_enabled: block.streaming_enabled,
                render_mode: block.render_mode.clone(),
                auto_copy: block.auto_copy,
            }
        } else {
            ChainNode::Process {
                id: block.id.clone(),
                model: block.model.clone(),
                prompt: block.prompt.clone(),
                language_vars,
                show_overlay: block.show_overlay,
                streaming_enabled: block.streaming_enabled,
                render_mode: block.render_mode.clone(),
                auto_copy: block.auto_copy,
            }
        }
    }
}

/// Viewer implementation for the processing chain graph
impl ChainNode {
    pub fn id(&self) -> &str {
        match self {
            ChainNode::Input { id, .. } => id,
            ChainNode::Process { id, .. } => id,
        }
    }

    pub fn set_auto_copy(&mut self, val: bool) {
        match self {
            ChainNode::Input { auto_copy, .. } => *auto_copy = val,
            ChainNode::Process { auto_copy, .. } => *auto_copy = val,
        }
    }
}

pub struct ChainViewer {
    pub ui_language: String,
    pub changed: bool,
    pub language_search: String,
    pub use_groq: bool,
    pub use_gemini: bool,
    pub use_openrouter: bool,
    pub use_ollama: bool,
}

impl ChainViewer {
    pub fn new(ui_language: &str, _prompt_mode: &str, use_groq: bool, use_gemini: bool, use_openrouter: bool, use_ollama: bool) -> Self {
        Self {
            ui_language: ui_language.to_string(),
            changed: false,
            language_search: String::new(),
            use_groq,
            use_gemini,
            use_openrouter,
            use_ollama,
        }
    }
    
    /// Check if a model's provider is enabled
    fn is_provider_enabled(&self, provider: &str) -> bool {
        match provider {
            "groq" => self.use_groq,
            "google" => self.use_gemini,
            "openrouter" => self.use_openrouter,
            "ollama" => self.use_ollama,
            _ => true, // Unknown providers are enabled by default
        }
    }
}

impl SnarlViewer<ChainNode> for ChainViewer {
    fn title(&mut self, node: &ChainNode) -> String {
        match node {
            ChainNode::Input { block_type, .. } => {
                let type_name = match (block_type.as_str(), self.ui_language.as_str()) {
                    ("audio", "vi") => "Âm thanh",
                    ("image", "vi") => "Hình ảnh",
                    ("text", "vi") => "Văn bản",
                    ("audio", "ko") => "오디오",
                    ("image", "ko") => "이미지",
                    ("text", "ko") => "텍스트",
                    ("audio", _) => "Audio",
                    ("image", _) => "Image",
                    ("text", _) => "Text",
                    _ => "Input",
                };
                let prefix = match self.ui_language.as_str() {
                    "vi" => "Đầu vào:",
                    "ko" => "입력:",
                    _ => "Input:",
                };
                format!("{} {}", prefix, type_name)
            }
            ChainNode::Process { .. } => {
                match self.ui_language.as_str() {
                    "vi" => "Xử lý".to_string(),
                    "ko" => "처리".to_string(),
                    _ => "Process".to_string(),
                }
            }
        }
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<ChainNode>,
    ) {
        use crate::gui::icons::{Icon, draw_icon_static};
        
        let node = &snarl[node];
        ui.horizontal(|ui| {
            // Add icon based on node type
            match node {
                ChainNode::Input { block_type, .. } => {
                    let icon = match block_type.as_str() {
                        "image" => Icon::Image,
                        "audio" => Icon::Microphone,
                        "text" => Icon::Text,
                        _ => Icon::Settings,
                    };
                    draw_icon_static(ui, icon, Some(16.0));
                    
                    let type_name = match (block_type.as_str(), self.ui_language.as_str()) {
                        ("audio", "vi") => "Âm thanh",
                        ("image", "vi") => "Hình ảnh",
                        ("text", "vi") => "Văn bản",
                        ("audio", "ko") => "오디오",
                        ("image", "ko") => "이미지",
                        ("text", "ko") => "텍스트",
                        ("audio", _) => "Audio",
                        ("image", _) => "Image",
                        ("text", _) => "Text",
                        _ => "Input",
                    };
                    let prefix = match self.ui_language.as_str() {
                        "vi" => "Đầu vào:",
                        "ko" => "입력:",
                        _ => "Input:",
                    };
                    ui.label(format!("{} {}", prefix, type_name));
                }
                ChainNode::Process { .. } => {
                    draw_icon_static(ui, Icon::Settings, Some(16.0));
                    let title = match self.ui_language.as_str() {
                        "vi" => "Xử lý",
                        "ko" => "처리",
                        _ => "Process",
                    };
                    ui.label(title);
                }
            };
        });
    }

    // Use default header colors (no custom coloring)

    fn inputs(&mut self, node: &ChainNode) -> usize {
        match node {
            ChainNode::Input { .. } => 0, // Input nodes have no inputs
            ChainNode::Process { .. } => 1, // Process nodes have 1 input
        }
    }

    fn outputs(&mut self, _node: &ChainNode) -> usize {
        1 // All nodes have 1 output
    }

    fn show_input(&mut self, _pin: &InPin, _ui: &mut egui::Ui, _snarl: &mut Snarl<ChainNode>) -> impl egui_snarl::ui::SnarlPin + 'static {
        // Green color for text connections
        PinInfo::circle().with_fill(egui::Color32::from_rgb(100, 200, 100))
    }

    fn show_output(&mut self, _pin: &OutPin, _ui: &mut egui::Ui, _snarl: &mut Snarl<ChainNode>) -> impl egui_snarl::ui::SnarlPin + 'static {
        // Blue color for output
        PinInfo::circle().with_fill(egui::Color32::from_rgb(100, 150, 255))
    }







    fn has_body(&mut self, _node: &ChainNode) -> bool {
        true
    }

    fn show_body(
        &mut self,
        node_id: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<ChainNode>,
    ) {
        let mut auto_copy_triggered = false;
        let current_node_uuid = snarl.get_node(node_id).map(|n| n.id().to_string()).unwrap_or_default();
        
        // Render Node UI
        {
            let node = snarl.get_node_mut(node_id).unwrap();
            
            ui.vertical(|ui| {
                ui.set_max_width(320.0);
                
                match node {
                    ChainNode::Input { block_type, model, prompt, language_vars, show_overlay, streaming_enabled, render_mode, auto_copy, .. } => {
                        // Row 1: Model
                        let model_label = match self.ui_language.as_str() { "vi" => "Mô hình:", "ko" => "모델:", _ => "Model:" };
                        ui.horizontal(|ui| {
                            ui.label(model_label);
                            let model_def = get_model_by_id(model);
                            let display_name = model_def.as_ref()
                                .map(|m| match self.ui_language.as_str() { "vi" => m.name_vi.as_str(), "ko" => m.name_ko.as_str(), _ => m.name_en.as_str() })
                                .unwrap_or(model.as_str());
                            
                            let filter_type = match block_type.as_str() {
                                "audio" => ModelType::Audio,
                                "image" => ModelType::Vision,
                                _ => ModelType::Text,
                            };
                            
                            // Model selector button with manual popup for tight width
                            let button_response = ui.button(display_name);
                            if button_response.clicked() {
                                egui::Popup::toggle_id(ui.ctx(), button_response.id);
                                // Trigger background scan when popup opens
                                if self.use_ollama {
                                    trigger_ollama_model_scan();
                                }
                            }
                            let popup_layer_id = button_response.id;
                            egui::Popup::from_toggle_button_response(&button_response)
                                .show(|ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend); // No text wrapping, auto width
                                
                                // Show Ollama loading indicator if scanning
                                if self.use_ollama && is_ollama_scan_in_progress() {
                                    let loading_text = match self.ui_language.as_str() {
                                        "vi" => "⏳ Đang quét các model local...",
                                        "ko" => "⏳ 로컬 모델 스캔 중...",
                                        _ => "⏳ Scanning local models...",
                                    };
                                    ui.label(egui::RichText::new(loading_text).weak().italics());
                                    ui.separator();
                                }
                                
                            for m in get_all_models_with_ollama() {
                                    if m.enabled && m.model_type == filter_type && self.is_provider_enabled(&m.provider) {
                                        let name = match self.ui_language.as_str() { 
                                            "vi" => &m.name_vi, 
                                            "ko" => &m.name_ko, 
                                            _ => &m.name_en 
                                        };
                                        let quota = match self.ui_language.as_str() { 
                                            "vi" => &m.quota_limit_vi, 
                                            "ko" => &m.quota_limit_ko, 
                                            _ => &m.quota_limit_en 
                                        };
                                        let search_icon = if model_supports_search(&m.id) { "🔍 " } else { "" };
                                        let label = format!("{}{} - {} - {}", search_icon, name, m.full_name, quota);
                                        let is_selected = *model == m.id;
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            *model = m.id.clone();
                                            self.changed = true;
                                            egui::Popup::toggle_id(ui.ctx(), popup_layer_id);
                                        }
                                    }
                                }
                            });
                        });

                        // Prompt Section (hidden for Whisper audio models only)
                        let is_whisper = block_type == "audio" && model.starts_with("whisper");

                        if !is_whisper {
                            // Row 2: Prompt Label + Add Tag Button
                            ui.horizontal(|ui| {
                                let prompt_label = match self.ui_language.as_str() { "vi" => "Lệnh:", "ko" => "프롬프트:", _ => "Prompt:" };
                                ui.label(prompt_label);
                                
                                let btn_label = match self.ui_language.as_str() { "vi" => "+ Ngôn ngữ", "ko" => "+ 언어", _ => "+ Language" };
                                let is_dark = ui.visuals().dark_mode;
                                let lang_btn_bg = if is_dark { 
                                    egui::Color32::from_rgb(50, 100, 110) 
                                } else { 
                                    egui::Color32::from_rgb(100, 160, 170) 
                                };
                                if ui.add(egui::Button::new(egui::RichText::new(btn_label).small().color(egui::Color32::WHITE))
                                    .fill(lang_btn_bg)
                                    .corner_radius(8.0))
                                    .clicked() {
                                    insert_next_language_tag(prompt, language_vars);
                                    self.changed = true;
                                }
                            });
                            
                            // Row 3: Prompt TextEdit
                            if ui.add(egui::TextEdit::multiline(prompt).desired_width(152.0).desired_rows(2)).changed() {
                                self.changed = true;
                            }
                            
                            // Row 4+: Language Variables
                            show_language_vars(ui, &self.ui_language, prompt, language_vars, &mut self.changed, &mut self.language_search);
                        }

                        // Bottom Row: Settings
                        ui.horizontal(|ui| {
                            let icon = if *show_overlay { Icon::EyeOpen } else { Icon::EyeClosed };
                            if icon_button(ui, icon).clicked() { 
                                *show_overlay = !*show_overlay;
                                self.changed = true;
                            }
                            
                            if *show_overlay {
                                // Render Mode Dropdown (Normal, Stream, Markdown)
                                let current_mode_label = match (render_mode.as_str(), *streaming_enabled) {
                                    ("markdown", _) => match self.ui_language.as_str() { "vi" => "Đẹp", "ko" => "마크다운", _ => "Markdown" },
                                    (_, true) => match self.ui_language.as_str() { "vi" => "Stream", "ko" => "스트림", _ => "Stream" },
                                    (_, false) => match self.ui_language.as_str() { "vi" => "Thường", "ko" => "일반", _ => "Normal" },
                                };

                                egui::ComboBox::from_id_salt(format!("render_mode_{:?}", node_id))
                                    .selected_text(current_mode_label)
                                    .width(0.0)
                                    .show_ui(ui, |ui| {
                                        let (lbl_norm, lbl_stm, lbl_md) = match self.ui_language.as_str() {
                                            "vi" => ("Thường", "Stream", "Đẹp"),
                                            "ko" => ("일반", "스트림", "마크다운"), 
                                            _ => ("Normal", "Stream", "Markdown"),
                                        };

                                        if ui.selectable_label(render_mode == "plain" && !*streaming_enabled, lbl_norm).clicked() {
                                            *render_mode = "plain".to_string();
                                            *streaming_enabled = false;
                                            self.changed = true;
                                        }
                                        if ui.selectable_label((render_mode == "stream" || render_mode == "plain") && *streaming_enabled, lbl_stm).clicked() {
                                            *render_mode = "stream".to_string();
                                            *streaming_enabled = true;
                                            self.changed = true;
                                        }
                                        if ui.selectable_label(render_mode == "markdown", lbl_md).clicked() {
                                            *render_mode = "markdown".to_string();
                                            *streaming_enabled = false;
                                            self.changed = true;
                                        }
                                    });
                            }
                            
                            let copy_label = match self.ui_language.as_str() { "vi" => "Copy", "ko" => "복사", _ => "Copy" };
                            if ui.checkbox(auto_copy, copy_label).changed() {
                                self.changed = true;
                                if *auto_copy { auto_copy_triggered = true; }
                            }
                        });
                    }
                    ChainNode::Process { model, prompt, language_vars, show_overlay, streaming_enabled, render_mode, auto_copy, .. } => {
                        // Row 1: Model
                        let model_label = match self.ui_language.as_str() { "vi" => "Mô hình:", "ko" => "모델:", _ => "Model:" };
                        ui.horizontal(|ui| {
                            ui.label(model_label);
                            let model_def = get_model_by_id(model);
                            let display_name = model_def.as_ref()
                                .map(|m| match self.ui_language.as_str() { "vi" => m.name_vi.as_str(), "ko" => m.name_ko.as_str(), _ => m.name_en.as_str() })
                                .unwrap_or(model.as_str());
                            
                            // Model selector button with manual popup for tight width

                            let button_response = ui.button(display_name);
                            if button_response.clicked() {
                                egui::Popup::toggle_id(ui.ctx(), button_response.id);
                                // Trigger background scan when popup opens
                                if self.use_ollama {
                                    trigger_ollama_model_scan();
                                }
                            }
                            let popup_layer_id = button_response.id;
                            egui::Popup::from_toggle_button_response(&button_response)
                                .show(|ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend); // No text wrapping, auto width
                                
                                // Show Ollama loading indicator if scanning
                                if self.use_ollama && is_ollama_scan_in_progress() {
                                    let loading_text = match self.ui_language.as_str() {
                                        "vi" => "⏳ Đang quét các model local...",
                                        "ko" => "⏳ 로컬 모델 스캔 중...",
                                        _ => "⏳ Scanning local models...",
                                    };
                                    ui.label(egui::RichText::new(loading_text).weak().italics());
                                    ui.separator();
                                }
                                
                                for m in get_all_models_with_ollama() {
                                    if m.enabled && m.model_type == ModelType::Text && self.is_provider_enabled(&m.provider) {
                                        let name = match self.ui_language.as_str() { 
                                            "vi" => &m.name_vi, 
                                            "ko" => &m.name_ko, 
                                            _ => &m.name_en 
                                        };
                                        let quota = match self.ui_language.as_str() { 
                                            "vi" => &m.quota_limit_vi, 
                                            "ko" => &m.quota_limit_ko, 
                                            _ => &m.quota_limit_en 
                                        };
                                        let search_icon = if model_supports_search(&m.id) { "🔍 " } else { "" };
                                        let label = format!("{}{} - {} - {}", search_icon, name, m.full_name, quota);
                                        let is_selected = *model == m.id;
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            *model = m.id.clone();
                                            self.changed = true;
                                            egui::Popup::toggle_id(ui.ctx(), popup_layer_id);
                                        }
                                    }
                                }
                            });
                        });

                        // Row 2: Prompt Label + Add Tag Button
                        ui.horizontal(|ui| {
                            let prompt_label = match self.ui_language.as_str() { "vi" => "Lệnh:", "ko" => "프롬프트:", _ => "Prompt:" };
                            ui.label(prompt_label);
                            
                            let btn_label = match self.ui_language.as_str() { "vi" => "+ Ngôn ngữ", "ko" => "+ 언어", _ => "+ Language" };
                            let is_dark = ui.visuals().dark_mode;
                            let lang_btn_bg = if is_dark { 
                                egui::Color32::from_rgb(50, 100, 110) 
                            } else { 
                                egui::Color32::from_rgb(100, 160, 170) 
                            };
                            if ui.add(egui::Button::new(egui::RichText::new(btn_label).small().color(egui::Color32::WHITE))
                                .fill(lang_btn_bg)
                                .corner_radius(8.0))
                                .clicked() {
                                insert_next_language_tag(prompt, language_vars);
                                self.changed = true;
                            }
                        });

                        // Row 3: Prompt TextEdit
                        if ui.add(egui::TextEdit::multiline(prompt).desired_width(152.0).desired_rows(2)).changed() {
                            self.changed = true;
                        }
                        
                        // Row 4+: Language Variables
                        show_language_vars(ui, &self.ui_language, prompt, language_vars, &mut self.changed, &mut self.language_search);

                        // Bottom Row: Settings
                        ui.horizontal(|ui| {
                            let icon = if *show_overlay { Icon::EyeOpen } else { Icon::EyeClosed };
                            if icon_button(ui, icon).clicked() { 
                                *show_overlay = !*show_overlay;
                                self.changed = true;
                            }
                            
                            if *show_overlay {
                                // Render Mode Dropdown (Normal, Stream, Markdown)
                                let current_mode_label = match (render_mode.as_str(), *streaming_enabled) {
                                    ("markdown", _) => match self.ui_language.as_str() { "vi" => "Đẹp", "ko" => "마크다운", _ => "Markdown" },
                                    (_, true) => match self.ui_language.as_str() { "vi" => "Stream", "ko" => "스트림", _ => "Stream" },
                                    (_, false) => match self.ui_language.as_str() { "vi" => "Thường", "ko" => "일반", _ => "Normal" },
                                };

                                egui::ComboBox::from_id_salt(format!("render_mode_{:?}", node_id))
                                    .selected_text(current_mode_label)
                                    .width(0.0)
                                    .show_ui(ui, |ui| {
                                        let (lbl_norm, lbl_stm, lbl_md) = match self.ui_language.as_str() {
                                            "vi" => ("Thường", "Stream", "Đẹp"),
                                            "ko" => ("일반", "스트림", "마크다운"), 
                                            _ => ("Normal", "Stream", "Markdown"),
                                        };

                                        if ui.selectable_label(render_mode == "plain" && !*streaming_enabled, lbl_norm).clicked() {
                                            *render_mode = "plain".to_string();
                                            *streaming_enabled = false;
                                            self.changed = true;
                                        }
                                        if ui.selectable_label((render_mode == "stream" || render_mode == "plain") && *streaming_enabled, lbl_stm).clicked() {
                                            *render_mode = "stream".to_string();
                                            *streaming_enabled = true;
                                            self.changed = true;
                                        }
                                        if ui.selectable_label(render_mode == "markdown", lbl_md).clicked() {
                                            *render_mode = "markdown".to_string();
                                            *streaming_enabled = false;
                                            self.changed = true;
                                        }
                                    });
                            }
                            
                            let copy_label = match self.ui_language.as_str() { "vi" => "Copy", "ko" => "복사", _ => "Copy" };
                            if ui.checkbox(auto_copy, copy_label).changed() {
                                self.changed = true;
                                if *auto_copy { auto_copy_triggered = true; }
                            }
                        });
                    }
                }
            });
        }
        
        // Enforce auto-copy exclusivity
        if auto_copy_triggered {
            for node in snarl.nodes_mut() {
                if node.id() != current_node_uuid {
                    node.set_auto_copy(false);
                }
            }
        }
    }



    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<ChainNode>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut egui::Ui, snarl: &mut Snarl<ChainNode>) {
        let add_process_label = match self.ui_language.as_str() {
            "vi" => "➕ Thêm bước xử lý",
            "ko" => "➕ 처리 단계 추가",
            _ => "➕ Add Process Node",
        };
        
        if ui.button(add_process_label).clicked() {
            snarl.insert_node(pos, ChainNode::default());
            self.changed = true;
            ui.close();
        }
    }

    fn has_node_menu(&mut self, node: &ChainNode) -> bool {
        !node.is_input() // Only show menu for non-input nodes
    }

    fn show_node_menu(&mut self, node_id: NodeId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<ChainNode>) {
        let delete_label = match self.ui_language.as_str() {
            "vi" => "🗑 Xóa node",
            "ko" => "🗑 노드 삭제",
            _ => "🗑 Delete Node",
        };
        
        if ui.button(delete_label).clicked() {
            snarl.remove_node(node_id);
            self.changed = true;
            ui.close();
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<ChainNode>) {
        // Default behavior - allow connection
        snarl.connect(from.id, to.id);
        self.changed = true;
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<ChainNode>) {
        snarl.disconnect(from.id, to.id);
        self.changed = true;
    }
}

fn show_language_vars(ui: &mut egui::Ui, _ui_language: &str, prompt: &str, language_vars: &mut HashMap<String, String>, changed: &mut bool, _search_query: &mut String) {
    // Find {languageN} tags in prompt
    let mut detected_vars = Vec::new();
    for k in 1..=10 {
        let tag = format!("{{language{}}}", k);
        if prompt.contains(&tag) {
            detected_vars.push(k);
        }
    }

    for num in detected_vars {
        let key = format!("language{}", num);
        if !language_vars.contains_key(&key) {
            language_vars.insert(key.clone(), "Vietnamese".to_string());
        }
        
        let label = format!("{{language{}}}:", num);

        ui.horizontal(|ui| {
            ui.label(label);
            let current_val = language_vars.get(&key).cloned().unwrap_or_default();
            
            // Create unique IDs for this specific language selector

            let search_id = egui::Id::new(format!("lang_search_{}", num));
            
            // Styled button to open popup
            let is_dark = ui.visuals().dark_mode;
            let lang_var_bg = if is_dark { 
                egui::Color32::from_rgb(70, 60, 100) 
            } else { 
                egui::Color32::from_rgb(150, 140, 180) 
            };
            let button_response = ui.add(egui::Button::new(egui::RichText::new(&current_val).color(egui::Color32::WHITE))
                .fill(lang_var_bg)
                .corner_radius(8.0));
            
            if button_response.clicked() {
                egui::Popup::toggle_id(ui.ctx(), button_response.id);
            }
            
            let popup_layer_id = button_response.id;
            egui::Popup::from_toggle_button_response(&button_response)
                .show(|ui| {
                ui.set_min_width(120.0);
                
                // Get or create search state for this popup from temp data
                let mut search_text: String = ui.data_mut(|d| d.get_temp(search_id).unwrap_or_default());
                
                // Search box
                let _search_response = ui.add(
                    egui::TextEdit::singleline(&mut search_text)
                        .hint_text("Search...")
                        .desired_width(110.0)
                );
                
                // Store search state back
                ui.data_mut(|d| d.insert_temp(search_id, search_text.clone()));
                
                ui.separator();
                
                // Language list in scroll area
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    ui.set_width(120.0); // Ensure scrollbar stays on the right edge
                    for lang in get_all_languages() {
                        let matches_search = search_text.is_empty() || lang.to_lowercase().contains(&search_text.to_lowercase());
                        if matches_search {
                            let is_selected = current_val == *lang;
                            if ui.selectable_label(is_selected, lang).clicked() {
                                language_vars.insert(key.clone(), lang.clone());
                                *changed = true;
                                // Clear search and close popup
                                ui.data_mut(|d| d.insert_temp::<String>(search_id, String::new()));
                                egui::Popup::toggle_id(ui.ctx(), popup_layer_id);
                            }
                        }
                    }
                });
            });
        });
    }
}

fn insert_next_language_tag(prompt: &mut String, language_vars: &mut HashMap<String, String>) {
    let mut max_num = 0;
    for k in 1..=10 {
        if prompt.contains(&format!("{{language{}}}", k)) {
            max_num = k;
        }
    }
    let next_num = max_num + 1;
    let tag = format!(" {{language{}}} ", next_num);
    prompt.push_str(&tag);
    
    let key = format!("language{}", next_num);
    if !language_vars.contains_key(&key) {
        language_vars.insert(key, "Vietnamese".to_string());
    }
}

/// Convert blocks to snarl graph with intelligent layout
pub fn blocks_to_snarl(blocks: &[ProcessingBlock], connections: &[(usize, usize)]) -> Snarl<ChainNode> {
    let mut snarl = Snarl::new();
    let mut node_ids = Vec::new();
    
    // Default layout parameters
    let start_x = 50.0;
    let start_y = 300.0; // Center vertically
    let spacing_x = 250.0; // Increased to widen the graph
    let spacing_y = 225.0; // Increased to prevent vertical overlap (nodes are tall)
    
    // Calculate positions based on graph structure
    let positions: Vec<egui::Pos2> = if !connections.is_empty() {
        use std::collections::{HashMap, VecDeque};
        
        // 1. Build adjacency
        let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(from, to) in connections {
            adj.entry(from).or_default().push(to);
        }
        
        // 2. Compute depth (layer) for each node via BFS
        let mut depths = vec![0; blocks.len()];
        let mut layer_nodes: HashMap<usize, Vec<usize>> = HashMap::new();
        
        let mut queue = VecDeque::new();
        queue.push_back((0, 0)); // Start BFS from node 0 (input)
        
        // Track visited to prevent cycles infinite loop (though unlikely in current DAG)
        let mut visited = vec![false; blocks.len()];
        visited[0] = true;
        
        while let Some((u, d)) = queue.pop_front() {
            depths[u] = d;
            layer_nodes.entry(d).or_default().push(u);
            
            if let Some(children) = adj.get(&u) {
                for &v in children {
                    if v < blocks.len() && !visited[v] {
                        visited[v] = true;
                        queue.push_back((v, d + 1));
                    }
                }
            }
        }
        
        // Handle disconnected nodes (put them at depth 0 or end? let's put at end)
        // Actually, let's just stick to default linear if not reachable, or append
        
        // 3. Assign positions
        let mut pos_map = vec![egui::pos2(0.0, 0.0); blocks.len()];
        
        for (depth, nodes) in layer_nodes.iter() {
            let count = nodes.len();
            let layer_height = (count as f32) * spacing_y;
            let layer_start_y = start_y - (layer_height / 2.0) + (spacing_y / 2.0);
            
            for (i, &node_idx) in nodes.iter().enumerate() {
                let x = start_x + (*depth as f32) * spacing_x;
                let y = layer_start_y + (i as f32) * spacing_y;
                pos_map[node_idx] = egui::pos2(x, y);
            }
        }
        
        // Fallback for unreachable nodes (if any) -> just place them linearly far away
        for i in 0..blocks.len() {
            if !visited[i] {
                pos_map[i] = egui::pos2(start_x + i as f32 * spacing_x, start_y + 300.0);
            }
        }
        
        pos_map
    } else {
        // Legacy linear layout
        blocks.iter().enumerate().map(|(i, _)| {
            egui::pos2(start_x + i as f32 * spacing_x, start_y)
        }).collect()
    };
    
    // 3. Create nodes
    for (i, block) in blocks.iter().enumerate() {
        let node = ChainNode::from_block(block, i == 0);
        let pos = positions[i];
        let node_id = snarl.insert_node(pos, node);
        node_ids.push(node_id);
    }
    
    // 4. Create connections
    if !connections.is_empty() {
        for &(from_idx, to_idx) in connections {
            if from_idx < node_ids.len() && to_idx < node_ids.len() {
                let from = OutPinId { node: node_ids[from_idx], output: 0 };
                let to = InPinId { node: node_ids[to_idx], input: 0 };
                snarl.connect(from, to);
            }
        }
    } else if blocks.len() > 1 {
        // Legacy fallback
        for i in 0..node_ids.len() - 1 {
            let from = OutPinId { node: node_ids[i], output: 0 };
            let to = InPinId { node: node_ids[i+1], input: 0 };
            snarl.connect(from, to);
        }
    }
    
    snarl
}

/// Convert snarl graph back to blocks and connections
/// Returns (blocks, connections) where connections is Vec<(from_idx, to_idx)>
pub fn snarl_to_graph(snarl: &Snarl<ChainNode>) -> (Vec<ProcessingBlock>, Vec<(usize, usize)>) {
    use std::collections::{HashMap, VecDeque};
    
    let mut blocks = Vec::new();
    let mut connections = Vec::new();
    let mut node_to_idx: HashMap<NodeId, usize> = HashMap::new();
    
    // Find input node (the one with is_input() true)
    let mut input_node_id: Option<NodeId> = None;
    for (node_id, node) in snarl.node_ids() {
        if node.is_input() {
            input_node_id = Some(node_id);
            break;
        }
    }
    
    // BFS traversal from input node to collect all reachable nodes
    if let Some(start_id) = input_node_id {
        let mut queue = VecDeque::new();
        queue.push_back((start_id, true)); // (node_id, is_first)
        
        while let Some((node_id, is_first)) = queue.pop_front() {
            // Skip if already processed
            if node_to_idx.contains_key(&node_id) {
                continue;
            }
            
            if let Some(node) = snarl.get_node(node_id) {
                let mut block = node.to_block();
                if !is_first {
                    block.block_type = "text".to_string();
                }
                
                let idx = blocks.len();
                node_to_idx.insert(node_id, idx);
                blocks.push(block);
                
                // Find all downstream nodes (fan-out support)
                let out_pin = OutPinId { node: node_id, output: 0 };
                for (from, to) in snarl.wires() {
                    if from == out_pin {
                        queue.push_back((to.node, false));
                    }
                }
            }
        }
        
        // Second pass: build connections using node_to_idx mapping
        for (from, to) in snarl.wires() {
            if let (Some(&from_idx), Some(&to_idx)) = (node_to_idx.get(&from.node), node_to_idx.get(&to.node)) {
                connections.push((from_idx, to_idx));
            }
        }
    }
    
    (blocks, connections)
}

/// Render the node graph in the preset editor
pub fn render_node_graph(
    ui: &mut egui::Ui,
    snarl: &mut Snarl<ChainNode>,
    ui_language: &str,
    prompt_mode: &str,
    use_groq: bool,
    use_gemini: bool,
    use_openrouter: bool,
    use_ollama: bool,
) -> bool {
    let mut viewer = ChainViewer::new(ui_language, prompt_mode, use_groq, use_gemini, use_openrouter, use_ollama);
    let style = SnarlStyle::default();
    
    snarl.show(&mut viewer, &style, egui::Id::new("chain_graph"), ui);
    
    // Constraint Enforcement: Post-update cleanup
    // 1. No self-loops
    // 2. Single connection per input
    
    let mut to_disconnect = Vec::new();
    let mut input_count: HashMap<InPinId, Vec<OutPinId>> = HashMap::new();
    
    for (out, inp) in snarl.wires() {
        if out.node == inp.node {
            to_disconnect.push((out, inp));
        } else {
            input_count.entry(inp).or_default().push(out);
        }
    }
    
    for (_inp, sources) in input_count {
        if sources.len() > 1 {
            // More than 1 connection: Keep the last one encountered (arbitrary but consistent)
            // discard all but last
            for &src in sources.iter().take(sources.len() - 1) {
                // We re-construct iterator to find inp... wait sources is OutPinIDs
                // We need (OutPinId, InPinId) to disconnect
                // But disconnect takes (Out, In)? Yes.
                to_disconnect.push((src, _inp));
            }
        }
    }
    
    let mut cleanup_changed = false;
    for (out, inp) in to_disconnect {
        snarl.disconnect(out, inp);
        cleanup_changed = true;
    }
    
    viewer.changed || cleanup_changed
}
