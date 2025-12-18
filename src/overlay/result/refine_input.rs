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

lazy_static::lazy_static! {
    /// Track which parent windows have refine input active
    static ref REFINE_STATES: Mutex<HashMap<isize, RefineInputState>> = Mutex::new(HashMap::new());
}

/// State for a refine input instance
struct RefineInputState {
    pub hwnd: HWND,       // Child window handle
    pub submitted: bool,  // Has user submitted?
    pub cancelled: bool,  // Has user cancelled?
    pub text: String,     // Submitted text
}

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
    DefWindowProcW(hwnd, msg, wparam, lparam)
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
        <span class="hint">Enter ↵ | Esc ✕</span>
    </div>
    <script>
        const editor = document.getElementById('editor');
        
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
            wc.hInstance = instance;
            wc.lpszClassName = class_name;
            wc.hbrBackground = HBRUSH(0);
            CLASS_ATOM = RegisterClassW(&wc);
        }
        
        let child_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SGT_RefineInput"),
            w!(""),
            WS_CHILD | WS_VISIBLE,
            2, 2, width, input_height, // Position at top with small margin
            parent_hwnd,
            None, instance, None
        );
        
        if child_hwnd.0 == 0 {
            return false;
        }
        
        // Create WebView inside the child window
        let html = get_refine_html(placeholder);
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
                DestroyWindow(child_hwnd);
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

/// Get the height of the refine input (for layout calculation)
pub fn get_refine_input_height() -> i32 {
    40 // Fixed height
}
