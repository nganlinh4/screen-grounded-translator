//! Default audio presets using the builder pattern.

use crate::config::preset::Preset;
use crate::config::preset::{BlockBuilder, PresetBuilder};

/// Create all default audio presets
pub fn create_audio_presets() -> Vec<Preset> {
    vec![
        // =====================================================================
        // MIC PRESETS
        // =====================================================================

        // Transcribe speech - Basic speech-to-text
        PresetBuilder::new("preset_transcribe", "Transcribe speech")
            .audio_mic()
            .auto_paste()
            .auto_stop()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .markdown() // Upgraded: Thường -> Đẹp
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Viết liên tục - Continuous writing (Online)
        PresetBuilder::new("preset_continuous_writing_online", "Viết liên tục")
            .audio_mic()
            .auto_paste()
            // No auto_stop
            .blocks(vec![
                BlockBuilder::audio("gemini-live-audio")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Fix pronunciation - Transcribe then speak back
        PresetBuilder::new("preset_fix_pronunciation", "Fix pronunciation")
            .audio_mic()
            .auto_stop()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .markdown() // Upgraded: Thường -> Đẹp
                    .auto_speak()
                    .build(),
            ])
            .build(),

        // Quick 4NR reply - Transcribe and translate
        PresetBuilder::new("preset_transcribe_retranslate", "Quick 4NR reply")
            .audio_mic()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .language("Korean")
                    .show_overlay(false)
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Korean")
                    .show_overlay(false)
                    .markdown_stream() // Đẹp+Str
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Quicker foreigner reply - Direct audio translation
        PresetBuilder::new("preset_quicker_foreigner_reply", "Quicker foreigner reply")
            .audio_mic()
            .auto_paste()
            .blocks(vec![
                BlockBuilder::audio("gemini-audio")
                    .prompt("Translate the audio to {language1}. Only output the translated text.")
                    .language("Korean")
                    .show_overlay(false)
                    .markdown_stream() // Đẹp+Str
                    .auto_copy()
                    .build(),
            ])
            .build(),

        // Quick AI Question - Speak to ask AI
        PresetBuilder::new("preset_quick_ai_question", "Quick AI Question")
            .audio_mic()
            .auto_stop()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Answer the following question concisely and helpfully. Format as markdown. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks.")
                    .markdown_stream() // Đẹp+Str
                    .build(),
            ])
            .build(),

        // Voice Search - Speak to search
        PresetBuilder::new("preset_voice_search", "Voice Search")
            .audio_mic()
            .auto_stop()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .language("Vietnamese")
                    .show_overlay(false)
                    .build(),
                BlockBuilder::text("compound_mini")
                    .prompt("Search the internet for information about the following query and provide a comprehensive summary. Include key facts, recent developments, and relevant details with clickable links to sources if possible. Format the output as markdown creatively. Only OUTPUT the markdown, DO NOT include markdown file indicator (```markdown) or triple backticks.")
                    .markdown_stream() // Đẹp+Str
                    .build(),
            ])
            .build(),

        // Thu âm nhanh - Input Adapter Only
        PresetBuilder::new("preset_quick_record", "Quick Record")
            .audio_mic()
            .auto_stop()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .show_overlay(true)
                    .markdown() // Đẹp
                    .build(),
            ])
            .build(),

        // =====================================================================
        // DEVICE AUDIO PRESETS
        // =====================================================================

        // Study language - Listen and translate
        PresetBuilder::new("preset_study_language", "Study language")
            .audio_device()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .language("Vietnamese")
                    .build(),
                BlockBuilder::text("cerebras_qwen3")
                    .prompt("Translate to {language1}. Output ONLY the translation.")
                    .language("Vietnamese")
                    .markdown_stream() // Đẹp+Str
                    .build(),
            ])
            .build(),

        // Live Translate - Realtime translation
        PresetBuilder::new("preset_realtime_audio_translate", "Live Translate")
            .audio_device()
            .realtime()
            .blocks(vec![
                BlockBuilder::audio("whisper-accurate")
                    .build(),
                BlockBuilder::text("google-gemma")
                    .language("Vietnamese")
                    .build(),
            ])
            .build(),

        // Thu âm máy - Input Adapter Only
        PresetBuilder::new("preset_record_device", "Record Device")
            .audio_device()
            .auto_stop()
            .blocks(vec![
                BlockBuilder::input_adapter()
                    .show_overlay(true)
                    .markdown()
                    .build(),
            ])
            .build(),

        // Chép lời TA - Transcribe English (Offline)
        PresetBuilder::new("preset_transcribe_english_offline", "Chép lời TA")
            .audio_device()
            .auto_paste()
            // No auto_stop
            .blocks(vec![
                BlockBuilder::audio("parakeet-local")
                    .language("English")
                    .show_overlay(false)
                    .auto_copy()
                    .build(),
            ])
            .build(),
    ]
}
