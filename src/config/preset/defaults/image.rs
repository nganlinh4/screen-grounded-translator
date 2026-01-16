//! Default image presets using the builder pattern.

use crate::config::preset::{BlockBuilder, PresetBuilder};
use crate::config::preset::Preset;
use crate::config::types::Hotkey;

/// Create all default image presets
pub fn create_image_presets() -> Vec<Preset> {
    vec![
        // =====================================================================
        // TRANSLATION PRESETS
        // =====================================================================
        
        // Translate - Basic image-to-text translation
        PresetBuilder::new("preset_translate", "Translate")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract text from this image and translate it to {language1}. Output ONLY the translation text directly, do not add introductory text.")
                    .language("Vietnamese")
                    .markdown_stream() // Upgraded: Stream -> Đẹp+Str
                    .build(),
            ])
            .build(),

        // Translate (High accuracy) - OCR then translate
        {
            let mut p = PresetBuilder::new("preset_extract_retranslate", "Translate (High accuracy)")
                .image()
                .blocks(vec![
                    BlockBuilder::image("maverick")
                        .prompt("Extract all text from this image exactly as it appears. Output ONLY the text.")
                        .language("English")
                        .show_overlay(false)
                        .build(),
                    BlockBuilder::text("cerebras_qwen3")
                        .prompt("Translate to {language1}. Output ONLY the translation.")
                        .language("Vietnamese")
                        .markdown() // Upgraded: Thường -> Đẹp (Special: keeping non-streaming as requested by original logic for this specific preset)
                        .streaming(false)
                        .build(),
                ])
                .build();
            p.hotkeys.push(Hotkey::new(192, "` / ~", 0));
            p
        },

        // Translate (Auto paste) - Hidden overlay, auto-paste
        PresetBuilder::new("preset_translate_auto_paste", "Translate (Auto paste)")
            .image()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract text from this image and translate it to {language1}. Output ONLY the translation text directly, do not add introductory text.")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .markdown_stream() // Upgraded: Stream -> Đẹp+Str
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Translate+Retranslate - Korean then Vietnamese
        PresetBuilder::new("preset_translate_retranslate", "Translate+Retranslate")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract text from this image and translate it to {language1}. Output ONLY the translation text directly, do not add introductory text.")
                    .language("Korean")
                    .markdown() // Đẹp
                    .auto_copy()
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .markdown_stream() // Đẹp+Str
                    .build(),
            ])
            .build(),

        // Translate (Accurate)+Retranslate - Triple chain
        PresetBuilder::new("preset_extract_retrans_retrans", "Translate (Accurate)+Retranslate")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract all text from this image exactly as it appears. Output ONLY the text.")
                    .language("English")
                    .show_overlay(false)
                    .build(),
                BlockBuilder::text("cerebras_zai_glm_4_7")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Korean")
                    .markdown_stream() // Đẹp+Str
                    .auto_copy()
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .markdown_stream() // Đẹp+Str
                    .build(),
            ])
            .build(),

        // =====================================================================
        // EXTRACTION PRESETS
        // =====================================================================

        // Extract text (OCR)
        PresetBuilder::new("preset_ocr", "Extract text")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract all text from this image exactly as it appears. Output ONLY the text.")
                    .language("English")
                    .show_overlay(false)
                    .markdown() // Upgraded: Thường -> Đẹp
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Read this region - OCR with TTS
        PresetBuilder::new("preset_ocr_read", "Read this region")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract all text from this image exactly as it appears. Output ONLY the text.")
                    .language("English")
                    .show_overlay(false)
                    .markdown() // Upgraded: Thường -> Đẹp
                    .auto_speak()
                    .build(),
            ])
            .build(),

        // Quick Screenshot - Just capture and copy
        PresetBuilder::new("preset_quick_screenshot", "Quick Screenshot")
            .image()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Extract Table
        PresetBuilder::new("preset_extract_table", "Extract Table")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract all data from any tables, forms, or structured content in this image. Format the output as a markdown table. Output ONLY the table, no explanations.")
                    .language("Vietnamese")
                    .markdown()
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // QR Scanner - Scan and format QR code content
        PresetBuilder::new("preset_qr_scanner", "QR Scanner")
            .image()
            .blocks(vec![
                // Node 0: QR Scanner (non-LLM, extracts raw content)
                BlockBuilder::image("qr-scanner")
                    .prompt("") // QR scanner doesn't need a prompt
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
                // Node 1: Format the QR content nicely
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Format this QR code content for display. Rules:\n\
                        - If URL: Make it a clickable markdown link [URL](URL) and describe what this link points to\n\
                        - If vCard/contact: Format as a readable contact card with name, phone, email, address\n\
                        - If WiFi (WIFI:S:...): Extract and display SSID, password, and security type clearly\n\
                        - If plain text: Display as-is, translate if not in {language1}\n\
                        - If calendar event: Format as readable event with date/time/location\n\
                        - If email/SMS: Format with recipient and content clearly\n\
                        Output clean markdown. DO NOT include code blocks or backticks.")
                    .language("Vietnamese")
                    .markdown_stream() // Upgraded: Đẹp -> Đẹp+Str
                    .build(),
            ])
            .build(),

        // =====================================================================
        // ANALYSIS PRESETS
        // =====================================================================

        // Summarize content
        PresetBuilder::new("preset_summarize", "Summarize content")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Analyze this image and summarize its content in {language1}. Only return the summary text, super concisely. Format the output as a markdown. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks.")
                    .language("Vietnamese")
                    .markdown_stream() // Upgraded: Đẹp -> Đẹp+Str
                    .build(),
            ])
            .build(),

        // Image description
        PresetBuilder::new("preset_desc", "Image description")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Describe this image in {language1}. Format the output as a markdown. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks.")
                    .language("Vietnamese")
                    .markdown_stream() // Upgraded: Đẹp -> Đẹp+Str
                    .build(),
            ])
            .build(),

        // Ask about image - Dynamic prompt
        PresetBuilder::new("preset_ask_image", "Ask about image")
            .image()
            .dynamic_prompt()
            .blocks(vec![
                BlockBuilder::image("gemini-3-flash-preview")
                    .prompt("")
                    .language("Vietnamese")
                    .markdown_stream() // Upgraded: Đẹp -> Đẹp+Str
                    .build(),
            ])
            .build(),

        // =====================================================================
        // ADVANCED PRESETS
        // =====================================================================

        // Kiểm chứng thông tin (Fact Check)
        PresetBuilder::new("preset_fact_check", "Kiểm chứng thông tin")
            .image()
            .blocks(vec![
                BlockBuilder::image("maverick")
                    .prompt("Extract and describe all text, claims, statements, and information visible in this image. Include any context that might be relevant for fact-checking. Output the content clearly.")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .build(),
                BlockBuilder::text("compound_mini")
                    .prompt("Fact-check the following claims/information. Search the internet to verify accuracy. Provide a clear verdict (TRUE/FALSE/PARTIALLY TRUE/UNVERIFIABLE) for each claim with evidence and sources. Respond in {language1}. Format as markdown. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks.")
                    .language("Vietnamese")
                    .markdown_stream() // Đẹp+Str
                    .build(),
            ])
            .build(),

        // Thần Trí tuệ (Omniscient God) - Complex branching graph
        PresetBuilder::new("preset_omniscient_god", "Thần Trí tuệ (Omniscient God)")
            .image()
            .blocks(vec![
                // Node 0: Extract from image
                BlockBuilder::image("maverick")
                    .prompt("Analyze this image and extract all text, claims, and key information. Be detailed and comprehensive.")
                    .language("English")
                    .markdown()
                    .build(),
                // Node 1: Make a learning HTML (from 0)
                BlockBuilder::text("cerebras_zai_glm_4_7")
                    .prompt("Create a standalone INTERACTIVE HTML learning card/game for the following text. Use internal CSS for a beautiful, modern, colored design, game-like and comprehensive interface. Only OUTPUT the raw HTML code, DO NOT include HTML file indicator (```html) or triple backticks.")
                    .language("Vietnamese")
                    .markdown()
                    .build(),
                // Node 2: Summarize with sources (from 3)
                BlockBuilder::text("compound_mini")
                    .prompt("Search the internet to ensure of the accuracy of the following text as well as getting as much source information as possible. Summarize the following text into a detailed markdown summary with clickable links to the sources. Structure it clearly. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks.")
                    .language("Vietnamese")
                    .markdown_stream() // Đẹp+Str
                    .build(),
                // Node 3: Translate (from 0)
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate the following text to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .markdown_stream() // Đẹp+Str
                    .build(),
                // Node 4: Summarize keywords (from 3)
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Summarize the essence of this text into 3-5 keywords or a short phrase in {language1}.")
                    .markdown_stream() // Đẹp+Str
                    .language("Vietnamese")
                    .build(),
            ])
            .connections(vec![(0, 3), (0, 1), (3, 4), (3, 2)])
            .build(),

        // Treo ảnh - Input Adapter Only
        PresetBuilder::new("preset_hang_image", "Input Overlay (Image)")
            .image()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .show_overlay(true)
                    .markdown()
                    .build(),
            ])
            .build(),
    ]
}
