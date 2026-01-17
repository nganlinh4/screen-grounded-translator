//! WebView-based refine input that floats above the markdown view
//! This replaces the native EDIT control for a consistent UI experience

use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::collections::HashMap;
use std::num::NonZeroIsize;
use std::sync::Mutex;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebViewBuilder};

const WM_APP_SET_TEXT: u32 = WM_USER + 200; // Custom message for cross-thread text injection
const WM_APP_HISTORY_SET: u32 = WM_USER + 201; // Custom message for history text replacement

lazy_static::lazy_static! {
    /// Track which parent windows have refine input active
    static ref REFINE_STATES: Mutex<HashMap<isize, RefineInputState>> = Mutex::new(HashMap::new());

    /// Cross-thread text injection: (parent_key, text_to_insert)
    static ref PENDING_TEXT: Mutex<Option<(isize, String)>> = Mutex::new(None);

    /// Cross-thread history text replacement: (parent_key, text_to_replace_all)
    static ref PENDING_HISTORY_TEXT: Mutex<Option<(isize, String)>> = Mutex::new(None);
}

/// State for a refine input instance
struct RefineInputState {
    pub hwnd: HWND,      // Child window handle
    pub submitted: bool, // Has user submitted?
    pub cancelled: bool, // Has user cancelled?
    pub text: String,    // Submitted text
}

unsafe impl Send for RefineInputState {}

// Thread-local storage for WebViews (not Send)
thread_local! {
    static REFINE_WEBVIEWS: std::cell::RefCell<HashMap<isize, wry::WebView>> = std::cell::RefCell::new(HashMap::new());
    // Shared WebContext for this thread using common data directory
    static REFINE_WEB_CONTEXT: std::cell::RefCell<Option<WebContext>> = std::cell::RefCell::new(None);
}

/// Wrapper for HWND to implement HasWindowHandle
struct HwndWrapper(HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.0 .0 as isize;
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
unsafe extern "system" fn refine_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        _ if msg == WM_APP_SET_TEXT => {
            // Apply pending text from cross-thread call
            apply_pending_text();
            LRESULT(0)
        }

        _ if msg == WM_APP_HISTORY_SET => {
            // Apply history text replacement from cross-thread call
            apply_history_text();
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
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

/// Internal function to apply history text (replace all content)
fn apply_history_text() {
    let pending = PENDING_HISTORY_TEXT.lock().unwrap().take();
    if let Some((parent_key, text)) = pending {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace("${", "\\${")
            .replace('\n', " ")
            .replace('\r', "");

        REFINE_WEBVIEWS.with(|webviews| {
            if let Some(wv) = webviews.borrow().get(&parent_key) {
                // Replace all text (for history navigation)
                let script = format!(
                    r#"(function() {{
                        const editor = document.getElementById('editor');
                        const text = `{}`;
                        editor.value = text;
                        editor.selectionStart = editor.selectionEnd = text.length;
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
    :root {
        /* Dark Mode (Default) */
        --bg-grad-start: #333;
        --bg-grad-end: #2a2a2a;
        --border-bottom: #444;
        --editor-bg: #1a1a1a;
        --text-color: #fff;
        --placeholder-color: #888;
        --mic-border: rgba(0, 200, 255, 0.3);
        --mic-bg: rgba(30, 30, 30, 0.9);
        --mic-hover-bg: rgba(0, 200, 255, 0.15);
        --mic-fill: #00c8ff;
        --send-border: rgba(79, 195, 247, 0.3);
        --send-bg: rgba(30, 30, 30, 0.9);
        --send-hover-bg: rgba(79, 195, 247, 0.15);
        --send-fill: #4fc3f7;
        --hint-color: #888;
    }
    
    :root[data-theme="light"] {
        --bg-grad-start: #ffffff;
        --bg-grad-end: #f0f0f0;
        --border-bottom: #ddd;
        --editor-bg: #ffffff;
        --text-color: #333;
        --placeholder-color: #999;
        --mic-border: rgba(0, 136, 209, 0.3);
        --mic-bg: rgba(255, 255, 255, 0.9);
        --mic-hover-bg: rgba(0, 136, 209, 0.1);
        --mic-fill: #0288d1;
        --send-border: rgba(3, 155, 229, 0.3);
        --send-bg: rgba(255, 255, 255, 0.9);
        --send-hover-bg: rgba(3, 155, 229, 0.1);
        --send-fill: #039be5;
        --hint-color: #999;
    }

    * { box-sizing: border-box; margin: 0; padding: 0; }
    
    html, body {
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: transparent;
        font-family: 'Google Sans Flex', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
        font-variation-settings: 'ROND' 100;
    }
    
    .container {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        padding: 0 10px;
        background: linear-gradient(180deg, var(--bg-grad-start) 0%, var(--bg-grad-end) 100%);
        border-bottom: 1px solid var(--border-bottom);
    }
    
    #editor {
        flex: 1;
        height: 28px;
        min-width: 30px;
        padding: 4px 10px;
        border: 1px solid transparent;
        outline: none;
        border-radius: 12px;
        font-family: 'Google Sans Flex', sans-serif;
        font-variation-settings: 'ROND' 100;
        font-size: 13px;
        color: var(--text-color);
        background: var(--editor-bg);
        transition: box-shadow 0.1s ease, border-color 0.1s ease;
    }
    
    #editor::placeholder {
        color: var(--placeholder-color);
    }
    
    #editor:focus {
        outline: none;
        border-color: var(--send-fill);
        box-shadow: 0 0 0 1px var(--send-fill);
    }
    
    /* Mic Button */
    .mic-btn {
        width: 28px;
        height: 28px;
        aspect-ratio: 1;
        border-radius: 50%;
        border: 1px solid var(--mic-border);
        margin-left: 8px;
        background: var(--mic-bg);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }
    
    .mic-btn:hover {
        background: var(--mic-hover-bg);
        border-color: var(--mic-fill);
        box-shadow: 0 0 8px var(--mic-hover-bg);
    }
    
    .mic-btn svg, .send-btn svg {
        width: 16px;
        height: 16px;
    }
    
    .mic-btn svg { fill: var(--mic-fill); }
    .send-btn svg { fill: var(--send-fill); }
    
    /* Send Button */
    .send-btn {
        width: 28px;
        height: 28px;
        aspect-ratio: 1;
        border-radius: 50%;
        border: 1px solid var(--send-border);
        margin-left: 6px;
        background: var(--send-bg);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.15s ease;
        flex-shrink: 0;
    }
    
    .send-btn:hover {
        background: var(--send-hover-bg);
        border-color: var(--send-fill);
        box-shadow: 0 0 8px var(--send-hover-bg);
    }
    
    .send-btn:active {
        transform: scale(0.95);
    }
    
    .hint {
        font-size: 11px;
        color: var(--hint-color);
        font-variation-settings: 'ROND' 100, 'slnt' -10;
        margin-left: 10px;
        white-space: nowrap;
    }
"#;

/// Generate HTML for the refine input
fn get_refine_html(placeholder: &str, is_dark: bool) -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();
    let escaped = placeholder.replace('\'', "\\'");
    let theme_attr = if is_dark { "" } else { "data-theme=\"light\"" };
    format!(
        r#"<!DOCTYPE html>
<html {}>
<head>
    <meta charset="UTF-8">
    <style>{}{}</style>
</head>
<body>
    <div class="container">
        <input type="text" id="editor" placeholder="{}" autofocus>
        <button class="mic-btn" id="micBtn" title="Speech to text">
            {mic_svg}
        </button>
        <button class="send-btn" id="sendBtn" title="Send">
            {send_svg}
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
            
            // Arrow Up = Navigate history (older)
            if (e.key === 'ArrowUp') {{
                e.preventDefault();
                window.ipc.postMessage('history_up:' + editor.value);
            }}
            
            // Arrow Down = Navigate history (newer)
            if (e.key === 'ArrowDown') {{
                e.preventDefault();
                window.ipc.postMessage('history_down:' + editor.value);
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

        window.updateTheme = (isDark) => {{
            if (isDark) {{
                document.documentElement.removeAttribute('data-theme');
            }} else {{
                document.documentElement.setAttribute('data-theme', 'light');
            }}
        }};
    </script>
</body>
</html>"#,
        theme_attr,
        font_css,
        REFINE_CSS,
        escaped,
        mic_svg = crate::overlay::html_components::icons::get_icon_svg("mic"),
        send_svg = crate::overlay::html_components::icons::get_icon_svg("send")
    )
}

/// Show the refine input above the markdown view
/// Returns the child window handle for positioning
pub fn show_refine_input(parent_hwnd: HWND, placeholder: &str) -> bool {
    let parent_key = parent_hwnd.0 as isize;

    // Check if already exists
    let exists = REFINE_WEBVIEWS.with(|webviews| webviews.borrow().contains_key(&parent_key));

    let is_dark = if let Ok(app) = crate::APP.lock() {
        match app.config.theme_mode {
            crate::config::ThemeMode::Dark => true,
            crate::config::ThemeMode::Light => false,
            crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        }
    } else {
        true
    };

    if exists {
        // Just focus existing and update theme
        focus_refine_input(parent_hwnd);
        REFINE_WEBVIEWS.with(|webviews| {
            if let Some(wv) = webviews.borrow().get(&parent_key) {
                let script = format!("if(window.updateTheme) window.updateTheme({});", is_dark);
                let _ = wv.evaluate_script(&script);
            }
        });
        return true;
    }

    unsafe {
        let mut parent_rect = RECT::default();
        let _ = GetClientRect(parent_hwnd, &mut parent_rect);

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
            2,
            2,
            width,
            input_height, // Position at top with small margin
            Some(parent_hwnd),
            None,
            Some(instance.into()),
            None,
        );

        if child_hwnd.is_err() {
            return false;
        }

        // Create WebView inside the child window
        let html = get_refine_html(placeholder, is_dark);
        let child_hwnd = child_hwnd.unwrap();
        let wrapper = HwndWrapper(child_hwnd);

        REFINE_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("result"));
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        let parent_key_for_ipc = parent_key;
        let result = {
            // LOCK SCOPE: Serialized build to prevent resource contention
            let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();
            crate::log_info!(
                "[RefineInput] Acquired init lock. Building for Parent HWND: {:?}...",
                parent_hwnd
            );

            let build_res = REFINE_WEB_CONTEXT.with(|ctx| {
                let mut ctx_ref = ctx.borrow_mut();
                let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                    WebViewBuilder::new_with_web_context(web_ctx)
                } else {
                    WebViewBuilder::new()
                };
                let builder =
                    crate::overlay::html_components::font_manager::configure_webview(builder);

                // Store HTML in font server and get URL for same-origin font loading
                let page_url =
                    crate::overlay::html_components::font_manager::store_html_page(html.clone())
                        .unwrap_or_else(|| {
                            format!("data:text/html,{}", urlencoding::encode(&html))
                        });

                builder
                    .with_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            width as u32,
                            input_height as u32,
                        )),
                    })
                    .with_url(&page_url)
                    .with_transparent(false)
                    .with_ipc_handler(move |msg: wry::http::Request<String>| {
                        let body = msg.body();
                        let mut states = REFINE_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&parent_key_for_ipc) {
                            if body.starts_with("submit:") {
                                let text = body.strip_prefix("submit:").unwrap_or("").to_string();
                                // Save to history before submitting
                                crate::overlay::input_history::add_to_history(&text);
                                state.text = text;
                                state.submitted = true;
                            } else if body == "cancel" {
                                crate::overlay::input_history::reset_history_navigation();
                                state.cancelled = true;
                            } else if body.starts_with("history_up:") {
                                let current = body.strip_prefix("history_up:").unwrap_or("");
                                if let Some(text) =
                                    crate::overlay::input_history::navigate_history_up(current)
                                {
                                    // Get the child window handle for posting message
                                    let child_hwnd = state.hwnd;
                                    drop(states); // Release lock before posting
                                    *PENDING_HISTORY_TEXT.lock().unwrap() =
                                        Some((parent_key_for_ipc, text));
                                    let _ = PostMessageW(
                                        Some(child_hwnd),
                                        WM_APP_HISTORY_SET,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                    return;
                                }
                            } else if body.starts_with("history_down:") {
                                let current = body.strip_prefix("history_down:").unwrap_or("");
                                if let Some(text) =
                                    crate::overlay::input_history::navigate_history_down(current)
                                {
                                    let child_hwnd = state.hwnd;
                                    drop(states);
                                    *PENDING_HISTORY_TEXT.lock().unwrap() =
                                        Some((parent_key_for_ipc, text));
                                    let _ = PostMessageW(
                                        Some(child_hwnd),
                                        WM_APP_HISTORY_SET,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                    return;
                                }
                            } else if body == "mic" {
                                // Trigger transcription preset
                                let transcribe_idx = {
                                    let app = crate::APP.lock().unwrap();
                                    app.config
                                        .presets
                                        .iter()
                                        .position(|p| p.id == "preset_transcribe")
                                };

                                if let Some(preset_idx) = transcribe_idx {
                                    std::thread::spawn(move || {
                                        crate::overlay::recording::show_recording_overlay(
                                            preset_idx,
                                        );
                                    });
                                }
                            }
                        }
                    })
                    .build_as_child(&wrapper)
            });
            crate::log_info!(
                "[RefineInput] Build finished. Status: {}",
                if build_res.is_ok() { "OK" } else { "ERR" }
            );
            build_res
        };

        match result {
            Ok(webview) => {
                REFINE_WEBVIEWS.with(|webviews| {
                    webviews.borrow_mut().insert(parent_key, webview);
                });

                let mut states = REFINE_STATES.lock().unwrap();
                states.insert(
                    parent_key,
                    RefineInputState {
                        hwnd: child_hwnd,
                        submitted: false,
                        cancelled: false,
                        text: String::new(),
                    },
                );

                // Immediately focus
                focus_refine_input(parent_hwnd);

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
    unsafe {
        // Ensure the parent window is active so the WebView can receive input
        let _ = SetForegroundWindow(parent_hwnd);
    }

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
    // Reset history navigation when hiding
    crate::overlay::input_history::reset_history_navigation();

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
            let _ = SetWindowPos(
                state.hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
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
    states
        .keys()
        .next()
        .map(|&k| HWND(k as *mut std::ffi::c_void))
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
            let _ = PostMessageW(Some(hwnd), WM_APP_SET_TEXT, WPARAM(0), LPARAM(0));
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
            let _ = GetClientRect(parent_hwnd, &mut parent_rect);

            let input_height = 40i32;
            let width = parent_rect.right - 4; // 2px margin each side

            // Resize the child window
            let _ = SetWindowPos(
                hwnd,
                Some(HWND::default()),
                2,
                2,
                width,
                input_height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );

            // Resize the WebView inside
            REFINE_WEBVIEWS.with(|webviews| {
                if let Some(webview) = webviews.borrow().get(&parent_key) {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            width as u32,
                            input_height as u32,
                        )),
                    });
                }
            });
        }
    }
}
