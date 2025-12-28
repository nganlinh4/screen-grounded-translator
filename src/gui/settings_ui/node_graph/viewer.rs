use super::body::show_body;
use super::node::ChainNode;
use crate::gui::icons::{draw_icon_static, Icon};
use crate::gui::locale::LocaleText;
use eframe::egui;
use egui_snarl::ui::{PinInfo, SnarlViewer};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};

pub struct ChainViewer<'a> {
    pub text: &'a LocaleText,
    pub ui_language: String,
    pub changed: bool,
    pub language_search: String,
    pub use_groq: bool,
    pub use_gemini: bool,
    pub use_openrouter: bool,
    pub use_ollama: bool,
    pub preset_type: String, // "image", "audio", "text"
}

impl<'a> ChainViewer<'a> {
    pub fn new(
        text: &'a LocaleText,
        ui_language: &str,
        _prompt_mode: &str,
        use_groq: bool,
        use_gemini: bool,
        use_openrouter: bool,
        use_ollama: bool,
        preset_type: &str,
    ) -> Self {
        Self {
            text,
            ui_language: ui_language.to_string(),
            changed: false,
            language_search: String::new(),
            use_groq,
            use_gemini,
            use_openrouter,
            use_ollama,
            preset_type: preset_type.to_string(),
        }
    }

    /// Check if a model's provider is enabled
    pub fn is_provider_enabled(&self, provider: &str) -> bool {
        match provider {
            "groq" => self.use_groq,
            "google" => self.use_gemini,
            "openrouter" => self.use_openrouter,
            "ollama" => self.use_ollama,
            _ => true, // Unknown providers are enabled by default
        }
    }
}

impl<'a> SnarlViewer<ChainNode> for ChainViewer<'a> {
    fn title(&mut self, node: &ChainNode) -> String {
        match node {
            ChainNode::Input { block_type, .. } => {
                let actual_type = if block_type == "input_adapter" {
                    self.preset_type.as_str()
                } else {
                    block_type.as_str()
                };
                let type_name = match actual_type {
                    "audio" => self.text.node_input_audio,
                    "image" => self.text.node_input_image,
                    "text" => self.text.node_input_text,
                    _ => "Input",
                };
                let prefix = self.text.node_input_prefix;
                format!("{} {}", prefix, type_name)
            }
            ChainNode::Special { .. } => {
                // Dynamic title based on preset type
                match self.preset_type.as_str() {
                    "image" => self.text.node_special_image_to_text.to_string(),
                    "audio" => self.text.node_special_audio_to_text.to_string(),
                    _ => self.text.node_special_default.to_string(),
                }
            }
            ChainNode::Process { .. } => self.text.node_process_title.to_string(),
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
        let node = &snarl[node];
        // Reverting to vertical-centered horizontal layout which is standard/safe
        ui.horizontal(|ui| {
            // Add icon based on node type
            match node {
                ChainNode::Input { block_type, .. } => {
                    let actual_type = if block_type == "input_adapter" {
                        self.preset_type.as_str()
                    } else {
                        block_type.as_str()
                    };
                    let icon = match actual_type {
                        "image" => Icon::Image,
                        "audio" => Icon::Microphone,
                        "text" => Icon::Text,
                        _ => Icon::Settings,
                    };
                    draw_icon_static(ui, icon, Some(16.0));

                    let type_name = match actual_type {
                        "audio" => self.text.node_input_audio,
                        "image" => self.text.node_input_image,
                        "text" => self.text.node_input_text,
                        _ => "Input",
                    };
                    let prefix = self.text.node_input_prefix;
                    ui.label(format!("{} {}", prefix, type_name));
                }
                ChainNode::Process { .. } => {
                    draw_icon_static(ui, Icon::Settings, Some(16.0));
                    let title = self.text.node_process_title;
                    ui.label(title);
                }

                ChainNode::Special { .. } => {
                    draw_icon_static(ui, Icon::Settings, Some(16.0));
                    // Dynamic header based on preset type
                    let title = match self.preset_type.as_str() {
                        "image" => self.text.node_special_image_to_text,
                        "audio" => self.text.node_special_audio_to_text,
                        _ => self.text.node_special_default,
                    };
                    ui.label(
                        egui::RichText::new(title).color(egui::Color32::from_rgb(255, 200, 100)),
                    );
                }
            };
        });
    }

    // Use default header colors (no custom coloring)

    fn inputs(&mut self, node: &ChainNode) -> usize {
        match node {
            ChainNode::Input { .. } => 0, // Input nodes have no inputs
            ChainNode::Process { .. } | ChainNode::Special { .. } => 1, // Process nodes have 1 input
        }
    }

    fn outputs(&mut self, _node: &ChainNode) -> usize {
        1 // All nodes have 1 output
    }

    fn show_input(
        &mut self,
        _pin: &InPin,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<ChainNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        // Green color for text connections
        PinInfo::circle().with_fill(egui::Color32::from_rgb(100, 200, 100))
    }

    fn show_output(
        &mut self,
        _pin: &OutPin,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<ChainNode>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
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
        show_body(self, node_id, ui, snarl);
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<ChainNode>) -> bool {
        true
    }

    fn show_graph_menu(
        &mut self,
        pos: egui::Pos2,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<ChainNode>,
    ) {
        let add_process_label = self.text.node_menu_add_normal;
        let add_special_label = match self.preset_type.as_str() {
            "image" => self.text.node_menu_add_special_image,
            "audio" => self.text.node_menu_add_special_audio,
            _ => self.text.node_menu_add_special_generic,
        };

        if ui.button(add_process_label).clicked() {
            snarl.insert_node(pos, ChainNode::default());
            self.changed = true;
            ui.close();
        }
        if self.preset_type != "text" {
            if ui.button(add_special_label).clicked() {
                let mut node = ChainNode::default();
                // Force it to be Special
                if let ChainNode::Process {
                    id,
                    block_type,
                    model,
                    prompt,
                    language_vars,
                    show_overlay,
                    streaming_enabled,
                    render_mode,
                    auto_copy,
                    auto_speak,
                } = node
                {
                    node = ChainNode::Special {
                        id,
                        block_type,
                        model,
                        prompt,
                        language_vars,
                        show_overlay,
                        streaming_enabled,
                        render_mode,
                        auto_copy,
                        auto_speak,
                    };
                }
                snarl.insert_node(pos, node);
                self.changed = true;
                ui.close();
            }
        }
    }

    fn has_node_menu(&mut self, node: &ChainNode) -> bool {
        !node.is_input() // Only show menu for non-input nodes
    }

    fn show_node_menu(
        &mut self,
        node_id: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<ChainNode>,
    ) {
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
        // Enforce constraints:
        // Preceding node of Special Node can ONLY be the Input Node.

        let to_node = snarl.get_node(to.id.node);
        let from_node = snarl.get_node(from.id.node);

        if let (Some(to_node), Some(from_node)) = (to_node, from_node) {
            if to_node.is_special() {
                if !from_node.is_input() {
                    // Violation: Attempting to connect non-input to Special node
                    return;
                }
            }
        }

        snarl.connect(from.id, to.id);
        self.changed = true;
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<ChainNode>) {
        snarl.disconnect(from.id, to.id);
        self.changed = true;
    }
}
