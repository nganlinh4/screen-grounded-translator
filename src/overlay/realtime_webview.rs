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

// Window dimensions
const OVERLAY_WIDTH: i32 = 500;
const OVERLAY_HEIGHT: i32 = 180;
const TRANSLATION_WIDTH: i32 = 500;
const TRANSLATION_HEIGHT: i32 = 180;
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
    let title_icon = if is_translation { "translate" } else { "graphic_eq" };
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
    
    let mic_text = text.realtime_mic;
    let device_text = text.realtime_device;
    let placeholder_text = text.realtime_waiting;
    
    // Build language options HTML
    let lang_options: String = languages.iter()
        .map(|lang| {
            let selected = if lang == current_language { "selected" } else { "" };
            format!(r#"<option value="{}" {}>{}</option>"#, lang, selected, lang)
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    // Audio source selector (only for transcription window) - simple toggle switch
    let audio_selector = if !is_translation {
        let is_device = audio_source == "device";
        format!(r#"
            <div class="audio-toggle" id="audio-toggle" title="Toggle audio source">
                <span class="material-symbols-rounded audio-icon {mic_active}" data-value="mic">mic</span>
                <span class="material-symbols-rounded audio-icon {device_active}" data-value="device">speaker_group</span>
            </div>
        "#, 
            mic_active = if !is_device { "active" } else { "" },
            device_active = if is_device { "active" } else { "" }
        )
    } else {
        // Language selector and model toggle for translation window
        let is_gemma = translation_model == "google-gemma";
        format!(r#"
            <div class="model-toggle" id="model-toggle">
                <span class="material-symbols-rounded model-icon {gemma_active}" data-value="google-gemma" title="AI Translation (Gemma)">auto_awesome</span>
                <span class="material-symbols-rounded model-icon {fallback_active}" data-value="groq-llama" title="Fast Translation (Groq)">speed</span>
            </div>
            <select id="language-select" title="Target Language">
                {lang_options}
            </select>
        "#,
            lang_options = lang_options,
            gemma_active = if is_gemma { "active" } else { "" },
            fallback_active = if !is_gemma { "active" } else { "" }
        )
    };
    
    format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@24,400,1,0" />
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        html, body {{
            height: 100%;
            overflow: hidden;
            background: rgba(26, 26, 26, 0.95);
            font-family: 'Segoe UI', sans-serif;
            color: #fff;
            border-radius: 12px;
            border: 1px solid {glow_color}40;
            box-shadow: 0 0 20px {glow_color}30;
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
            gap: 6px;
            align-items: center;
            flex: 1;
            justify-content: flex-end;
        }}
        .ctrl-btn {{
            font-size: 14px;
            color: #888;
            cursor: pointer;
            padding: 2px 8px;
            border-radius: 4px;
            background: rgba(255,255,255,0.05);
            border: 1px solid rgba(255,255,255,0.1);
            transition: all 0.2s;
            user-select: none;
        }}
        .ctrl-btn:hover {{
            color: #fff;
            background: rgba(255,255,255,0.15);
        }}
        .vis-toggle {{
            display: flex;
            gap: 2px;
            background: rgba(30,30,30,0.8);
            border-radius: 4px;
            padding: 2px;
        }}
        .vis-btn {{
            font-size: 12px;
            cursor: pointer;
            padding: 2px 5px;
            border-radius: 3px;
            transition: all 0.2s;
            user-select: none;
        }}
        .vis-btn.active {{
            opacity: 1;
        }}
        .vis-btn.inactive {{
            opacity: 0.3;
        }}
        .vis-btn:hover {{
            background: rgba(255,255,255,0.1);
        }}
        .vis-btn.mic {{
            color: #00c8ff;
        }}
        .vis-btn.trans {{
            color: #ff9633;
        }}
        select {{
            background: rgba(40, 40, 40, 0.9);
            color: #ccc;
            border: 1px solid rgba(255,255,255,0.15);
            border-radius: 4px;
            padding: 3px 8px;
            font-size: 11px;
            cursor: pointer;
            outline: none;
            max-width: 120px;
            scrollbar-width: thin;
            scrollbar-color: #555 #2a2a2a;
        }}
        select:hover {{
            border-color: {glow_color};
        }}
        select option {{
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
        .old {{
            color: #888;
        }}
        .new {{
            color: #fff;
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
        .audio-toggle {{
            display: flex;
            gap: 2px;
            background: rgba(30,30,30,0.8);
            border-radius: 4px;
            padding: 2px;
        }}
        .audio-icon {{
            font-size: 16px;
            padding: 4px;
            border-radius: 3px;
            cursor: pointer;
            color: #555;
            transition: all 0.2s;
        }}
        .audio-icon:hover {{
            color: #aaa;
        }}
        .audio-icon.active {{
            color: #00c8ff;
            background: rgba(0,200,255,0.15);
        }}
        .model-toggle {{
            display: flex;
            gap: 2px;
            background: rgba(30,30,30,0.8);
            border-radius: 4px;
            padding: 2px;
        }}
        .model-icon {{
            font-size: 16px;
            padding: 4px;
            border-radius: 3px;
            cursor: pointer;
            color: #555;
            transition: all 0.2s;
        }}
        .model-icon:hover {{
            color: #aaa;
        }}
        .model-icon.active {{
            color: #ff9633;
            background: rgba(255,150,51,0.15);
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
    <div id="container">
        <div id="header">
            <div id="title">{title_content}</div>
            <div id="controls">
                {audio_selector}
                <span class="ctrl-btn" id="font-decrease" title="Decrease font size"><span class="material-symbols-rounded">remove</span></span>
                <span class="ctrl-btn" id="font-increase" title="Increase font size"><span class="material-symbols-rounded">add</span></span>
                <div class="vis-toggle">
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
        
        // Audio Toggle Switch Logic
        const audioToggle = document.getElementById('audio-toggle');
        if (audioToggle) {{
            const icons = audioToggle.querySelectorAll('.audio-icon');
            
            icons.forEach(icon => {{
                icon.addEventListener('click', (e) => {{
                    e.stopPropagation();
                    e.preventDefault();
                    
                    // Update UI - toggle active class
                    icons.forEach(i => i.classList.remove('active'));
                    icon.classList.add('active');
                    
                    // Send IPC
                    const val = icon.getAttribute('data-value');
                    window.ipc.postMessage('audioSource:' + val);
                }});
            }});
        }}

        // Language Select Logic
        const langSelect = document.getElementById('language-select');
        if (langSelect) {{
            langSelect.addEventListener('change', function(e) {{
                e.stopPropagation();
                window.ipc.postMessage('language:' + this.value);
            }});
            langSelect.addEventListener('mousedown', function(e) {{ e.stopPropagation(); }});
        }}

        // Model Toggle Switch Logic
        const modelToggle = document.getElementById('model-toggle');
        if (modelToggle) {{
            const icons = modelToggle.querySelectorAll('.model-icon');
            
            icons.forEach(icon => {{
                icon.addEventListener('click', (e) => {{
                    e.stopPropagation();
                    e.preventDefault();
                    
                    // Update UI - toggle active class
                    icons.forEach(i => i.classList.remove('active'));
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
        
        function escapeHtml(text) {{
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }}
        
        function updateText(oldText, newText) {{
            const hasContent = oldText || newText;
            
            if (isFirstText && hasContent) {{
                content.innerHTML = '';
                isFirstText = false;
                minContentHeight = 0;
            }}
            
            if (!hasContent) {{
                content.innerHTML = '<span class="placeholder">{placeholder_text}</span>';
                content.style.minHeight = '';
                isFirstText = true;
                minContentHeight = 0;
                targetScrollTop = 0;
                currentScrollTop = 0;
                viewport.scrollTop = 0;
                return;
            }}
            
            let html = '';
            if (oldText) {{
                html += '<span class="old">' + escapeHtml(oldText) + '</span>';
                if (newText) html += ' ';
            }}
            if (newText) {{
                html += '<span class="new">' + escapeHtml(newText) + '</span>';
            }}
            content.innerHTML = html;
            
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
        function switchModel(isGemma) {{
            const modelToggle = document.getElementById('model-toggle');
            if (!modelToggle) return;
            
            const icons = modelToggle.querySelectorAll('.model-icon');
            icons.forEach(icon => {{
                const val = icon.getAttribute('data-value');
                const shouldBeActive = (isGemma && val === 'google-gemma') || (!isGemma && val === 'groq-llama');
                
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
</html>"#)
}

pub fn is_realtime_overlay_active() -> bool {
    unsafe { IS_ACTIVE && REALTIME_HWND.0 != 0 }
}

pub fn show_realtime_overlay(preset_idx: usize) {
    unsafe {
        if IS_ACTIVE { return; }
        
        let preset = APP.lock().unwrap().config.presets[preset_idx].clone();
        

        
        // Reset state
        IS_ACTIVE = true;
        REALTIME_STOP_SIGNAL.store(false, Ordering::SeqCst);
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
                    .unwrap_or_else(|| "Vietnamese".to_string())
            }
        } else {
            "Vietnamese".to_string()
        };
        
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
                    let old = &full[..pos];
                    let new = &full[pos..];
                    (old.trim().to_string(), new.trim().to_string())
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
                    (
                        state.committed_translation.clone(),
                        state.uncommitted_translation.clone()
                    )
                } else {
                    (String::new(), String::new())
                }
            };
            update_webview_text(hwnd, &old_text, &new_text);
            LRESULT(0)
        }
        WM_MODEL_SWITCH => {
            // Animate the model switch in the UI
            // WPARAM: 1 = google-gemma, 0 = groq-llama
            let is_gemma = wparam.0 == 1;
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.switchModel) window.switchModel({});", is_gemma);
            
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

