//! Processing Block - the fundamental unit of a processing chain.
//!
//! A block represents a single processing step (OCR, translation, TTS, etc.)
//! Multiple blocks can be chained together in a preset.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::types::BlockType;

// ============================================================================
// PROCESSING BLOCK
// ============================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProcessingBlock {
    /// Unique identifier for this block
    #[serde(default = "generate_block_id")]
    pub id: String,

    /// Type of block: "input_adapter", "image", "text", "audio"
    #[serde(default)]
    pub block_type: String,

    /// Model ID to use for processing
    #[serde(default)]
    pub model: String,

    /// Prompt template (supports {language1}, {language2}, etc.)
    #[serde(default)]
    pub prompt: String,

    /// Primary selected language (legacy: maps to language_vars["language1"])
    #[serde(default)]
    pub selected_language: String,

    /// Language variable mappings for prompt template
    #[serde(default)]
    pub language_vars: HashMap<String, String>,

    /// Whether to stream the response
    #[serde(default = "default_true")]
    pub streaming_enabled: bool,

    /// Render mode: "stream", "plain", "markdown"
    #[serde(default = "default_render_mode")]
    pub render_mode: String,

    /// Whether to show the result overlay
    #[serde(default = "default_true")]
    pub show_overlay: bool,

    /// Auto-copy result to clipboard
    #[serde(default)]
    pub auto_copy: bool,

    /// Auto-speak result using TTS
    #[serde(default)]
    pub auto_speak: bool,
}

fn generate_block_id() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn default_true() -> bool {
    true
}

fn default_render_mode() -> String {
    "stream".to_string()
}

impl Default for ProcessingBlock {
    fn default() -> Self {
        Self {
            id: generate_block_id(),
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate to {language1}. Output ONLY the translation.".to_string(),
            selected_language: "Vietnamese".to_string(),
            language_vars: HashMap::new(),
            streaming_enabled: true,
            render_mode: "stream".to_string(),
            show_overlay: true,
            auto_copy: false,
            auto_speak: false,
        }
    }
}

// ============================================================================
// BLOCK BUILDER - Fluent API for creating blocks
// ============================================================================

/// Builder for creating ProcessingBlocks with a fluent API.
///
/// # Example
/// ```
/// let block = BlockBuilder::text("text_accurate_kimi")
///     .prompt("Translate to {language1}.")
///     .language("Vietnamese")
///     .streaming(true)
///     .build();
/// ```
pub struct BlockBuilder {
    block: ProcessingBlock,
}

impl BlockBuilder {
    /// Create a new text processing block
    pub fn text(model: &str) -> Self {
        Self {
            block: ProcessingBlock {
                block_type: "text".to_string(),
                model: model.to_string(),
                ..Default::default()
            },
        }
    }

    /// Create a new image/vision processing block
    pub fn image(model: &str) -> Self {
        Self {
            block: ProcessingBlock {
                block_type: "image".to_string(),
                model: model.to_string(),
                streaming_enabled: false, // Vision models typically don't stream
                ..Default::default()
            },
        }
    }

    /// Create a new audio processing block
    pub fn audio(model: &str) -> Self {
        Self {
            block: ProcessingBlock {
                block_type: "audio".to_string(),
                model: model.to_string(),
                streaming_enabled: false,
                prompt: String::new(), // Audio blocks often don't need prompts
                ..Default::default()
            },
        }
    }

    /// Create an input adapter (pass-through) block
    pub fn input_adapter() -> Self {
        Self {
            block: ProcessingBlock {
                block_type: "input_adapter".to_string(),
                model: String::new(),
                prompt: String::new(),
                streaming_enabled: false,
                show_overlay: false,
                ..Default::default()
            },
        }
    }

    /// Set the prompt template
    pub fn prompt(mut self, prompt: &str) -> Self {
        self.block.prompt = prompt.to_string();
        self
    }

    /// Set the primary language (for {language1} substitution)
    pub fn language(mut self, lang: &str) -> Self {
        self.block.selected_language = lang.to_string();
        self.block
            .language_vars
            .insert("language1".to_string(), lang.to_string());
        self
    }

    /// Enable/disable streaming
    pub fn streaming(mut self, enabled: bool) -> Self {
        self.block.streaming_enabled = enabled;
        self
    }

    /// Shorthand for markdown render mode
    pub fn markdown(mut self) -> Self {
        self.block.render_mode = "markdown".to_string();
        self
    }

    /// Shorthand for markdown streaming render mode
    pub fn markdown_stream(mut self) -> Self {
        self.block.render_mode = "markdown_stream".to_string();
        self.block.streaming_enabled = true;
        self
    }

    /// Set explicit render mode
    pub fn render_mode(mut self, mode: &str) -> Self {
        self.block.render_mode = mode.to_string();
        self
    }

    /// Enable/disable overlay display
    pub fn show_overlay(mut self, show: bool) -> Self {
        self.block.show_overlay = show;
        self
    }

    /// Enable auto-copy to clipboard
    pub fn auto_copy(mut self) -> Self {
        self.block.auto_copy = true;
        self
    }

    /// Enable auto-speak (TTS)
    pub fn auto_speak(mut self) -> Self {
        self.block.auto_speak = true;
        self
    }

    /// Build the final ProcessingBlock
    pub fn build(self) -> ProcessingBlock {
        self.block
    }
}

// ============================================================================
// BLOCK TYPE HELPERS
// ============================================================================

impl ProcessingBlock {
    /// Check if this is an input adapter block
    pub fn is_input_adapter(&self) -> bool {
        self.block_type == "input_adapter"
    }

    /// Check if this is an image/vision block
    pub fn is_image(&self) -> bool {
        self.block_type == "image"
    }

    /// Check if this is a text block
    pub fn is_text(&self) -> bool {
        self.block_type == "text"
    }

    /// Check if this is an audio block
    pub fn is_audio(&self) -> bool {
        self.block_type == "audio"
    }

    /// Get the block type as enum
    pub fn block_type_enum(&self) -> BlockType {
        BlockType::from_str(&self.block_type)
    }
}
