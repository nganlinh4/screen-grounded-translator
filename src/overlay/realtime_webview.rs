//! WebView2-based realtime transcription overlay
//! 
//! Uses smooth scrolling for a non-eye-sore reading experience.
//! Text appends at the bottom, viewport smoothly slides up.

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Mutex, Once};
use std::num::NonZeroIsize;
use std::collections::HashMap;
use wry::{WebViewBuilder, Rect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle, Win32WindowHandle, HandleError};
use crate::APP;
use crate::gui::locale::LocaleText;
use crate::config::get_all_languages;
use crate::api::realtime_audio::{
    start_realtime_transcription, RealtimeState, SharedRealtimeState,
    WM_REALTIME_UPDATE, WM_TRANSLATION_UPDATE, WM_VOLUME_UPDATE, WM_MODEL_SWITCH, REALTIME_RMS,
};

// Gap between realtime and translation overlays
const GAP: i32 = 20;

lazy_static::lazy_static! {
    pub static ref REALTIME_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    static ref REALTIME_STATE: SharedRealtimeState = Arc::new(Mutex::new(RealtimeState::new()));
    /// Signal to change audio source (true = restart with new source)
    pub static ref AUDIO_SOURCE_CHANGE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// The new audio source to use ("mic" or "device")
    pub static ref NEW_AUDIO_SOURCE: Mutex<String> = Mutex::new(String::new());
    /// Signal to change target language
    pub static ref LANGUAGE_CHANGE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// The new target language to use
    pub static ref NEW_TARGET_LANGUAGE: Mutex<String> = Mutex::new(String::new());
    /// Signal to change translation model
    pub static ref TRANSLATION_MODEL_CHANGE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// The new translation model to use ("google-gemma" or "groq-llama")
    pub static ref NEW_TRANSLATION_MODEL: Mutex<String> = Mutex::new(String::new());
    /// Visibility state for windows
    pub static ref MIC_VISIBLE: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    pub static ref TRANS_VISIBLE: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
}

static mut REALTIME_HWND: HWND = HWND(0);
static mut TRANSLATION_HWND: HWND = HWND(0);
static mut IS_ACTIVE: bool = false;

static REGISTER_REALTIME_CLASS: Once = Once::new();
static REGISTER_TRANSLATION_CLASS: Once = Once::new();

// Thread-local storage for WebViews
thread_local! {
    static REALTIME_WEBVIEWS: std::cell::RefCell<HashMap<isize, wry::WebView>> = std::cell::RefCell::new(HashMap::new());
}

/// Wrapper for HWND to implement HasWindowHandle
struct HwndWrapper(HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.0.0 as isize;
        if let Some(non_zero) = NonZeroIsize::new(hwnd) {
            let mut handle = Win32WindowHandle::new(non_zero);
            handle.hinstance = None;
            let raw = RawWindowHandle::Win32(handle);
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        } else {
            Err(HandleError::Unavailable)
        }
    }
}

/// CSS and HTML for the realtime overlay with smooth scrolling
fn get_realtime_html(is_translation: bool, audio_source: &str, languages: &[String], current_language: &str, translation_model: &str, font_size: u32, text: &LocaleText) -> String {
    let _title_icon = if is_translation { "translate" } else { "graphic_eq" };
    let title_text = if is_translation { text.realtime_translation } else { text.realtime_listening };
    let glow_color = if is_translation { "#ff9633" } else { "#00c8ff" };
    
    // Title content: volume bars for transcription, text for translation
    let title_content = if is_translation {
        format!("{}", title_text)
    } else {
        // Volume visualizer bars (30 bars for ~3 seconds of history at 100ms updates)
        let bars: String = (0..30).map(|_| r#"<div class="volume-bar" style="height: 3px;"></div>"#).collect::<Vec<_>>().join("");
        format!(r#"<div class="volume-bars" id="volume-bars">{}</div>"#, bars)
    };
    
    let _mic_text = text.realtime_mic;
    let _device_text = text.realtime_device;
    let placeholder_text = text.realtime_waiting;
    
    // Build language options HTML - show full name in dropdown, but store code for display
    let lang_options: String = languages.iter()
        .map(|lang| {
            let selected = if lang == current_language { "selected" } else { "" };
            // Get 2-letter ISO 639-1 code
            let lang_code = isolang::Language::from_name(lang)
                .and_then(|l| l.to_639_1())
                .map(|c| c.to_uppercase())
                .unwrap_or_else(|| lang.chars().take(2).collect::<String>().to_uppercase());
            // Option shows full name, but we store code as data attribute for selected display
            format!(r#"<option value="{}" data-code="{}" {}>{}</option>"#, lang, lang_code, selected, lang)
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    // Audio source selector (only for transcription window) - simple toggle switch
    let audio_selector = if !is_translation {
        let is_device = audio_source == "device";
        format!(r#"
            <div class="btn-group">
                <span class="material-symbols-rounded audio-icon {mic_active}" id="audio-toggle" data-value="mic" title="Microphone">mic</span>
                <span class="material-symbols-rounded audio-icon {device_active}" data-value="device" title="System Audio">speaker_group</span>
            </div>
        "#, 
            mic_active = if !is_device { "active" } else { "" },
            device_active = if is_device { "active" } else { "" }
        )
    } else {
        // Language selector and model toggle for translation window
        let gemma_active = if translation_model == "google-gemma" { "active" } else { "" };
        let groq_active = if translation_model == "groq-llama" { "active" } else { "" };
        let gtx_active = if translation_model == "google-gtx" { "active" } else { "" };

        format!(r#"
            <div class="btn-group">
                <span class="material-symbols-rounded model-icon {gemma_active}" data-value="google-gemma" title="AI Translation (Gemma)">auto_awesome</span>
                <span class="material-symbols-rounded model-icon {groq_active}" data-value="groq-llama" title="Fast Translation (Groq)">speed</span>
                <span class="material-symbols-rounded model-icon {gtx_active}" data-value="google-gtx" title="Unlimited Translation (Google)">language</span>
            </div>
            <select id="language-select" title="Target Language">
                {lang_options}
            </select>
        "#,
            lang_options = lang_options,
            gemma_active = gemma_active,
            groq_active = groq_active,
            gtx_active = gtx_active
        )
    };
    
    let loading_icon = if is_translation {
        r##"<svg class="loading-svg" viewBox="0 0 24 24" fill="none" stroke="#ff9633" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6"></path><path d="m4 14 6-6 2-3"></path><path d="M2 5h12"></path><path d="M7 2h1"></path><path d="m22 22-5-10-5 10"></path><path d="M14 18h6"></path></svg>"##
    } else {
        r##"<svg class="loading-svg" viewBox="0 0 24 24" fill="none" stroke="#00c8ff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"></path><path d="M19 10v1a7 7 0 0 1-14 0v-1"></path><line x1="12" x2="12" y1="19" y2="22"></line></svg>"##
    };

    format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link rel="preload" href="https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@24,400,1,0&display=swap" as="style" />
    <link rel="preload" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:opsz,slnt,wdth,wght,ROND@6..144,-10..0,25..151,100..1000,100&display=swap" as="style" />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@24,400,1,0&display=swap" />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:opsz,slnt,wdth,wght,ROND@6..144,-10..0,25..151,100..1000,100&display=swap" />
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        html, body {{
            height: 100%;
            overflow: hidden;
            background: rgba(26, 26, 26, 0.95);
            font-family: 'Google Sans Flex', sans-serif;
            color: #fff;
            border-radius: 8px;
            border: 1px solid {glow_color}40;
            box-shadow: 0 0 20px {glow_color}30;
        }}
        /* Loading overlay - covers content until fonts load, then fades out */
        #loading-overlay {{
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgb(26, 26, 26);
            z-index: 9999;
            pointer-events: none;
            display: flex;
            justify-content: center;
            align-items: center;
            animation: fadeOut 0.4s ease-out 0.9s forwards;
        }}
        .loading-svg {{
            width: 72px;
            height: 72px;
            filter: drop-shadow(0 0 12px {glow_color}90);
            animation: breathe 2.5s ease-in-out infinite;
        }}
        @keyframes breathe {{
            0%, 100% {{ 
                transform: scale(1); 
                opacity: 0.85;
                filter: drop-shadow(0 0 8px {glow_color}60);
            }}
            50% {{ 
                transform: scale(1.08); 
                opacity: 1;
                filter: drop-shadow(0 0 20px {glow_color});
            }}
        }}
        @keyframes fadeOut {{
            from {{ opacity: 1; }}
            to {{ opacity: 0; }}
        }}
        .material-symbols-rounded {{
            font-family: 'Material Symbols Rounded';
            font-weight: normal;
            font-style: normal;
            font-size: 18px;
            line-height: 1;
            letter-spacing: normal;
            text-transform: none;
            display: inline-block;
            white-space: nowrap;
            word-wrap: normal;
            direction: ltr;
            vertical-align: middle;
        }}
        #container {{
            display: flex;
            flex-direction: column;
            height: 100%;
            padding: 8px 12px;
            cursor: grab;
            position: relative;
        }}
        #container:active {{
            cursor: grabbing;
        }}
        #header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 6px;
            flex-shrink: 0;
            gap: 8px;
            transition: all 0.25s ease-out;
            overflow: hidden;
            max-height: 40px;
        }}
        #header.collapsed {{
            max-height: 0;
            margin-bottom: 0;
            opacity: 0;
        }}
        @keyframes pulse {{
            0%, 100% {{ transform: translateX(-50%) scale(1); opacity: 0.7; }}
            50% {{ transform: translateX(-50%) scale(1.2); opacity: 1; }}
        }}
        #header-toggle {{
            position: absolute;
            left: 50%;
            transform: translateX(-50%);
            display: flex;
            justify-content: center;
            align-items: center;
            cursor: pointer;
            padding: 2px 6px;
            color: #666;
            transition: all 0.25s ease-out;
            z-index: 10;
            top: 32px;
            opacity: 0.4;
        }}
        #header:hover ~ #header-toggle {{
            color: #00c8ff;
            opacity: 1;
            animation: pulse 1s ease-in-out infinite;
        }}
        #header-toggle:hover {{
            color: #fff;
            opacity: 1;
            animation: pulse 0.8s ease-in-out infinite;
        }}
        #header-toggle.collapsed {{
            top: 4px;
            opacity: 0.3;
            animation: none;
        }}
        #header-toggle.collapsed:hover {{
            opacity: 0.8;
        }}
        #header-toggle .material-symbols-rounded {{
            font-size: 14px;
            transition: transform 0.25s ease-out;
        }}
        #header-toggle.collapsed .material-symbols-rounded {{
            transform: rotate(180deg);
        }}
        #title {{
            font-size: 12px;
            font-weight: bold;
            color: #aaa;
            flex-shrink: 0;
            display: flex;
            align-items: center;
            gap: 6px;
        }}
        .volume-bars {{
            display: flex;
            align-items: center;
            gap: 1px;
            height: 16px;
        }}
        .volume-bar {{
            width: 2px;
            background: linear-gradient(to top, #00c8ff, #00f0ff);
            border-radius: 1px;
            transition: height 0.08s ease-out;
            min-height: 2px;
        }}
        #controls {{
            display: flex;
            gap: 8px;
            align-items: center;
            flex: 1;
            justify-content: flex-end;
        }}
        .btn-group {{
            display: flex;
            gap: 1px;
            align-items: center;
        }}
        .ctrl-btn {{
            font-size: 14px;
            color: #888;
            cursor: pointer;
            padding: 4px 8px;
            border-radius: 50%;
            background: rgba(30,30,30,0.8);
            border: 1px solid rgba(255,255,255,0.1);
            transition: all 0.2s;
            user-select: none;
            width: 26px;
            height: 26px;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .ctrl-btn:hover {{
            color: #fff;
            background: rgba(255,255,255,0.15);
            border-color: {glow_color};
            box-shadow: 0 0 8px {glow_color}40;
        }}
        .vis-btn {{
            font-size: 14px;
            cursor: pointer;
            padding: 2px;
            border-radius: 4px;
            transition: all 0.2s;
            user-select: none;
            background: transparent;
            border: none;
        }}
        .vis-btn.active {{
            opacity: 1;
        }}
        .vis-btn.inactive {{
            opacity: 0.3;
        }}
        .vis-btn:hover {{
            opacity: 0.7;
        }}
        .vis-btn.mic {{
            color: #00c8ff;
        }}
        .vis-btn.trans {{
            color: #ff9633;
        }}
        select {{
            font-family: 'Google Sans Flex', sans-serif;
            font-variation-settings: 'wght' 600, 'ROND' 100;
            background: rgba(30, 30, 30, 0.9);
            color: #ccc;
            border: 1px solid rgba(255,255,255,0.15);
            border-radius: 50%;
            padding: 0;
            font-size: 10px;
            font-weight: bold;
            cursor: pointer;
            outline: none;
            width: 26px;
            height: 26px;
            scrollbar-width: thin;
            scrollbar-color: #555 #2a2a2a;
            transition: all 0.2s;
            -webkit-appearance: none;
            -moz-appearance: none;
            appearance: none;
            text-align: center;
            text-align-last: center;
        }}
        select:hover {{
            border-color: {glow_color};
            box-shadow: 0 0 6px {glow_color}30;
        }}
        select option {{
            font-family: 'Google Sans Flex', sans-serif;
            background: #2a2a2a;
            color: #ccc;
            padding: 4px 8px;
        }}
        select option:checked {{
            background: linear-gradient(0deg, {glow_color}40, {glow_color}40);
        }}
        /* Custom scrollbar for WebKit browsers */
        select::-webkit-scrollbar {{
            width: 8px;
        }}
        select::-webkit-scrollbar-track {{
            background: #2a2a2a;
            border-radius: 4px;
        }}
        select::-webkit-scrollbar-thumb {{
            background: #555;
            border-radius: 4px;
        }}
        select::-webkit-scrollbar-thumb:hover {{
            background: #777;
        }}
        #viewport {{
            flex: 1;
            overflow: hidden;
            position: relative;
        }}
        #content {{
            font-size: {font_size}px;
            line-height: 1.5;
            padding-bottom: 5px;
        }}
        @keyframes wipe-in {{
            from {{
                -webkit-mask-position: 100% 0;
                mask-position: 100% 0;
                transform: translateX(-4px);
                opacity: 0;
                filter: blur(2px);
            }}
            to {{
                -webkit-mask-position: 0% 0;
                mask-position: 0% 0;
                transform: translateX(0);
                opacity: 1;
                filter: blur(0);
            }}
        }}

        /* Base styling for all text chunks */
        .text-chunk {{
            font-family: 'Google Sans Flex', sans-serif !important;
            font-optical-sizing: auto;
            display: inline;
            transition: 
                color 0.6s cubic-bezier(0.2, 0, 0.2, 1),
                font-variation-settings 0.6s cubic-bezier(0.2, 0, 0.2, 1),
                -webkit-mask-position 0.35s cubic-bezier(0.2, 0, 0.2, 1),
                mask-position 0.35s cubic-bezier(0.2, 0, 0.2, 1),
                opacity 0.35s ease-out,
                filter 0.35s ease-out;
        }}
        
        /* Old/committed text styling */
        .text-chunk.old {{
            color: #9aa0a6;
            font-variation-settings: 'wght' 300, 'wdth' 100, 'slnt' 0, 'GRAD' 0, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
        }}
        
        /* New/uncommitted text styling */
        .text-chunk.new {{
            color: #ffffff;
            font-variation-settings: 'wght' 400, 'wdth' 98, 'slnt' 0, 'GRAD' 150, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
        }}
        
        /* Appearing state - wipe animation */
        .text-chunk.appearing {{
            color: #ffffff;
            font-variation-settings: 'wght' 400, 'wdth' 98, 'slnt' 0, 'GRAD' 150, 'ROND' 100, 'ROUN' 100, 'RNDS' 100;
            
            -webkit-mask-image: linear-gradient(to right, black 50%, transparent 100%);
            mask-image: linear-gradient(to right, black 50%, transparent 100%);
            -webkit-mask-size: 200% 100%;
            mask-size: 200% 100%;
            -webkit-mask-position: 100% 0;
            mask-position: 100% 0;
            opacity: 0;
            filter: blur(2px);
        }}
        
        /* Appearing -> visible */
        .text-chunk.appearing.show {{
            -webkit-mask-position: 0% 0;
            mask-position: 0% 0;
            opacity: 1;
            filter: blur(0);
        }}
        .placeholder {{
            color: #666;
            font-style: italic;
        }}
        /* Resize handle - visible grip in corner */
        #resize-hint {{
            position: absolute;
            bottom: 0;
            right: 0;
            width: 16px;
            height: 16px;
            cursor: se-resize;
            opacity: 0.5;
            display: flex;
            align-items: flex-end;
            justify-content: flex-end;
            padding: 2px;
            font-size: 10px;
            color: #888;
            user-select: none;
        }}
        #resize-hint:hover {{
            opacity: 1;
            color: {glow_color};
        }}
        .audio-icon {{
            font-size: 16px;
            padding: 2px;
            cursor: pointer;
            color: #555;
            transition: all 0.2s;
            background: transparent;
            border: none;
        }}
        .audio-icon:hover {{
            color: #aaa;
        }}
        .audio-icon.active {{
            color: #00c8ff;
        }}
        .model-icon {{
            font-size: 16px;
            padding: 2px;
            cursor: pointer;
            color: #555;
            transition: all 0.2s;
            background: transparent;
            border: none;
        }}
        .model-icon:hover {{
            color: #aaa;
        }}
        .model-icon.active {{
            color: #ff9633;
        }}
        @keyframes model-switch-pulse {{
            0% {{ transform: scale(1); box-shadow: 0 0 0 0 rgba(255,150,51,0.7); }}
            25% {{ transform: scale(1.3); box-shadow: 0 0 15px 5px rgba(255,150,51,0.5); }}
            50% {{ transform: scale(1.1); box-shadow: 0 0 10px 3px rgba(255,150,51,0.3); }}
            75% {{ transform: scale(1.2); box-shadow: 0 0 12px 4px rgba(255,150,51,0.4); }}
            100% {{ transform: scale(1); box-shadow: 0 0 0 0 rgba(255,150,51,0); }}
        }}
        .model-icon.switching {{
            animation: model-switch-pulse 2s ease-out;
            color: #ff9633 !important;
            background: rgba(255,150,51,0.3) !important;
        }}
    </style>
</head>
<body>
    <div id="loading-overlay">{loading_icon}</div>
    <div id="container">
        <div id="header">
            <div id="title">{title_content}</div>
            <div id="controls">
                {audio_selector}
                <div class="btn-group">
                    <span class="ctrl-btn" id="font-decrease" title="Decrease font size"><span class="material-symbols-rounded">remove</span></span>
                    <span class="ctrl-btn" id="font-increase" title="Increase font size"><span class="material-symbols-rounded">add</span></span>
                </div>
                <div class="btn-group">
                    <span class="vis-btn mic active" id="toggle-mic" title="Toggle Transcription"><span class="material-symbols-rounded">subtitles</span></span>
                    <span class="vis-btn trans active" id="toggle-trans" title="Toggle Translation"><span class="material-symbols-rounded">translate</span></span>
                </div>
            </div>
        </div>
        <div id="header-toggle" title="Toggle header"><span class="material-symbols-rounded">expand_less</span></div>
        <div id="viewport">
            <div id="content">
                <span class="placeholder">{placeholder_text}</span>
            </div>
        </div>
        <div id="resize-hint"><span class="material-symbols-rounded" style="font-size: 14px;">south_east</span></div>
    </div>
    <script>
        const container = document.getElementById('container');
        const viewport = document.getElementById('viewport');
        const content = document.getElementById('content');
        const header = document.getElementById('header');
        const headerToggle = document.getElementById('header-toggle');
        const toggleMic = document.getElementById('toggle-mic');
        const toggleTrans = document.getElementById('toggle-trans');
        const fontDecrease = document.getElementById('font-decrease');
        const fontIncrease = document.getElementById('font-increase');
        const resizeHint = document.getElementById('resize-hint');
        
        let currentFontSize = {font_size};
        let isResizing = false;
        let resizeStartX = 0;
        let resizeStartY = 0;
        let micVisible = true;
        let transVisible = true;
        let headerCollapsed = false;
        
        // Header toggle (with null check in case element is commented out)
        if (headerToggle) {{
            headerToggle.addEventListener('click', function(e) {{
                e.stopPropagation();
                headerCollapsed = !headerCollapsed;
                header.classList.toggle('collapsed', headerCollapsed);
                headerToggle.classList.toggle('collapsed', headerCollapsed);
            }});
        }}
        
        // Drag support
        container.addEventListener('mousedown', function(e) {{
            if (e.target.closest('#controls') || e.target.closest('#header-toggle') || e.target.id === 'resize-hint' || isResizing) return;
            window.ipc.postMessage('startDrag');
        }});
        
        // Resize support
        resizeHint.addEventListener('mousedown', function(e) {{
            e.stopPropagation();
            e.preventDefault();
            isResizing = true;
            resizeStartX = e.screenX;
            resizeStartY = e.screenY;
            document.addEventListener('mousemove', onResizeMove);
            document.addEventListener('mouseup', onResizeEnd);
        }});
        
        function onResizeMove(e) {{
            if (!isResizing) return;
            const dx = e.screenX - resizeStartX;
            const dy = e.screenY - resizeStartY;
            if (Math.abs(dx) > 5 || Math.abs(dy) > 5) {{
                window.ipc.postMessage('resize:' + dx + ',' + dy);
                resizeStartX = e.screenX;
                resizeStartY = e.screenY;
            }}
        }}
        
        function onResizeEnd(e) {{
            isResizing = false;
            document.removeEventListener('mousemove', onResizeMove);
            document.removeEventListener('mouseup', onResizeEnd);
            window.ipc.postMessage('saveResize');
        }}
        
        // Visibility toggle buttons
        toggleMic.addEventListener('click', function(e) {{
            e.stopPropagation();
            micVisible = !micVisible;
            this.classList.toggle('active', micVisible);
            this.classList.toggle('inactive', !micVisible);
            window.ipc.postMessage('toggleMic:' + (micVisible ? '1' : '0'));
        }});
        
        toggleTrans.addEventListener('click', function(e) {{
            e.stopPropagation();
            transVisible = !transVisible;
            this.classList.toggle('active', transVisible);
            this.classList.toggle('inactive', !transVisible);
            window.ipc.postMessage('toggleTrans:' + (transVisible ? '1' : '0'));
        }});
        
        // Function to update visibility state from native side
        window.setVisibility = function(mic, trans) {{
            micVisible = mic;
            transVisible = trans;
            toggleMic.classList.toggle('active', mic);
            toggleMic.classList.toggle('inactive', !mic);
            toggleTrans.classList.toggle('active', trans);
            toggleTrans.classList.toggle('inactive', !trans);
        }};
        
        // Font size controls
        fontDecrease.addEventListener('click', function(e) {{
            e.stopPropagation();
            if (currentFontSize > 10) {{
                currentFontSize -= 2;
                content.style.fontSize = currentFontSize + 'px';
                // Reset min height so text can shrink properly
                minContentHeight = 0;
                content.style.minHeight = '';
                window.ipc.postMessage('fontSize:' + currentFontSize);
            }}
        }});
        
        fontIncrease.addEventListener('click', function(e) {{
            e.stopPropagation();
            if (currentFontSize < 32) {{
                currentFontSize += 2;
                content.style.fontSize = currentFontSize + 'px';
                // Reset min height for fresh calculation
                minContentHeight = 0;
                content.style.minHeight = '';
                window.ipc.postMessage('fontSize:' + currentFontSize);
            }}
        }});
        
        // Audio Toggle Switch Logic - query all audio icons directly
        const audioIcons = document.querySelectorAll('.audio-icon');
        if (audioIcons.length) {{
            audioIcons.forEach(icon => {{
                icon.addEventListener('click', (e) => {{
                    e.stopPropagation();
                    e.preventDefault();
                    
                    // Update UI - toggle active class
                    audioIcons.forEach(i => i.classList.remove('active'));
                    icon.classList.add('active');
                    
                    // Send IPC
                    const val = icon.getAttribute('data-value');
                    window.ipc.postMessage('audioSource:' + val);
                }});
            }});
        }}

        // Language Select Logic - show short code when collapsed, full name when open
        const langSelect = document.getElementById('language-select');
        if (langSelect) {{
            // Store original full names
            const options = langSelect.querySelectorAll('option');
            options.forEach(opt => {{
                opt.dataset.fullname = opt.textContent;
            }});
            
            // Function to show short codes (when collapsed)
            function showCodes() {{
                options.forEach(opt => {{
                    opt.textContent = opt.dataset.code || opt.dataset.fullname.substring(0, 2).toUpperCase();
                }});
            }}
            
            // Function to show full names (when dropdown open)
            function showFullNames() {{
                options.forEach(opt => {{
                    opt.textContent = opt.dataset.fullname;
                }});
            }}
            
            // Initially show codes
            showCodes();
            
            // Show full names when dropdown opens
            langSelect.addEventListener('focus', showFullNames);
            langSelect.addEventListener('mousedown', function(e) {{ 
                e.stopPropagation();
                showFullNames();
            }});
            
            // Show codes when dropdown closes
            langSelect.addEventListener('blur', showCodes);
            langSelect.addEventListener('change', function(e) {{
                e.stopPropagation();
                window.ipc.postMessage('language:' + this.value);
                // Delay to let the dropdown close animation finish
                setTimeout(showCodes, 100);
            }});
        }}

        // Model Toggle Switch Logic - query all model icons directly
        const modelIcons = document.querySelectorAll('.model-icon');
        if (modelIcons.length) {{
            modelIcons.forEach(icon => {{
                icon.addEventListener('click', (e) => {{
                    e.stopPropagation();
                    e.preventDefault();
                    
                    // Update UI - toggle active class
                    modelIcons.forEach(i => i.classList.remove('active'));
                    icon.classList.add('active');
                    
                    // Send IPC
                    const val = icon.getAttribute('data-value');
                    window.ipc.postMessage('translationModel:' + val);
                }});
            }});
        }}
        
        // Handle resize to keep text at bottom
        let lastWidth = viewport.clientWidth;
        const resizeObserver = new ResizeObserver(entries => {{
            for (let entry of entries) {{
                if (Math.abs(entry.contentRect.width - lastWidth) > 5) {{
                    lastWidth = entry.contentRect.width;
                    // Reset min height on width change (reflow)
                    minContentHeight = 0;
                    content.style.minHeight = '';
                    
                    // Force scroll to bottom immediately to prevent jump
                    if (content.scrollHeight > viewport.clientHeight) {{
                        viewport.scrollTop = content.scrollHeight - viewport.clientHeight;
                    }}
                    targetScrollTop = viewport.scrollTop;
                    currentScrollTop = targetScrollTop;
                }}
            }}
        }});
        resizeObserver.observe(viewport);
        
        let isFirstText = true;
        let currentScrollTop = 0;
        let targetScrollTop = 0;
        let animationFrame = null;
        let minContentHeight = 0;
        
        function animateScroll() {{
            const diff = targetScrollTop - currentScrollTop;
            
            if (Math.abs(diff) > 0.5) {{
                const ease = Math.min(0.08, Math.max(0.02, Math.abs(diff) / 1000));
                currentScrollTop += diff * ease;
                viewport.scrollTop = currentScrollTop;
                animationFrame = requestAnimationFrame(animateScroll);
            }} else {{
                currentScrollTop = targetScrollTop;
                viewport.scrollTop = currentScrollTop;
                animationFrame = null;
            }}
        }}
        
        let currentOldTextLength = 0;

        function updateText(oldText, newText) {{
            const hasContent = oldText || newText;
            
            if (isFirstText && hasContent) {{
                content.innerHTML = '';
                isFirstText = false;
                minContentHeight = 0;
                currentOldTextLength = 0;
            }}
            
            if (!hasContent) {{
                content.innerHTML = '<span class="placeholder">{placeholder_text}</span>';
                content.style.minHeight = '';
                isFirstText = true;
                minContentHeight = 0;
                targetScrollTop = 0;
                currentScrollTop = 0;
                viewport.scrollTop = 0;
                currentOldTextLength = 0;
                return;
            }}

            // 1. Handle history rewrite or shrink
            if (oldText.length < currentOldTextLength) {{
                content.innerHTML = '';
                currentOldTextLength = 0;
            }}
            
            // Get all existing chunks
            const allChunks = Array.from(content.querySelectorAll('.text-chunk'));
            let totalChunkText = allChunks.map(c => c.textContent).join('');
            const fullText = oldText + newText;
            
            // 2. If old text grew, transition chunks from new to old
            if (oldText.length > currentOldTextLength) {{
                let committedLen = oldText.length;
                let accumulatedLen = 0;
                
                for (const chunk of allChunks) {{
                    const chunkLen = chunk.textContent.length;
                    const chunkEnd = accumulatedLen + chunkLen;
                    
                    // If this chunk falls within committed range, mark as old
                    if (chunkEnd <= committedLen) {{
                        if (!chunk.classList.contains('old')) {{
                            chunk.classList.remove('appearing', 'new');
                            chunk.classList.add('old');
                        }}
                    }}
                    accumulatedLen = chunkEnd;
                }}
            }}
            currentOldTextLength = oldText.length;
            
            // 3. Handle new text growth - create appearing chunks
            if (fullText.length > totalChunkText.length && fullText.startsWith(totalChunkText)) {{
                const delta = fullText.substring(totalChunkText.length);
                
                const chunk = document.createElement('span');
                chunk.className = 'text-chunk appearing';
                chunk.textContent = delta;
                content.appendChild(chunk);
                
                // Trigger wipe animation
                requestAnimationFrame(() => {{
                    chunk.classList.add('show');
                    // After wipe completes, transition to proper state
                    setTimeout(() => {{
                        chunk.classList.remove('appearing', 'show');
                        // Check if this chunk is now committed or still new
                        const chunkStart = totalChunkText.length;
                        if (chunkStart < currentOldTextLength) {{
                            chunk.classList.add('old');
                        }} else {{
                            chunk.classList.add('new');
                        }}
                    }}, 350); // Match wipe animation duration
                }});
            }} else if (fullText !== totalChunkText) {{
                // Text was revised - rebuild (rare case)
                content.innerHTML = '';
                if (oldText) {{
                    const oldChunk = document.createElement('span');
                    oldChunk.className = 'text-chunk old';
                    oldChunk.textContent = oldText;
                    content.appendChild(oldChunk);
                }}
                if (newText) {{
                    const newChunk = document.createElement('span');
                    newChunk.className = 'text-chunk new';
                    newChunk.textContent = newText;
                    content.appendChild(newChunk);
                }}
            }}
            
            // Scroll logic
            const naturalHeight = content.offsetHeight;
            if (naturalHeight > minContentHeight) {{
                minContentHeight = naturalHeight;
            }}
            content.style.minHeight = minContentHeight + 'px';
            const viewportHeight = viewport.offsetHeight;
            if (minContentHeight > viewportHeight) {{
                const maxScroll = minContentHeight - viewportHeight;
                if (maxScroll > targetScrollTop) {{
                    targetScrollTop = maxScroll;
                }}
            }}
            if (!animationFrame) {{
                animationFrame = requestAnimationFrame(animateScroll);
            }}
        }}

        window.updateText = updateText;
        
        // Volume visualizer with history buffer (like recording.rs)
        // 30 samples at 100ms = 3 seconds of history
        const volumeHistory = new Array(30).fill(0);
        let historyHead = 0;
        
        function updateVolume(rms) {{
            const bars = document.querySelectorAll('.volume-bar');
            if (!bars.length) return;
            
            // Add new RMS to history buffer (circular)
            volumeHistory[historyHead] = rms;
            historyHead = (historyHead + 1) % 30;
            
            // Draw bars from history (newest on right, oldest on left)
            bars.forEach((bar, i) => {{
                // Get historical value, flowing from left to right
                const histIdx = (historyHead + i) % 30;
                const amp = volumeHistory[histIdx];
                
                // Scale RMS (0-0.3) to visual height (2-16px)
                const height = Math.max(2, Math.min(16, amp * 120));
                bar.style.height = height + 'px';
            }});
        }}
        
        window.updateVolume = updateVolume;
        
        // Model switch animation (called when 429 fallback switches models)
        function switchModel(modelName) {{
            const icons = document.querySelectorAll('.model-icon');
            if (!icons.length) return;
            
            icons.forEach(icon => {{
                const val = icon.getAttribute('data-value');
                const shouldBeActive = val === modelName;
                
                // Update active state
                icon.classList.remove('active');
                if (shouldBeActive) {{
                    icon.classList.add('active');
                    // Add switching animation
                    icon.classList.add('switching');
                    // Remove animation class after it completes (2s)
                    setTimeout(() => icon.classList.remove('switching'), 2000);
                }}
            }});
        }}
        
        window.switchModel = switchModel;
    </script>
</body>
</html>"#,
        glow_color = glow_color,
        title_content = title_content,
        audio_selector = audio_selector,
        placeholder_text = placeholder_text,
        font_size = font_size,
        loading_icon = loading_icon
    )
}

pub fn is_realtime_overlay_active() -> bool {
    unsafe { IS_ACTIVE && REALTIME_HWND.0 != 0 }
}

/// Stop the realtime overlay and close all windows
pub fn stop_realtime_overlay() {
    unsafe {
        if REALTIME_HWND.0 != 0 {
            let _ = PostMessageW(REALTIME_HWND, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn show_realtime_overlay(preset_idx: usize) {
    unsafe {
        if IS_ACTIVE { return; }
        
        let mut preset = APP.lock().unwrap().config.presets[preset_idx].clone();
        

        
        // Reset state
        IS_ACTIVE = true;
        REALTIME_STOP_SIGNAL.store(false, Ordering::SeqCst);
        
        // Reset visibility flags
        MIC_VISIBLE.store(true, Ordering::SeqCst);
        TRANS_VISIBLE.store(true, Ordering::SeqCst);
        
        // Reset change signals
        AUDIO_SOURCE_CHANGE.store(false, Ordering::SeqCst);
        LANGUAGE_CHANGE.store(false, Ordering::SeqCst);
        TRANSLATION_MODEL_CHANGE.store(false, Ordering::SeqCst);
        
        // Reset translation state
        {
            let mut state = REALTIME_STATE.lock().unwrap();
            *state = RealtimeState::new();
        }
        
        let instance = GetModuleHandleW(None).unwrap();
        
        // --- Create Main Realtime Overlay ---
        let class_name = w!("RealtimeWebViewOverlay");
        REGISTER_REALTIME_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(realtime_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(0);
            let _ = RegisterClassW(&wc);
        });
        
        // Fetch config
        let (font_size, config_audio_source, config_language, config_translation_model, trans_size, transcription_size) = {
            let app = APP.lock().unwrap();
            (
                app.config.realtime_font_size,
                app.config.realtime_audio_source.clone(),
                app.config.realtime_target_language.clone(),
                app.config.realtime_translation_model.clone(),
                app.config.realtime_translation_size,
                app.config.realtime_transcription_size
            )
        };
        
        // IMPORTANT: Override preset.audio_source with saved config value
        // This ensures the transcription engine uses the user's saved preference
        if !config_audio_source.is_empty() {
            preset.audio_source = config_audio_source.clone();
        }
        
    let target_language = if !config_language.is_empty() {
        config_language
    } else if preset.blocks.len() > 1 {
        // Get from translation block
        let trans_block = &preset.blocks[1];
        if !trans_block.selected_language.is_empty() {
            trans_block.selected_language.clone()
        } else {
            trans_block.language_vars.get("language").cloned()
                .or_else(|| trans_block.language_vars.get("language1").cloned())
                .unwrap_or_else(|| "English".to_string())
        }
    } else {
        "English".to_string()
    };
        
        // Initialize NEW_TARGET_LANGUAGE so translation loop uses saved language from start
        if !target_language.is_empty() {
            if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
                *new_lang = target_language.clone();
            }
            // Trigger a language "change" so translation loop picks it up immediately
            LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
        }
        
        // Calculate positions
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        
        let has_translation = preset.blocks.len() > 1;
        
        // Use configured sizes
        let main_w = transcription_size.0;
        let main_h = transcription_size.1;
        let trans_w = trans_size.0;
        let trans_h = trans_size.1;
        
        let (main_x, main_y) = if has_translation {
            let total_w = main_w + trans_w + GAP;
            ((screen_w - total_w) / 2, (screen_h - main_h) / 2)
        } else {
            ((screen_w - main_w) / 2, (screen_h - main_h) / 2)
        };
        
        // Create popup window with resize support
        let main_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Realtime Transcription"),
            WS_POPUP | WS_VISIBLE,
            main_x, main_y, main_w, main_h,
            None, None, instance, None
        );
        
        // Enable rounded corners (Windows 11+)
        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            main_hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&corner_pref) as u32,
        );
        
        REALTIME_HWND = main_hwnd;
        
        // Create WebView for main overlay
        create_realtime_webview(main_hwnd, false, &config_audio_source, &target_language, &config_translation_model, font_size);
        
        // --- Create Translation Overlay if needed ---
        let translation_hwnd = if has_translation {
            let trans_class = w!("RealtimeTranslationWebViewOverlay");
            REGISTER_TRANSLATION_CLASS.call_once(|| {
                let mut wc = WNDCLASSW::default();
                wc.lpfnWndProc = Some(translation_wnd_proc);
                wc.hInstance = instance;
                wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
                wc.lpszClassName = trans_class;
                wc.style = CS_HREDRAW | CS_VREDRAW;
                wc.hbrBackground = HBRUSH(0);
                let _ = RegisterClassW(&wc);
            });
            
            let trans_x = main_x + main_w + GAP;
            let trans_hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                trans_class,
                w!("Translation"),
                WS_POPUP | WS_VISIBLE,
                trans_x, main_y, trans_w, trans_h,
                None, None, instance, None
            );
            
            // Enable rounded corners (Windows 11+)
            let corner_pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                trans_hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&corner_pref) as u32,
            );
            
            TRANSLATION_HWND = trans_hwnd;
            create_realtime_webview(trans_hwnd, true, "mic", &target_language, &config_translation_model, font_size);
            
            Some(trans_hwnd)
        } else {
            TRANSLATION_HWND = HWND(0);
            None
        };
        
        // Start realtime transcription
        start_realtime_transcription(
            preset,
            REALTIME_STOP_SIGNAL.clone(),
            main_hwnd,
            translation_hwnd,
            REALTIME_STATE.clone(),
        );
        
        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_QUIT { break; }
        }
        
        // Cleanup
        destroy_realtime_webview(REALTIME_HWND);
        if TRANSLATION_HWND.0 != 0 {
            destroy_realtime_webview(TRANSLATION_HWND);
        }
        
        IS_ACTIVE = false;
        REALTIME_HWND = HWND(0);
        TRANSLATION_HWND = HWND(0);
    }
}



fn create_realtime_webview(hwnd: HWND, is_translation: bool, audio_source: &str, current_language: &str, translation_model: &str, font_size: u32) {
    let hwnd_key = hwnd.0 as isize;
    
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect); }
    
    // Use full language list from isolang crate
    let languages = get_all_languages();
    
    // Fetch locale text
    let locale_text = {
        let app = APP.lock().unwrap();
        let lang = app.config.ui_language.clone();
        LocaleText::get(&lang)
    };
    
    let html = get_realtime_html(is_translation, audio_source, &languages, current_language, translation_model, font_size, &locale_text);
    let wrapper = HwndWrapper(hwnd);
    
    // Capture hwnd for the IPC handler closure
    let hwnd_for_ipc = hwnd;
    
    let result = WebViewBuilder::new()
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32
            )),
        })
        .with_html(&html)
        .with_transparent(false)
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            let body = msg.body();
            if body == "startDrag" {
                // Initiate window drag
                unsafe {
                    let _ = ReleaseCapture();
                    SendMessageW(
                        hwnd_for_ipc,
                        WM_NCLBUTTONDOWN,
                        WPARAM(HTCAPTION as usize),
                        LPARAM(0)
                    );
                }
            } else if body == "close" {
                unsafe {
                    let _ = PostMessageW(
                        hwnd_for_ipc,
                        WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0)
                    );
                }
            } else if body == "saveResize" {
                unsafe {
                    let mut rect = RECT::default();
                    GetWindowRect(hwnd_for_ipc, &mut rect);
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;
                    
                    let mut app = APP.lock().unwrap();
                    if hwnd_for_ipc == REALTIME_HWND {
                        app.config.realtime_transcription_size = (w, h);
                    } else {
                        app.config.realtime_translation_size = (w, h);
                    }
                    crate::config::save_config(&app.config);
                }
            } else if body.starts_with("fontSize:") {
                // Font size change - store for future use
                if let Ok(size) = body[9..].parse::<u32>() {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_font_size = size;
                    crate::config::save_config(&app.config);
                }
            } else if body.starts_with("audioSource:") {
                // Audio source change - signal restart with new source
                let source = body[12..].to_string();
                if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
                    *new_source = source.clone();
                }
                
                // Save to config
                {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_audio_source = source;
                    crate::config::save_config(&app.config);
                }
                AUDIO_SOURCE_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("language:") {
                // Target language change - signal update
                let lang = body[9..].to_string();
                if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
                    *new_lang = lang.clone();
                }
                
                // Save to config
                {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_target_language = lang;
                    crate::config::save_config(&app.config);
                }
                LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("translationModel:") {
                // Translation model change - signal update
                let model = body[17..].to_string();
                if let Ok(mut new_model) = NEW_TRANSLATION_MODEL.lock() {
                    *new_model = model.clone();
                }
                
                // Save to config
                {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_translation_model = model;
                    crate::config::save_config(&app.config);
                }
                TRANSLATION_MODEL_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("resize:") {
                // Resize window by delta
                let coords = &body[7..];
                if let Some((dx_str, dy_str)) = coords.split_once(',') {
                    if let (Ok(dx), Ok(dy)) = (dx_str.parse::<i32>(), dy_str.parse::<i32>()) {
                        unsafe {
                            let mut rect = RECT::default();
                            GetWindowRect(hwnd_for_ipc, &mut rect);
                            let new_width = (rect.right - rect.left + dx).max(200);
                            let new_height = (rect.bottom - rect.top + dy).max(100);
                            SetWindowPos(
                                hwnd_for_ipc,
                                None,
                                rect.left,
                                rect.top,
                                new_width,
                                new_height,
                                SWP_NOZORDER | SWP_NOACTIVATE
                            );
                        }
                    }
                }
            } else if body.starts_with("toggleMic:") {
                // Toggle transcription window visibility
                let visible = &body[10..] == "1";
                MIC_VISIBLE.store(visible, Ordering::SeqCst);
                unsafe {
                    if REALTIME_HWND.0 != 0 {
                        ShowWindow(REALTIME_HWND, if visible { SW_SHOW } else { SW_HIDE });
                    }
                    // Sync to other webview
                    sync_visibility_to_webviews();
                    
                    // If both windows are now off, completely stop everything
                    if !MIC_VISIBLE.load(Ordering::SeqCst) && !TRANS_VISIBLE.load(Ordering::SeqCst) {
                        REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
                        PostQuitMessage(0);
                    } else if visible {
                        // Force update since we suppressed them while hidden
                        let _ = PostMessageW(REALTIME_HWND, WM_REALTIME_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            } else if body.starts_with("toggleTrans:") {
                // Toggle translation window visibility
                let visible = &body[12..] == "1";
                TRANS_VISIBLE.store(visible, Ordering::SeqCst);
                unsafe {
                    if TRANSLATION_HWND.0 != 0 {
                        ShowWindow(TRANSLATION_HWND, if visible { SW_SHOW } else { SW_HIDE });
                    }
                    // Sync to other webview
                    sync_visibility_to_webviews();
                    
                    // If both windows are now off, completely stop everything
                    if !MIC_VISIBLE.load(Ordering::SeqCst) && !TRANS_VISIBLE.load(Ordering::SeqCst) {
                        REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
                        PostQuitMessage(0);
                    } else if visible {
                        // Force update since we suppressed them while hidden
                        let _ = PostMessageW(TRANSLATION_HWND, WM_TRANSLATION_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            }
        })
        .build_as_child(&wrapper);
    
    if let Ok(webview) = result {
        REALTIME_WEBVIEWS.with(|wvs| {
            wvs.borrow_mut().insert(hwnd_key, webview);
        });
    }
}

fn destroy_realtime_webview(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;
    REALTIME_WEBVIEWS.with(|wvs| {
        wvs.borrow_mut().remove(&hwnd_key);
    });
}

/// Sync visibility toggle state to all webviews
fn sync_visibility_to_webviews() {
    let mic_vis = MIC_VISIBLE.load(Ordering::SeqCst);
    let trans_vis = TRANS_VISIBLE.load(Ordering::SeqCst);
    let script = format!("if(window.setVisibility) window.setVisibility({}, {});", mic_vis, trans_vis);
    
    REALTIME_WEBVIEWS.with(|wvs| {
        for webview in wvs.borrow().values() {
            let _ = webview.evaluate_script(&script);
        }
    });
}

fn update_webview_text(hwnd: HWND, old_text: &str, new_text: &str) {
    let hwnd_key = hwnd.0 as isize;
    
    // Escape the text for JavaScript
    fn escape_js(text: &str) -> String {
        text.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "")
    }
    
    let escaped_old = escape_js(old_text);
    let escaped_new = escape_js(new_text);
    
    let script = format!("window.updateText('{}', '{}');", escaped_old, escaped_new);
    
    REALTIME_WEBVIEWS.with(|wvs| {
        if let Some(webview) = wvs.borrow().get(&hwnd_key) {
            let _ = webview.evaluate_script(&script);
        }
    });
}

unsafe extern "system" fn realtime_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_REALTIME_UPDATE => {
            // Get old (committed) and new (current sentence) text from state
            let (old_text, new_text) = {
                if let Ok(state) = REALTIME_STATE.lock() {
                    // Everything before last_committed_pos is "old"
                    // Everything after is "new" (current sentence)
                    let full = &state.full_transcript;
                    let pos = state.last_committed_pos.min(full.len());
                    let old_raw = &full[..pos];
                    let new_raw = &full[pos..];
                    
                    let old = old_raw.trim_end();
                    let new = new_raw.trim_start();
                    if !old.is_empty() && !new.is_empty() {
                        (old.to_string(), format!(" {}", new))
                    } else {
                        (old.to_string(), new.to_string())
                    }
                } else {
                    (String::new(), String::new())
                }
            };
            update_webview_text(hwnd, &old_text, &new_text);
            LRESULT(0)
        }
        WM_VOLUME_UPDATE => {
            // Read RMS from shared atomic and update visualizer
            let rms_bits = REALTIME_RMS.load(Ordering::Relaxed);
            let rms = f32::from_bits(rms_bits);
            
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.updateVolume) window.updateVolume({});", rms);
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_SIZE => {
            // Resize WebView to match window size
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            let hwnd_key = hwnd.0 as isize;
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width, height)),
                    });
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
            DestroyWindow(hwnd);
            
            if TRANSLATION_HWND.0 != 0 {
                DestroyWindow(TRANSLATION_HWND);
            }
            
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn translation_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRANSLATION_UPDATE => {
            // Get old (committed) and new (uncommitted) translation from state
            let (old_text, new_text) = {
                if let Ok(state) = REALTIME_STATE.lock() {
                    let old = state.committed_translation.trim_end();
                    let new = state.uncommitted_translation.trim_start();
                    if !old.is_empty() && !new.is_empty() {
                        (old.to_string(), format!(" {}", new))
                    } else {
                        (old.to_string(), new.to_string())
                    }
                } else {
                    (String::new(), String::new())
                }
            };
            update_webview_text(hwnd, &old_text, &new_text);
            LRESULT(0)
        }
        WM_MODEL_SWITCH => {
            // Animate the model switch in the UI
            // WPARAM: 0 = groq-llama, 1 = google-gemma, 2 = google-gtx
            let model_name = match wparam.0 {
                1 => "google-gemma",
                2 => "google-gtx",
                _ => "groq-llama"
            };
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.switchModel) window.switchModel('{}');", model_name);
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_SIZE => {
            // Resize WebView to match window size
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            let hwnd_key = hwnd.0 as isize;
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width, height)),
                    });
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
            DestroyWindow(hwnd);
            
            if REALTIME_HWND.0 != 0 {
                DestroyWindow(REALTIME_HWND);
            }
            
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

