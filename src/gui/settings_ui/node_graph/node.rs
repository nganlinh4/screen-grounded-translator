use crate::config::ProcessingBlock;
use std::collections::HashMap;

/// Node type for the processing chain
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum ChainNode {
    /// Input node (audio/image/text source)
    Input {
        id: String,
        block_type: String, // "audio", "image", "text", "input_adapter"
        auto_copy: bool,
        auto_speak: bool,
        // Removed processing fields
    },
    /// Special Processing Node (First level processor, Presets)
    Special {
        id: String,
        block_type: String,
        model: String,
        prompt: String,
        language_vars: HashMap<String, String>,
        show_overlay: bool,
        streaming_enabled: bool,
        render_mode: String,
        auto_copy: bool,
        auto_speak: bool,
    },
    /// Processing node (transforms text)
    Process {
        id: String,
        block_type: String,
        model: String,
        prompt: String,
        language_vars: HashMap<String, String>,
        show_overlay: bool,
        streaming_enabled: bool,
        render_mode: String,
        auto_copy: bool,
        auto_speak: bool,
    },
}

impl Default for ChainNode {
    fn default() -> Self {
        ChainNode::Process {
            id: format!(
                "{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ),
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
            language_vars: HashMap::new(),
            show_overlay: true,
            streaming_enabled: true,
            render_mode: "stream".to_string(),
            auto_copy: false,
            auto_speak: false,
        }
    }
}

impl ChainNode {
    pub fn is_input(&self) -> bool {
        matches!(self, ChainNode::Input { .. })
    }

    pub fn is_special(&self) -> bool {
        matches!(self, ChainNode::Special { .. })
    }

    /// Convert to ProcessingBlock for execution
    pub fn to_block(&self) -> ProcessingBlock {
        match self {
            ChainNode::Input {
                id,
                block_type: _,
                auto_copy,
                auto_speak,
            } => {
                ProcessingBlock {
                    id: id.clone(),
                    block_type: "input_adapter".to_string(), // Always adapter for Input Node
                    model: String::new(),
                    prompt: String::new(),
                    selected_language: String::new(),
                    language_vars: HashMap::new(),
                    show_overlay: false,
                    streaming_enabled: false,
                    render_mode: "plain".to_string(),
                    auto_copy: *auto_copy,
                    auto_speak: *auto_speak,
                }
            }
            ChainNode::Special {
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
            }
            | ChainNode::Process {
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
            } => ProcessingBlock {
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
                auto_speak: *auto_speak,
            },
        }
    }

    /// Create from ProcessingBlock
    pub fn from_block(block: &ProcessingBlock, role: &str) -> Self {
        // role: "input", "special", "process"

        // Populate language_vars from selected_language if missing (legacy support)
        let mut language_vars = block.language_vars.clone();
        if !language_vars.contains_key("language1") && !block.selected_language.is_empty() {
            language_vars.insert("language1".to_string(), block.selected_language.clone());
        }

        match role {
            "input" => ChainNode::Input {
                id: block.id.clone(),
                block_type: block.block_type.clone(),
                auto_copy: block.auto_copy,
                auto_speak: block.auto_speak,
            },
            "special" => ChainNode::Special {
                id: block.id.clone(),
                block_type: block.block_type.clone(),
                model: block.model.clone(),
                prompt: block.prompt.clone(),
                language_vars,
                show_overlay: block.show_overlay,
                streaming_enabled: block.streaming_enabled,
                render_mode: block.render_mode.clone(),
                auto_copy: block.auto_copy,
                auto_speak: block.auto_speak,
            },
            _ => ChainNode::Process {
                id: block.id.clone(),
                block_type: block.block_type.clone(),
                model: block.model.clone(),
                prompt: block.prompt.clone(),
                language_vars,
                show_overlay: block.show_overlay,
                streaming_enabled: block.streaming_enabled,
                render_mode: block.render_mode.clone(),
                auto_copy: block.auto_copy,
                auto_speak: block.auto_speak,
            },
        }
    }

    pub fn id(&self) -> &str {
        match self {
            ChainNode::Input { id, .. }
            | ChainNode::Special { id, .. }
            | ChainNode::Process { id, .. } => id,
        }
    }

    pub fn set_auto_copy(&mut self, val: bool) {
        match self {
            ChainNode::Input { auto_copy, .. } => *auto_copy = val,
            ChainNode::Special { auto_copy, .. } => *auto_copy = val,
            ChainNode::Process { auto_copy, .. } => *auto_copy = val,
        }
    }
}
