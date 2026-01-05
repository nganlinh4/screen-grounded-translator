//! Default text presets using the builder pattern.

use crate::config::preset::Preset;
use crate::config::preset::{BlockBuilder, PresetBuilder};

/// Create all default text presets
pub fn create_text_presets() -> Vec<Preset> {
    vec![
        // =====================================================================
        // TEXT SELECTION PRESETS (highlight text to process)
        // =====================================================================

        // Read aloud - TTS for selected text
        PresetBuilder::new("preset_read_aloud", "Read aloud")
            .text_select()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .auto_speak()
                    .build(),
            ])
            .build(),

        // Trans (Select text) - Translate highlighted text
        PresetBuilder::new("preset_translate_select", "Trans (Select text)")
            .text_select()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Dịch (Arena) - Compare 3 translation models in parallel
        PresetBuilder::new("preset_translate_arena", "Dịch (Arena)")
            .text_select()
            .blocks(vec![
                // Node 0: Input adapter (text selection)
                BlockBuilder::input_adapter()
                    .build(),
                // Node 1: Google Translate (GTX) - fast, non-LLM
                BlockBuilder::text("google-gtx")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .streaming(false)
                    .build(),
                // Node 2: Groq Kimi - accurate LLM
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .auto_copy()
                    .build(),
                // Node 3: Gemini Flash Lite - Google's fast LLM
                BlockBuilder::text("text_gemini_flash_lite")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .streaming(false)
                    .build(),
            ])
            // All 3 translation nodes branch from input (0 -> 1, 0 -> 2, 0 -> 3)
            .connections(vec![(0, 1), (0, 2), (0, 3)])
            .build(),

        // Trans+Retrans (Select) - Korean then Vietnamese
        PresetBuilder::new("preset_trans_retrans_select", "Trans+Retrans (Select)")
            .text_select()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation.")
                    .language("Korean")
                    .auto_copy()
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .build(),
            ])
            .build(),

        // Select-Trans-Replace - Translate and paste back
        PresetBuilder::new("preset_select_translate_replace", "Select-Trans-Replace")
            .text_select()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .streaming(false)
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Fix Grammar
        PresetBuilder::new("preset_fix_grammar", "Fix Grammar")
            .text_select()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Correct grammar, spelling, and punctuation errors in the following text. Do not change the meaning or tone. Output ONLY the corrected text.")
                    .language("Vietnamese")
                    .streaming(false)
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Rephrase
        PresetBuilder::new("preset_rephrase", "Rephrase")
            .text_select()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Paraphrase the following text using varied vocabulary while maintaining the exact original meaning and language. Output ONLY the paraphrased text.")
                    .language("Vietnamese")
                    .streaming(false)
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Make Formal
        PresetBuilder::new("preset_make_formal", "Make Formal")
            .text_select()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Rewrite the following text to be professional and formal, suitable for business communication. CRITICAL: Your output MUST be in the EXACT SAME LANGUAGE as the input text (if input is Korean, output Korean; if Vietnamese, output Vietnamese; if Japanese, output Japanese, etc.). Do NOT translate to English. Maintain the original meaning. Output ONLY the rewritten text.")
                    .language("Vietnamese")
                    .streaming(false)
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Explain
        PresetBuilder::new("preset_explain", "Explain")
            .text_select()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Explain what this is in {language1}. Be concise but thorough. Mention the purpose, key logic, and any important patterns or techniques used. Format the output as a markdown. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) triple backticks.")
                    .language("Vietnamese")
                    .markdown()
                    .build(),
            ])
            .build(),

        // Ask about text - Dynamic prompt
        PresetBuilder::new("preset_ask_text", "Ask about text")
            .text_select()
            .dynamic_prompt()
            .blocks(vec![
                BlockBuilder::text("compound_mini")
                    .prompt("")
                    .language("Vietnamese")
                    .markdown()
                    .build(),
            ])
            .build(),

        // Edit as follows - Dynamic prompt with auto-paste
        PresetBuilder::new("preset_edit_as_follows", "Edit as follows:")
            .text_select()
            .dynamic_prompt()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::text("compound_mini")
                    .prompt("Edit the following text according to the user's specific instructions. CRITICAL: Maintain the original language of the text unless instructed otherwise. Output ONLY the edited result without any introductory text, explanations, or quotes.")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // =====================================================================
        // TEXT TYPING PRESETS (type text to process)
        // =====================================================================

        // Treo text - Input Adapter Only
        PresetBuilder::new("preset_hang_text", "Input Overlay (Text)")
            .text_select()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .show_overlay(true)
                    .build(),
            ])
            .build(),

        // Trans+Retrans (Type) - Korean then Vietnamese with continuous mode
        PresetBuilder::new("preset_trans_retrans_typing", "Trans+Retrans (Type)")
            .text_type()
            .continuous()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation. Text to translate:")
                    .language("Korean")
                    .auto_copy()
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .build(),
            ])
            .build(),

        // Ask AI
        PresetBuilder::new("preset_ask_ai", "Ask AI")
            .text_type()
            .blocks(vec![
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Answer the following question or request helpfully and comprehensively. Format the output as markdown creatively. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks. QUESTION/REQUEST:")
                    .markdown()
                    .build(),
            ])
            .build(),

        // Internet Search
        PresetBuilder::new("preset_internet_search", "Internet Search")
            .text_type()
            .blocks(vec![
                BlockBuilder::text("compound_mini")
                    .prompt("Search the internet for information about the following query and provide a comprehensive summary. Include key facts, recent developments, and relevant details with clickable links to sources if possible. Format the output as markdown creatively. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks. SEARCH FOR:")
                    .markdown()
                    .build(),
            ])
            .build(),

        // Make a Game
        PresetBuilder::new("preset_make_game", "Make a Game")
            .text_type()
            .blocks(vec![
                BlockBuilder::text("gemini-flash")
                    .prompt("Create a complete, standalone HTML game. The game MUST be playable using ONLY MOUSE CONTROLS (like swipe , drag or clicks, no keyboard required). Avoid the looping Game Over UI at startup. Use modern and trending CSS on the internet for a polished look, prefer using images or icons or svg assets from the internet for a convincing game aesthetics. Provide HTML code only. Only OUTPUT the raw HTML code, DO NOT include HTML file indicator (```html) or triple backticks. Create the game based on the following request:")
                    .markdown()
                    .build(),
            ])
            .build(),

        // Note nhanh - Input Adapter Only
        PresetBuilder::new("preset_quick_note", "Quick Note")
            .text_type()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .show_overlay(true)
                    .build(),
            ])
            .build(),
    ]
}
