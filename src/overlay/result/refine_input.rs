//! WebView-based refine input that floats above the markdown view
//! This replaces the native EDIT control for a consistent UI experience

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use std::sync::Mutex;
use std::collections::HashMap;
use std::num::NonZeroIsize;
use wry::{WebViewBuilder, Rect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle, Win32WindowHandle, HandleError};
use windows::core::w;

const WM_APP_SET_TEXT: u32 = WM_USER + 200; // Custom message for cross-thread text injection

lazy_static::lazy_static! {
    /// Track which parent windows have refine input active
    static ref REFINE_STATES: Mutex<HashMap<isize, RefineInputState>> = Mutex::new(HashMap::new());
    
    /// Cross-thread text injection: (parent_key, text_to_insert)
    static ref PENDING_TEXT: Mutex<Option<(isize, String)>> = Mutex::new(None);
}

/// State for a refine input instance
struct RefineInputState {
    pub hwnd: HWND,       // Child window handle
    pub submitted: bool,  // Has user submitted?
    pub cancelled: bool,  // Has user cancelled?
    pub text: String,     // Submitted text
}

unsafe impl Send for RefineInputState {}

// Thread-local storage for WebViews (not Send)
thread_local! {
    static REFINE_WEBVIEWS: std::cell::RefCell<HashMap<isize, wry::WebView>> = std::cell::RefCell::new(HashMap::new());
}

/// Wrapper for HWND to implement HasWindowHandle
struct HwndWrapper(HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
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

/// Window procedure for the refine input child window
unsafe extern "system" fn refine_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        _ if msg == WM_APP_SET_TEXT => {
            // Apply pending text from cross-thread call
            apply_pending_text();
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Internal function to apply pending text (insert at cursor position)
fn apply_pending_text() {
    let pending = PENDING_TEXT.lock().unwrap().take();
    if let Some((parent_key, text)) = pending {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace("${", "\\${")
            .replace('\n', " ") // Refine input is single line
            .replace('\r', "");
        
        REFINE_WEBVIEWS.with(|webviews| {
            if let Some(wv) = webviews.borrow().get(&parent_key) {
                // Insert at cursor position instead of replacing all text
                let script = format!(
                    r#"(function() {{
                        const editor = document.getElementById('editor');
                        const start = editor.selectionStart;
                        const end = editor.selectionEnd;
                        const text = `{}`;
                        editor.value = editor.value.substring(0, start) + text + editor.value.substring(end);
                        editor.selectionStart = editor.selectionEnd = start + text.length;
                        editor.focus();
                    }})();"#,
                    escaped
                );
                let _ = wv.evaluate_script(&script);
            }
        });
    }
}

/// CSS for the compact refine input
const REFINE_CSS: &str = r#"
    * { box-sizing: border-box; margin: 0; padding: 0; }
    
    html, body {
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: #2a2a2a;
        font-family: 'Segoe UI', -apple-system, BlinkMacSystemFont, sans-serif;
    }
    
    .container {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        padding: 0 10px;
        background: linear-gradient(180deg, #333 0%, #2a2a2a 100%);
        border-bottom: 1px solid #444;
    }
    
    #editor {
        flex: 1;
        height: 28px;
        min-width: 30px;
        padding: 4px 10px;
        border: none;
        outline: none;
        border-radius: 6px;
        font-family: 'Segoe UI', -apple-system, BlinkMacSystemFont, sans-serif;
        font-size: 13px;
        color: #fff;
        background: #1a1a1a;
    }
    
    #editor::placeholder {
        color: #888;
    }
    
    #editor:focus {
        outline: none;
        box-shadow: 0 0 0 1px #4fc3f7;
    }
    
    /* Mic Button - Solid cyan aesthetic */
    .mic-btn {
        width: 28px;
        height: 28px;
        aspect-ratio: 1;
        border-radius: 50%;
        border: 1px solid rgba(0, 200, 255, 0.3);
        margin-left: 8px;
        background: rgba(30, 30, 30, 0.9);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }
    
    .mic-btn:hover {
        background: rgba(0, 200, 255, 0.15);
        border-color: #00c8ff;
        box-shadow: 0 0 8px rgba(0, 200, 255, 0.4);
    }
    
    .mic-btn:active {
        transform: scale(0.95);
    }
    
    .mic-btn svg {
        width: 14px;
        height: 14px;
        fill: #00c8ff;
    }
    
    /* Send Button - Solid teal aesthetic */
    .send-btn {
        width: 28px;
        height: 28px;
        aspect-ratio: 1;
        border-radius: 50%;
        border: 1px solid rgba(79, 195, 247, 0.3);
        margin-left: 6px;
        background: rgba(30, 30, 30, 0.9);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }
    
    .send-btn:hover {
        background: rgba(79, 195, 247, 0.15);
        border-color: #4fc3f7;
        box-shadow: 0 0 8px rgba(79, 195, 247, 0.4);
    }
    
    .send-btn:active {
        transform: scale(0.95);
    }
    
    .send-btn svg {
        width: 14px;
        height: 14px;
        fill: #4fc3f7;
    }
    
    .hint {
        font-size: 11px;
        color: #888;
        margin-left: 10px;
        white-space: nowrap;
    }
"#;

/// Generate HTML for the refine input
fn get_refine_html(placeholder: &str) -> String {
    let escaped = placeholder.replace('\'', "\\'");
    format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>{}</style>
</head>
<body>
    <div class="container">
        <input type="text" id="editor" placeholder="{}" autofocus>
        <button class="mic-btn" id="micBtn" title="Speech to text">
            <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3z"/>
                <path d="M17 11c0 2.76-2.24 5-5 5s-5-2.24-5-5H5c0 3.53 2.61 6.43 6 6.92V21h2v-3.08c3.39-.49 6-3.39 6-6.92h-2z"/>
            </svg>
        </button>
        <button class="send-btn" id="sendBtn" title="Send">
            <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
            </svg>
        </button>
        <span class="hint">Esc ✕</span>
    </div>
    <script>
        const editor = document.getElementById('editor');
        const micBtn = document.getElementById('micBtn');
        const sendBtn = document.getElementById('sendBtn');
        
        window.onload = () => {{
            setTimeout(() => editor.focus(), 50);
        }};
        
        editor.addEventListener('keydown', (e) => {{
            if (e.key === 'Enter') {{
                e.preventDefault();
                const text = editor.value.trim();
                if (text) {{
                    window.ipc.postMessage('submit:' + text);
                }}
            }}
            
            if (e.key === 'Escape') {{
                e.preventDefault();
                window.ipc.postMessage('cancel');
            }}
        }});
        
        micBtn.addEventListener('click', (e) => {{
            e.preventDefault();
            window.ipc.postMessage('mic');
        }});
        
        // Send button click (simulates Enter)
        sendBtn.addEventListener('click', (e) => {{
            e.preventDefault();
            const text = editor.value.trim();
            if (text) {{
                window.ipc.postMessage('submit:' + text);
            }}
        }});
        
        document.addEventListener('contextmenu', e => e.preventDefault());
    </script>
</body>
</html>"#, REFINE_CSS, escaped)
}

/// Show the refine input above the markdown view
/// Returns the child window handle for positioning
pub fn show_refine_input(parent_hwnd: HWND, placeholder: &str) -> bool {
    let parent_key = parent_hwnd.0 as isize;
    
    // Check if already exists
    let exists = REFINE_WEBVIEWS.with(|webviews| {
        webviews.borrow().contains_key(&parent_key)
    });
    
    if exists {
        // Just focus existing
        focus_refine_input(parent_hwnd);
        return true;
    }
    
    unsafe {
        let mut parent_rect = RECT::default();
        GetClientRect(parent_hwnd, &mut parent_rect);
        
        let input_height = 40i32;
        let width = parent_rect.right - 4; // 2px margin each side
        
        // Create the child window for the WebView
        let instance = GetModuleHandleW(None).unwrap();
        
        // Use a simple static child window class
        static mut CLASS_ATOM: u16 = 0;
        if CLASS_ATOM == 0 {
            let class_name = w!("SGT_RefineInput");
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(refine_wnd_proc);
            wc.hInstance = instance.into();
            wc.lpszClassName = class_name;
            wc.hbrBackground = HBRUSH::default();
            CLASS_ATOM = RegisterClassW(&wc);
        }
        
        let child_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SGT_RefineInput"),
            w!(""),
            WS_CHILD | WS_VISIBLE,
            2, 2, width, input_height, // Position at top with small margin
            Some(parent_hwnd),
            None, Some(instance.into()), None
        );
        
        if child_hwnd.is_err() {
            return false;
        }
        
        // Create WebView inside the child window
        let html = get_refine_html(placeholder);
        let child_hwnd = child_hwnd.unwrap();
        let wrapper = HwndWrapper(child_hwnd);
        
        let parent_key_for_ipc = parent_key;
        let result = WebViewBuilder::new()
            .with_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width as u32, input_height as u32)),
            })
            .with_html(&html)
            .with_transparent(false)
            .with_ipc_handler(move |msg: wry::http::Request<String>| {
                let body = msg.body();
                let mut states = REFINE_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&parent_key_for_ipc) {
                    if body.starts_with("submit:") {
                        state.text = body.strip_prefix("submit:").unwrap_or("").to_string();
                        state.submitted = true;
                    } else if body == "cancel" {
                        state.cancelled = true;
                    } else if body == "mic" {
                        // Trigger transcription preset
                        let transcribe_idx = {
                            let app = crate::APP.lock().unwrap();
                            app.config.presets.iter().position(|p| p.id == "preset_transcribe")
                        };
                        
                        if let Some(preset_idx) = transcribe_idx {
                            std::thread::spawn(move || {
                                crate::overlay::recording::show_recording_overlay(preset_idx);
                            });
                        }
                    }
                }
            })
            .build_as_child(&wrapper);
        
        match result {
            Ok(webview) => {
                REFINE_WEBVIEWS.with(|webviews| {
                    webviews.borrow_mut().insert(parent_key, webview);
                });
                
                let mut states = REFINE_STATES.lock().unwrap();
                states.insert(parent_key, RefineInputState {
                    hwnd: child_hwnd,
                    submitted: false,
                    cancelled: false,
                    text: String::new(),
                });
                
                true
            }
            Err(_) => {
                let _ = DestroyWindow(child_hwnd);
                false
            }
        }
    }
}

/// Focus the refine input WebView
pub fn focus_refine_input(parent_hwnd: HWND) {
    let parent_key = parent_hwnd.0 as isize;
    
    REFINE_WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&parent_key) {
            let _ = webview.focus();
            let _ = webview.evaluate_script("document.getElementById('editor').focus();");
        }
    });
}

/// Check if user submitted or cancelled, and get the text
/// Returns: (submitted, cancelled, text)
pub fn poll_refine_input(parent_hwnd: HWND) -> (bool, bool, String) {
    let parent_key = parent_hwnd.0 as isize;
    
    let mut states = REFINE_STATES.lock().unwrap();
    if let Some(state) = states.get_mut(&parent_key) {
        let result = (state.submitted, state.cancelled, state.text.clone());
        // Reset flags after reading
        state.submitted = false;
        state.cancelled = false;
        if result.0 || result.1 {
            state.text.clear();
        }
        result
    } else {
        (false, false, String::new())
    }
}

/// Hide and destroy the refine input
pub fn hide_refine_input(parent_hwnd: HWND) {
    let parent_key = parent_hwnd.0 as isize;
    
    // Remove WebView first
    REFINE_WEBVIEWS.with(|webviews| {
        webviews.borrow_mut().remove(&parent_key);
    });
    
    // Remove state and destroy window
    let mut states = REFINE_STATES.lock().unwrap();
    if let Some(state) = states.remove(&parent_key) {
        unsafe {
            let _ = DestroyWindow(state.hwnd);
        }
    }
}

/// Check if refine input is currently visible
pub fn is_refine_input_active(parent_hwnd: HWND) -> bool {
    let parent_key = parent_hwnd.0 as isize;
    let states = REFINE_STATES.lock().unwrap();
    states.contains_key(&parent_key)
}

/// Bring the refine input to the top of the z-order
/// Call this after creating other child windows to ensure refine input stays visible
pub fn bring_to_top(parent_hwnd: HWND) {
    let parent_key = parent_hwnd.0 as isize;
    let states = REFINE_STATES.lock().unwrap();
    if let Some(state) = states.get(&parent_key) {
        unsafe {
            let _ = SetWindowPos(state.hwnd, Some(HWND_TOP), 0, 0, 0, 0, 
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }
}

/// Check if ANY refine input is active (across all windows)
/// Used to detect if we should paste into refine input
pub fn is_any_refine_active() -> bool {
    let states = REFINE_STATES.lock().unwrap();
    !states.is_empty()
}

/// Get the parent HWND of any active refine input
pub fn get_active_refine_parent() -> Option<HWND> {
    let states = REFINE_STATES.lock().unwrap();
    states.keys().next().map(|&k| HWND(k as *mut std::ffi::c_void))
}

/// Set text in the refine input (cross-thread safe)
/// Inserts at cursor position instead of replacing all text
pub fn set_refine_text(parent_hwnd: HWND, text: &str) {
    let parent_key = parent_hwnd.0 as isize;
    
    // Get the child window handle for this refine input
    let child_hwnd = {
        let states = REFINE_STATES.lock().unwrap();
        states.get(&parent_key).map(|s| s.hwnd)
    };
    
    if let Some(hwnd) = child_hwnd {
        // Store the text and parent key in the mutex
        *PENDING_TEXT.lock().unwrap() = Some((parent_key, text.to_string()));
        
        // Post message to the child window to trigger the injection
        unsafe {
            PostMessageW(Some(hwnd), WM_APP_SET_TEXT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Resize the refine input to match parent window width
/// Call this when the parent window is resized
pub fn resize_refine_input(parent_hwnd: HWND) {
    let parent_key = parent_hwnd.0 as isize;
    
    // Get the child window handle
    let child_hwnd = {
        let states = REFINE_STATES.lock().unwrap();
        states.get(&parent_key).map(|s| s.hwnd)
    };
    
    if let Some(hwnd) = child_hwnd {
        unsafe {
            let mut parent_rect = RECT::default();
            GetClientRect(parent_hwnd, &mut parent_rect);
            
            let input_height = 40i32;
            let width = parent_rect.right - 4; // 2px margin each side
            
            // Resize the child window
            let _ = SetWindowPos(
                hwnd, 
                Some(HWND::default()), 
                2, 2, width, input_height,
                SWP_NOZORDER | SWP_NOACTIVATE
            );
            
            // Resize the WebView inside
            REFINE_WEBVIEWS.with(|webviews| {
                if let Some(webview) = webviews.borrow().get(&parent_key) {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width as u32, input_height as u32)),
                    });
                }
            });
        }
    }
}
