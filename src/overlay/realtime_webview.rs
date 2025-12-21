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
use crate::config::get_all_languages;
use crate::api::realtime_audio::{
    start_realtime_transcription, RealtimeState, SharedRealtimeState,
    WM_REALTIME_UPDATE, WM_TRANSLATION_UPDATE,
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
fn get_realtime_html(is_translation: bool, audio_source: &str, languages: &[String], current_language: &str) -> String {
    let title = if is_translation { "🌐 Translation" } else { "🎤 Listening..." };
    let glow_color = if is_translation { "#ff9633" } else { "#00c8ff" };
    
    // Build language options HTML
    let lang_options: String = languages.iter()
        .map(|lang| {
            let selected = if lang == current_language { "selected" } else { "" };
            format!(r#"<option value="{}" {}>{}</option>"#, lang, selected, lang)
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    // Audio source selector (only for transcription window)
    let audio_selector = if !is_translation {
        let mic_selected = if audio_source == "mic" { "selected" } else { "" };
        let device_selected = if audio_source == "device" { "selected" } else { "" };
        format!(r#"
            <select id="audio-source" title="Audio Source">
                <option value="mic" {}>🎤 Mic</option>
                <option value="device" {}>🔊 Device</option>
            </select>
        "#, mic_selected, device_selected)
    } else {
        // Language selector for translation window
        format!(r#"
            <select id="language-select" title="Target Language">
                {}
            </select>
        "#, lang_options)
    };
    
    format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
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
        #container {{
            display: flex;
            flex-direction: column;
            height: 100%;
            padding: 8px 12px;
            cursor: grab;
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
        }}
        #title {{
            font-size: 12px;
            font-weight: bold;
            color: #aaa;
            flex-shrink: 0;
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
            font-size: 16px;
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
    </style>
</head>
<body>
    <div id="container">
        <div id="header">
            <div id="title">{title}</div>
            <div id="controls">
                {audio_selector}
                <span class="ctrl-btn" id="font-decrease" title="Decrease font size">−</span>
                <span class="ctrl-btn" id="font-increase" title="Increase font size">+</span>
                <div class="vis-toggle">
                    <span class="vis-btn mic active" id="toggle-mic" title="Toggle Transcription">🎤</span>
                    <span class="vis-btn trans active" id="toggle-trans" title="Toggle Translation">🌐</span>
                </div>
            </div>
        </div>
        <div id="viewport">
            <div id="content">
                <span class="placeholder">Waiting for speech...</span>
            </div>
        </div>
        <div id="resize-hint">⋱</div>
    </div>
    <script>
        const container = document.getElementById('container');
        const viewport = document.getElementById('viewport');
        const content = document.getElementById('content');
        const toggleMic = document.getElementById('toggle-mic');
        const toggleTrans = document.getElementById('toggle-trans');
        const fontDecrease = document.getElementById('font-decrease');
        const fontIncrease = document.getElementById('font-increase');
        const resizeHint = document.getElementById('resize-hint');
        
        let currentFontSize = 16;
        let isResizing = false;
        let resizeStartX = 0;
        let resizeStartY = 0;
        let micVisible = true;
        let transVisible = true;
        
        // Drag support
        container.addEventListener('mousedown', function(e) {{
            if (e.target.closest('#controls') || e.target.id === 'resize-hint' || isResizing) return;
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
        
        // Audio source / Language selector
        const selector = document.getElementById('audio-source') || document.getElementById('language-select');
        if (selector) {{
            selector.addEventListener('change', function(e) {{
                e.stopPropagation();
                if (this.id === 'audio-source') {{
                    window.ipc.postMessage('audioSource:' + this.value);
                }} else {{
                    window.ipc.postMessage('language:' + this.value);
                }}
            }});
            selector.addEventListener('mousedown', function(e) {{
                e.stopPropagation();
            }});
        }}
        
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
                content.innerHTML = '<span class="placeholder">Waiting for speech...</span>';
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
        
        // Extract audio source and target language from preset
        let audio_source = preset.audio_source.as_str();
        let target_language = if preset.blocks.len() > 1 {
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
        
        // Calculate positions
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        
        let has_translation = preset.blocks.len() > 1;
        
        let (main_x, main_y) = if has_translation {
            let total_w = OVERLAY_WIDTH * 2 + GAP;
            ((screen_w - total_w) / 2, (screen_h - OVERLAY_HEIGHT) / 2)
        } else {
            ((screen_w - OVERLAY_WIDTH) / 2, (screen_h - OVERLAY_HEIGHT) / 2)
        };
        
        // Create popup window with resize support
        let main_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Realtime Transcription"),
            WS_POPUP | WS_VISIBLE,
            main_x, main_y, OVERLAY_WIDTH, OVERLAY_HEIGHT,
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
        create_realtime_webview(main_hwnd, false, audio_source, &target_language);
        
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
            
            let trans_x = main_x + OVERLAY_WIDTH + GAP;
            let trans_hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                trans_class,
                w!("Translation"),
                WS_POPUP | WS_VISIBLE,
                trans_x, main_y, TRANSLATION_WIDTH, TRANSLATION_HEIGHT,
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
            create_realtime_webview(trans_hwnd, true, audio_source, &target_language);
            
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



fn create_realtime_webview(hwnd: HWND, is_translation: bool, audio_source: &str, current_language: &str) {
    let hwnd_key = hwnd.0 as isize;
    
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect); }
    
    // Use full language list from isolang crate
    let languages = get_all_languages();
    let html = get_realtime_html(is_translation, audio_source, languages, current_language);
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
            } else if body.starts_with("fontSize:") {
                // Font size change - store for future use
                if let Ok(size) = body[9..].parse::<u32>() {
                    println!("[WEBVIEW] Font size changed to: {}", size);
                    // Could save to config here
                }
            } else if body.starts_with("audioSource:") {
                // Audio source change - signal restart with new source
                let source = body[12..].to_string();
                println!("[WEBVIEW] Audio source change requested: {}", source);
                if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
                    *new_source = source;
                }
                AUDIO_SOURCE_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("language:") {
                // Target language change - signal update
                let lang = body[9..].to_string();
                println!("[WEBVIEW] Target language change requested: {}", lang);
                if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
                    *new_lang = lang;
                }
                LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
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

