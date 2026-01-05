use crate::gui::locale::LocaleText;

pub fn get_realtime_html(
    is_translation: bool,
    audio_source: &str,
    languages: &[String],
    current_language: &str,
    translation_model: &str,
    transcription_model: &str,
    font_size: u32,
    text: &LocaleText,
) -> String {
    let _title_icon = if is_translation {
        "translate"
    } else {
        "graphic_eq"
    };
    let title_text = if is_translation {
        text.realtime_translation
    } else {
        text.realtime_listening
    };
    let glow_color = if is_translation { "#ff9633" } else { "#00c8ff" };

    // Title content: volume bars for transcription, text for translation
    let title_content = if is_translation {
        format!("{}", title_text)
    } else {
        // Canvas-based volume visualizer for smooth 60fps animation
        r#"<canvas id="volume-canvas" width="90" height="24"></canvas>"#.to_string()
    };

    let _mic_text = text.realtime_mic;
    let _device_text = text.realtime_device;
    let placeholder_text = text.realtime_waiting;

    // Build language options HTML - show full name in dropdown, but store code for display
    let lang_options: String = languages
        .iter()
        .map(|lang| {
            let selected = if lang == current_language {
                "selected"
            } else {
                ""
            };
            // Get 2-letter ISO 639-1 code
            let lang_code = isolang::Language::from_name(lang)
                .and_then(|l| l.to_639_1())
                .map(|c| c.to_uppercase())
                .unwrap_or_else(|| lang.chars().take(2).collect::<String>().to_uppercase());
            // Option shows full name, but we store code as data attribute for selected display
            format!(
                r#"<option value="{}" data-code="{}" {}>{}</option>"#,
                lang, lang_code, selected, lang
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Audio source selector (only for transcription window) - simple mic/device toggle
    let audio_selector = if !is_translation {
        let is_device = audio_source == "device";
        let gemini_active = if transcription_model == "gemini" {
            "active"
        } else {
            ""
        };
        let parakeet_active = if transcription_model == "parakeet" {
            "active"
        } else {
            ""
        };

        format!(
            r#"
            <div class="btn-group">
                <span class="material-symbols-rounded audio-icon {mic_active}" id="mic-btn" data-value="mic" title="Microphone Input">{mic_svg}</span>
                <span class="material-symbols-rounded audio-icon {device_active}" id="device-btn" data-value="device" title="Device Audio">{device_svg}</span>
            </div>
            <div class="btn-group">
                <span class="material-symbols-rounded trans-model-icon {gemini_active}" data-value="gemini" title="Gemini Live (Cloud)">{auto_awesome_svg}</span>
                <span class="material-symbols-rounded trans-model-icon {parakeet_active}" data-value="parakeet" title="Parakeet (Local)">{bolt_en_svg}</span>
            </div>
        "#,
            mic_active = if !is_device { "active" } else { "" },
            device_active = if is_device { "active" } else { "" },
            gemini_active = gemini_active,
            parakeet_active = parakeet_active,
            mic_svg = crate::overlay::html_components::icons::get_icon_svg("mic"),
            device_svg = crate::overlay::html_components::icons::get_icon_svg("speaker_group"),
            auto_awesome_svg = crate::overlay::html_components::icons::get_icon_svg("auto_awesome"),
            bolt_en_svg = crate::overlay::html_components::icons::get_icon_svg("bolt_en")
        )
    } else {
        // Language selector and model toggle for translation window
        let gemma_active = if translation_model == "google-gemma" {
            "active"
        } else {
            ""
        };
        let cerebras_active = if translation_model == "cerebras-oss" {
            "active"
        } else {
            ""
        };
        let gtx_active = if translation_model == "google-gtx" {
            "active"
        } else {
            ""
        };

        format!(
            r#"
            <span class="ctrl-btn speak-btn" id="speak-btn" title="Text-to-Speech Settings"><span class="material-symbols-rounded">{volume_up_svg}</span></span>
            <div class="btn-group">
                <span class="material-symbols-rounded model-icon {gemma_active}" data-value="google-gemma" title="AI Translation (Gemma)">{auto_awesome_svg}</span>
                <span class="material-symbols-rounded model-icon {cerebras_active}" data-value="cerebras-oss" title="Instant AI (Cerebras)">{speed_svg}</span>
                <span class="material-symbols-rounded model-icon {gtx_active}" data-value="google-gtx" title="Unlimited Translation (Google)">{language_svg}</span>
            </div>
            <select id="language-select" title="Target Language">
                {lang_options}
            </select>
        "#,
            lang_options = lang_options,
            gemma_active = gemma_active,
            cerebras_active = cerebras_active,
            gtx_active = gtx_active,
            volume_up_svg = crate::overlay::html_components::icons::get_icon_svg("volume_up"),
            auto_awesome_svg = crate::overlay::html_components::icons::get_icon_svg("auto_awesome"),
            speed_svg = crate::overlay::html_components::icons::get_icon_svg("speed"),
            language_svg = crate::overlay::html_components::icons::get_icon_svg("language")
        )
    };

    let loading_icon = if is_translation {
        r##"<svg class="loading-svg" viewBox="0 -6 24 36" fill="none" stroke="#ff9633" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><g class="trans-part-1"><path d="m5 8 6 6"></path><path d="m4 14 6-6 2-3"></path><path d="M2 5h12"></path><path d="M7 2h1"></path></g><g class="trans-part-2"><path d="m22 22-5-10-5 10"></path><path d="M14 18h6"></path></g></svg>"##
    } else {
        r##"<svg class="loading-svg" viewBox="0 -12 24 48" fill="none" stroke="#00c8ff" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"><line class="wave-line delay-1" x1="4" y1="8" x2="4" y2="16"></line><line class="wave-line delay-2" x1="9" y1="4" x2="9" y2="20"></line><line class="wave-line delay-3" x1="14" y1="6" x2="14" y2="18"></line><line class="wave-line delay-4" x1="19" y1="8" x2="19" y2="16"></line></svg>"##
    };

    // Construct CSS and JS from components
    let css = format!(
        "{}{}",
        crate::overlay::html_components::css_main::get(glow_color, font_size),
        crate::overlay::html_components::css_modals::get()
    );
    let js = format!(
        "{}{}",
        crate::overlay::html_components::js_main::get(font_size),
        crate::overlay::html_components::js_logic::get(placeholder_text)
    );

    // Get local font CSS (cached fonts, no network loading)
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        {font_css}
        {css_content}
    </style>
</head>
<body>
    <div id="loading-overlay">{loading_icon}</div>
    <div id="container">
        <div id="header">
            <div id="title">{title_content}</div>
            <div id="controls">
                {audio_selector}
                <span class="ctrl-btn" id="copy-btn" title="Copy text"><span class="material-symbols-rounded">{content_copy_svg}</span></span>
                <div class="pill-group">
                    <span class="ctrl-btn" id="font-decrease" title="Decrease font size"><span class="material-symbols-rounded">{remove_svg}</span></span>
                    <span class="ctrl-btn" id="font-increase" title="Increase font size"><span class="material-symbols-rounded">{add_svg}</span></span>
                </div>
                <div class="btn-group">
                    <span class="vis-btn mic active" id="toggle-mic" title="Toggle Transcription"><span class="material-symbols-rounded">{subtitles_svg}</span></span>
                    <span class="vis-btn trans active" id="toggle-trans" title="Toggle Translation"><span class="material-symbols-rounded">{translate_svg}</span></span>
                </div>
            </div>
        </div>
        <div id="header-toggle" title="Toggle header"><span class="material-symbols-rounded">{expand_less_svg}</span></div>
        <div id="viewport">
            <div id="content">
                <span class="placeholder">{placeholder_text}</span>
            </div>
        </div>
        <div id="resize-hint"><span class="material-symbols-rounded" style="font-size: 20px;">{pip_svg}</span></div>
    </div>
    <!-- Download Modal -->
    <div id="download-modal-overlay"></div>
    <div id="download-modal">
        <div class="download-modal-title">
            <span class="material-symbols-rounded">{download_svg}</span>
            <span id="download-title">Downloading Model</span>
        </div>
        <div class="download-modal-msg" id="download-msg">Please wait...</div>
        <div class="download-progress-bar">
            <div class="download-progress-fill" id="download-fill" style="width: 0%;"></div>
        </div>
        <div class="download-modal-footnote">{supports_english}</div>
    </div>
    <!-- TTS Settings Modal -->
    <div id="tts-modal-overlay"></div>
    <div id="tts-modal">
        <div class="tts-modal-title">
            <span class="material-symbols-rounded">{volume_up_svg}</span>
            {tts_title}
            <div class="toggle-switch" id="tts-toggle" style="margin-left: auto;"></div>
        </div>
        <div class="tts-modal-row">
            <span class="tts-modal-label">{tts_speed}</span>
            <div class="speed-slider-container">
                <input type="range" class="speed-slider" id="speed-slider" min="50" max="200" value="100" step="10">
                <span class="speed-value" id="speed-value">1.0x</span>
                <button class="auto-toggle on" id="auto-speed-toggle" title="Auto-adjust speed to catch up">{tts_auto}</button>
            </div>
    </div>
            </div>
        </div>
    </div>
    <!-- App Selection Modal -->
    <div id="app-modal-overlay"></div>
    <div id="app-modal">
        <div class="app-modal-title">
            <span class="material-symbols-rounded">{apps_svg}</span>
            {app_select_title}
        </div>
        <div class="app-modal-hint">{app_select_hint}</div>
        <div id="app-list" class="app-list">
            <div class="app-loading">Loading...</div>
        </div>
    </div>
    <script>
        {js_content}
    </script>
</body>
</html>"#,
        css_content = css,
        js_content = js,
        loading_icon = loading_icon,
        title_content = title_content,
        audio_selector = audio_selector,
        placeholder_text = placeholder_text,
        tts_title = text.realtime_tts_title,
        tts_speed = text.realtime_tts_speed,
        tts_auto = text.realtime_tts_auto,
        app_select_title = text.app_select_title,
        app_select_hint = text.app_select_hint,
        content_copy_svg = crate::overlay::html_components::icons::get_icon_svg("content_copy"),
        remove_svg = crate::overlay::html_components::icons::get_icon_svg("remove"),
        add_svg = crate::overlay::html_components::icons::get_icon_svg("add"),
        subtitles_svg = crate::overlay::html_components::icons::get_icon_svg("subtitles"),
        translate_svg = crate::overlay::html_components::icons::get_icon_svg("translate"),
        expand_less_svg = crate::overlay::html_components::icons::get_icon_svg("expand_less"),
        pip_svg = crate::overlay::html_components::icons::get_icon_svg("picture_in_picture_small"),
        volume_up_svg = crate::overlay::html_components::icons::get_icon_svg("volume_up"),
        apps_svg = crate::overlay::html_components::icons::get_icon_svg("apps"),
        download_svg = crate::overlay::html_components::icons::get_icon_svg("download"),
        supports_english = text.parakeet_supports_english_only
    )
}
