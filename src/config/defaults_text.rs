//! Default presets - Part 2: Text presets

use super::preset::Preset;
use super::types::ProcessingBlock;

/// Create text-based default presets
pub fn create_text_presets() -> Vec<Preset> {
    let mut presets = Vec::new();

    // 2b. Read aloud (Đọc to) - FIRST text preset
    let mut p2b = Preset::default();
    p2b.id = "preset_read_aloud".to_string();
    p2b.name = "Read aloud".to_string();
    p2b.preset_type = "text".to_string();
    p2b.text_input_mode = "select".to_string();
    p2b.blocks = vec![ProcessingBlock {
        block_type: "input_adapter".to_string(), // optimized: no LLM needed
        model: "".to_string(),
        prompt: "".to_string(),
        selected_language: "English".to_string(),
        streaming_enabled: false, // input adapter is instant
        show_overlay: false,
        auto_copy: false,
        auto_speak: true,
        ..Default::default()
    }];
    presets.push(p2b);

    // 3. Trans (Select text)
    let mut p3 = Preset::default();
    p3.id = "preset_translate_select".to_string();
    p3.name = "Trans (Select text)".to_string();
    p3.preset_type = "text".to_string();
    p3.text_input_mode = "select".to_string();
    p3.blocks = vec![ProcessingBlock {
        block_type: "text".to_string(),
        model: "text_accurate_kimi".to_string(),
        prompt: "Translate the following text to {language1}. Output ONLY the translation."
            .to_string(),
        selected_language: "Vietnamese".to_string(),
        streaming_enabled: true,
        show_overlay: true,
        auto_copy: true,
        ..Default::default()
    }];
    presets.push(p3);

    // 3h. Trans+Retrans (Select) - Dịch+Dịch lại (Bôi)
    let mut p3h = Preset::default();
    p3h.id = "preset_trans_retrans_select".to_string();
    p3h.name = "Trans+Retrans (Select)".to_string();
    p3h.preset_type = "text".to_string();
    p3h.text_input_mode = "select".to_string();
    p3h.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate the following text to {language1}. Output ONLY the translation."
                .to_string(),
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
        },
    ];
    presets.push(p3h);

    // 3b. Select-Trans-Replace (Bôi-Dịch-Thay)
    let mut p3b = Preset::default();
    p3b.id = "preset_select_translate_replace".to_string();
    p3b.name = "Select-Trans-Replace".to_string();
    p3b.preset_type = "text".to_string();
    p3b.text_input_mode = "select".to_string();
    p3b.auto_paste = true; // Replace original text
    p3b.blocks = vec![ProcessingBlock {
        block_type: "text".to_string(),
        model: "text_accurate_kimi".to_string(),
        prompt: "Translate the following text to {language1}. Output ONLY the translation."
            .to_string(),
        selected_language: "Vietnamese".to_string(),
        streaming_enabled: false,
        show_overlay: false, // Background processing
        auto_copy: true,
        ..Default::default()
    }];
    presets.push(p3b);

    // 3c. Fix Grammar (Sửa ngữ pháp)
    let mut p3c = Preset::default();
    p3c.id = "preset_fix_grammar".to_string();
    p3c.name = "Fix Grammar".to_string();
    p3c.preset_type = "text".to_string();
    p3c.text_input_mode = "select".to_string();
    p3c.auto_paste = true; // Replace original text
    p3c.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Correct grammar, spelling, and punctuation errors in the following text. Do not change the meaning or tone. Output ONLY the corrected text.".to_string(),
            selected_language: "Vietnamese".to_string(), // Not used but required
            streaming_enabled: false,
            show_overlay: false,
            auto_copy: true,
            ..Default::default()
        }
    ];
    presets.push(p3c);

    // 3d. Rephrase (Viết lại)
    let mut p3d = Preset::default();
    p3d.id = "preset_rephrase".to_string();
    p3d.name = "Rephrase".to_string();
    p3d.preset_type = "text".to_string();
    p3d.text_input_mode = "select".to_string();
    p3d.auto_paste = true;
    p3d.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Paraphrase the following text using varied vocabulary while maintaining the exact original meaning and language. Output ONLY the paraphrased text.".to_string(),
            selected_language: "Vietnamese".to_string(),
            streaming_enabled: false,
            show_overlay: false,
            auto_copy: true,
            ..Default::default()
        }
    ];
    presets.push(p3d);

    // 3e. Make Formal (Chuyên nghiệp hóa)
    let mut p3e = Preset::default();
    p3e.id = "preset_make_formal".to_string();
    p3e.name = "Make Formal".to_string();
    p3e.preset_type = "text".to_string();
    p3e.text_input_mode = "select".to_string();
    p3e.auto_paste = true;
    p3e.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Rewrite the following text to be professional and formal, suitable for business communication. CRITICAL: Your output MUST be in the EXACT SAME LANGUAGE as the input text (if input is Korean, output Korean; if Vietnamese, output Vietnamese; if Japanese, output Japanese, etc.). Do NOT translate to English. Maintain the original meaning. Output ONLY the rewritten text.".to_string(),
            selected_language: "Vietnamese".to_string(),
            streaming_enabled: false,
            show_overlay: false,
            auto_copy: true,
            ..Default::default()
        }
    ];
    presets.push(p3e);

    // 3f. Explain (Giải thích)
    let mut p3f = Preset::default();
    p3f.id = "preset_explain".to_string();
    p3f.name = "Explain".to_string();
    p3f.preset_type = "text".to_string();
    p3f.text_input_mode = "select".to_string();
    p3f.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Explain what this is in {language1}. Be concise but thorough. Mention the purpose, key logic, and any important patterns or techniques used. Format the output as a markdown. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) triple backticks.".to_string(),
            selected_language: "Vietnamese".to_string(),
            streaming_enabled: true,
            render_mode: "markdown".to_string(),
            show_overlay: true,
            auto_copy: false,
            ..Default::default()
        }
    ];
    presets.push(p3f);

    // 3f2. Ask about text (Hỏi về text) - dynamic prompt for text selection
    let mut p3f2 = Preset::default();
    p3f2.id = "preset_ask_text".to_string();
    p3f2.name = "Ask about text".to_string();
    p3f2.preset_type = "text".to_string();
    p3f2.text_input_mode = "select".to_string();
    p3f2.prompt_mode = "dynamic".to_string(); // User types custom command
    p3f2.blocks = vec![ProcessingBlock {
        block_type: "text".to_string(),
        model: "compound_mini".to_string(),
        prompt: "".to_string(), // Empty - user will provide
        selected_language: "Vietnamese".to_string(),
        streaming_enabled: true,
        render_mode: "markdown".to_string(),
        show_overlay: true,
        auto_copy: false,
        ..Default::default()
    }];
    presets.push(p3f2);

    // 3f3. Edit as follows (Sửa như sau:) - dynamic prompt for text selection
    let mut p3f3 = Preset::default();
    p3f3.id = "preset_edit_as_follows".to_string();
    p3f3.name = "Edit as follows:".to_string();
    p3f3.preset_type = "text".to_string();
    p3f3.text_input_mode = "select".to_string();
    p3f3.prompt_mode = "dynamic".to_string();
    p3f3.auto_paste = true;
    p3f3.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "compound_mini".to_string(),
            prompt: "Edit the following text according to the user's specific instructions. CRITICAL: Maintain the original language of the text unless instructed otherwise. Output ONLY the edited result without any introductory text, explanations, or quotes.".to_string(),
            selected_language: "Vietnamese".to_string(),
            streaming_enabled: true,
            render_mode: "stream".to_string(),
            show_overlay: false,
            auto_copy: true,
            ..Default::default()
        }
    ];
    presets.push(p3f3);

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
            prompt: "Translate the following text to {language1}. Output ONLY the translation. Text to translate:".to_string(),
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
    presets.push(p5);

    // 5a. Hỏi AI (Ask AI - non-internet version)
    let mut p5a = Preset::default();
    p5a.id = "preset_ask_ai".to_string();
    p5a.name = "Ask AI".to_string();
    p5a.preset_type = "text".to_string();
    p5a.text_input_mode = "type".to_string();
    p5a.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Answer the following question or request helpfully and comprehensively. Format the output as markdown creatively. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks. QUESTION/REQUEST:".to_string(),
            streaming_enabled: true,
            render_mode: "markdown".to_string(),
            show_overlay: true,
            auto_copy: false,
            ..Default::default()
        }
    ];
    presets.push(p5a);

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
            prompt: "Search the internet for information about the following query and provide a comprehensive summary. Include key facts, recent developments, and relevant details with clickable links to sources if possible. Format the output as markdown creatively. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks. SEARCH FOR:".to_string(),
            streaming_enabled: true,
            render_mode: "markdown".to_string(),
            show_overlay: true,
            auto_copy: false,
            ..Default::default()
        }
    ];
    presets.push(p5b);

    // 5c. Make a Game (Tạo con game)
    let mut p5c = Preset::default();
    p5c.id = "preset_make_game".to_string();
    p5c.name = "Make a Game".to_string();
    p5c.preset_type = "text".to_string();
    p5c.text_input_mode = "type".to_string();
    p5c.blocks = vec![
        ProcessingBlock {
            block_type: "text".to_string(),
            model: "gemini-flash".to_string(), // Use stronger model for coding
            prompt: "Create a complete, standalone HTML game. The game MUST be playable using ONLY MOUSE CONTROLS (like swipe , drag or clicks, no keyboard required). Avoid the looping Game Over UI at startup. Use modern and trending CSS on the internet for a polished look, prefer using images or icons or svg assets from the internet for a convincing game aesthetics. Provide HTML code only. Only OUTPUT the raw HTML code, DO NOT include HTML file indicator (```html) or triple backticks. Create the game based on the following request:".to_string(),
            streaming_enabled: true,
            render_mode: "markdown".to_string(),
            show_overlay: true,
            auto_copy: false,
            ..Default::default()
        }
    ];
    presets.push(p5c);

    presets
}
