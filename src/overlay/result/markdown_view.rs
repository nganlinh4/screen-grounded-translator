use pulldown_cmark::{html, Options, Parser};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::collections::HashMap;
use std::num::NonZeroIsize;
use std::sync::{Mutex, Once};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebViewBuilder};

lazy_static::lazy_static! {
    // Store WebViews per parent window - wrapped in thread-local storage to avoid Send issues
    static ref WEBVIEW_STATES: Mutex<HashMap<isize, bool>> = Mutex::new(HashMap::new());
    // Global flag to indicate WebView2 is ready
    static ref WEBVIEW_READY: Mutex<bool> = Mutex::new(false);
    // Flag to skip next navigation handler call (set before history.back())
    static ref SKIP_NEXT_NAVIGATION: Mutex<HashMap<isize, bool>> = Mutex::new(HashMap::new());
}

// Global hidden window handle for WebView warmup
static mut WARMUP_HWND: HWND = HWND(std::ptr::null_mut());
static REGISTER_WARMUP_CLASS: Once = Once::new();

// Thread-local storage for WebViews since they're not Send
thread_local! {
    static WEBVIEWS: std::cell::RefCell<HashMap<isize, wry::WebView>> = std::cell::RefCell::new(HashMap::new());
    // Hidden warmup WebView
    static WARMUP_WEBVIEW: std::cell::RefCell<Option<wry::WebView>> = std::cell::RefCell::new(None);
}

/// Wrapper for HWND to implement HasWindowHandle
struct HwndWrapper(HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.0 .0 as isize;
        if let Some(non_zero) = NonZeroIsize::new(hwnd) {
            let mut handle = Win32WindowHandle::new(non_zero);
            // hinstance is optional, can be null
            handle.hinstance = None;
            let raw = RawWindowHandle::Win32(handle);
            // Safety: the handle is valid for the lifetime of HwndWrapper
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        } else {
            Err(HandleError::Unavailable)
        }
    }
}

/// Warmup markdown WebView - call from main.rs at app startup
/// This pre-initializes WebView2 infrastructure from the main thread context
pub fn warmup() {
    std::thread::spawn(|| {
        warmup_internal();
    });
}

fn warmup_internal() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_MarkdownWarmup");

        REGISTER_WARMUP_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(warmup_wnd_proc);
            wc.hInstance = instance.into();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(std::ptr::null_mut());
            let _ = RegisterClassW(&wc);
        });

        // Create a small hidden window with WS_EX_NOACTIVATE to prevent focus stealing
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            class_name,
            w!("MarkdownWarmup"),
            WS_POPUP,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        WARMUP_HWND = hwnd;

        // Make it transparent (invisible)
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

        // Create a WebView to warm up WebView2 infrastructure
        let wrapper = HwndWrapper(hwnd);
        let result = WebViewBuilder::new()
            .with_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(50, 50)),
            })
            .with_html("<html><body>Warmup</body></html>")
            .with_transparent(false)
            .build_as_child(&wrapper);

        match result {
            Ok(webview) => {
                WARMUP_WEBVIEW.with(|wv| {
                    *wv.borrow_mut() = Some(webview);
                });
                // Mark as ready
                if let Ok(mut ready) = WEBVIEW_READY.lock() {
                    *ready = true;
                }
            }
            Err(_) => {
                // Warmup failed - WebView2 may not work
            }
        }

        // Message loop to keep the warmup thread alive
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn warmup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Google Fonts link for Google Sans Flex with rounded aesthetics
const GOOGLE_FONTS_LINK: &str = r#"
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link rel="preload" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:opsz,slnt,wdth,wght,ROND@6..144,-10..0,25..151,100..1000,100&display=swap" as="style" />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:opsz,slnt,wdth,wght,ROND@6..144,-10..0,25..151,100..1000,100&display=swap" />
"#;

/// CSS styling for the markdown content
const MARKDOWN_CSS: &str = r#"
    * { box-sizing: border-box; }
    body { 
        font-family: 'Google Sans Flex', 'Segoe UI', -apple-system, sans-serif;
        font-optical-sizing: auto;
        font-variation-settings: 'wght' 400, 'wdth' 100, 'slnt' 0, 'ROND' 100;
        font-size: 14px;
        line-height: 1.6;
        background: #1a1a1a;
        min-height: 100vh;
        color: #e0e0e0;
        padding: 8px;
        margin: 0;
        overflow-x: hidden;
        word-wrap: break-word;
    }
    body > *:first-child { margin-top: 0; }
    h1 { 
        font-size: 1.8em; 
        color: #4fc3f7; 
        border-bottom: 1px solid #333; 
        padding-bottom: 8px; 
        margin-top: 0;
        font-variation-settings: 'wght' 600, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    h2 { 
        font-size: 1.5em; 
        color: #81d4fa; 
        border-bottom: 1px solid #2a2a2a; 
        padding-bottom: 6px; 
        margin-top: 0.5em;
        font-variation-settings: 'wght' 550, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    h3 { 
        font-size: 1.2em; 
        color: #b3e5fc; 
        margin-top: 0.5em;
        font-variation-settings: 'wght' 500, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    h4, h5, h6 { 
        color: #e1f5fe; 
        margin-top: 0.5em;
        font-variation-settings: 'wght' 500, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    p { margin: 0.5em 0; }
    strong, b {
        font-variation-settings: 'wght' 600, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    em, i {
        font-variation-settings: 'wght' 400, 'wdth' 100, 'slnt' -10, 'ROND' 100;
    }
    code { 
        font-family: 'Cascadia Code', 'Fira Code', Consolas, monospace;
        background: #2d2d2d; 
        padding: 2px 6px; 
        border-radius: 4px;
        font-size: 0.9em;
        color: #ce9178;
    }
    pre { 
        background: #1a1a1a; 
        padding: 12px 16px; 
        border-radius: 8px; 
        overflow-x: auto;
        border: 1px solid #333;
    }
    pre code { 
        background: transparent; 
        padding: 0; 
        color: #d4d4d4;
    }
    a { color: #81d4fa; text-decoration: none; }
    a:hover { text-decoration: underline; }
    blockquote { 
        border-left: 4px solid #4fc3f7; 
        padding-left: 16px; 
        margin-left: 0;
        color: #aaa; 
        background: #1a1a1a;
        padding: 8px 16px;
        border-radius: 0 8px 8px 0;
        font-variation-settings: 'wght' 300, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    ul, ol { padding-left: 24px; margin: 0.8em 0; }
    li { margin: 4px 0; }
    table { 
        border-collapse: collapse; 
        width: 100%; 
        margin: 1em 0;
    }
    th, td { 
        border: 1px solid #444; 
        padding: 8px 12px; 
        text-align: left;
    }
    th { 
        background: #252525; 
        color: #81d4fa;
        font-variation-settings: 'wght' 600, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    tr:nth-child(even) { background: #1a1a1a; }
    hr { border: none; border-top: 1px solid #444; margin: 1.5em 0; }
    img { max-width: 100%; border-radius: 8px; }
    
    /* Scrollbar styling */
    ::-webkit-scrollbar { width: 8px; height: 8px; }
    ::-webkit-scrollbar-track { background: #1a1a1a; }
    ::-webkit-scrollbar-thumb { background: #444; border-radius: 4px; }
    ::-webkit-scrollbar-thumb:hover { background: #555; }
"#;

/// Check if content is already HTML (rather than Markdown)
fn is_html_content(content: &str) -> bool {
    let trimmed = content.trim();
    // Check for HTML doctype or opening html tag
    trimmed.starts_with("<!DOCTYPE") || 
    trimmed.starts_with("<!doctype") ||
    trimmed.starts_with("<html") ||
    trimmed.starts_with("<HTML") ||
    // Check for common HTML structure patterns
    (trimmed.contains("<html") && trimmed.contains("</html>")) ||
    (trimmed.contains("<head") && trimmed.contains("</head>")) ||
    // Also detect HTML fragments (has script/style but no html wrapper)
    is_html_fragment(content)
}

/// Check if content is an HTML fragment (has HTML-like content but no document wrapper)
/// Examples: <div><style>...</style><script>...</script></div>
fn is_html_fragment(content: &str) -> bool {
    let lower = content.to_lowercase();
    // Has script or style tags but no html/doctype wrapper
    (lower.contains("<script") || lower.contains("<style"))
        && !lower.contains("<!doctype")
        && !lower.contains("<html")
}

/// Wrap an HTML fragment in a proper document structure
/// This ensures WebView2 can properly parse the DOM
fn wrap_html_fragment(fragment: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body>
{}
</body>
</html>"#,
        fragment
    )
}

/// Inject localStorage/sessionStorage polyfill into HTML for WebView2 compatibility
/// WebView2's with_html() runs in a sandboxed context that denies storage access
/// This provides an in-memory fallback so scripts don't crash
fn inject_storage_polyfill(html: &str) -> String {
    // First, wrap HTML fragments in a proper document structure
    // This ensures WebView2 can properly parse the DOM (fixes "null" getElementById errors)
    let html = if is_html_fragment(html) {
        wrap_html_fragment(html)
    } else {
        html.to_string()
    };

    // Polyfill script that provides in-memory storage when real storage is blocked
    let polyfill = r#"<script>
(function() {
    // Check if localStorage is accessible
    try {
        var test = '__storage_test__';
        localStorage.setItem(test, test);
        localStorage.removeItem(test);
        // localStorage works, no polyfill needed
    } catch (e) {
        // localStorage blocked, create in-memory polyfill
        var memoryStorage = {};
        var createStorage = function() {
            return {
                _data: {},
                length: 0,
                getItem: function(key) { return this._data.hasOwnProperty(key) ? this._data[key] : null; },
                setItem: function(key, value) { this._data[key] = String(value); this.length = Object.keys(this._data).length; },
                removeItem: function(key) { delete this._data[key]; this.length = Object.keys(this._data).length; },
                clear: function() { this._data = {}; this.length = 0; },
                key: function(i) { var keys = Object.keys(this._data); return keys[i] || null; }
            };
        };
        try {
            Object.defineProperty(window, 'localStorage', { value: createStorage(), writable: false });
            Object.defineProperty(window, 'sessionStorage', { value: createStorage(), writable: false });
        } catch (e2) {
            // If defineProperty fails, try direct assignment
            window.localStorage = createStorage();
            window.sessionStorage = createStorage();
        }
    }
})();
</script>"#;

    // Find the best place to inject the polyfill (before any other scripts)
    // Priority: after <head>, after <html>, or at the very start
    let lower = html.to_lowercase();

    if let Some(pos) = lower.find("<head>") {
        // Inject right after <head>
        let insert_pos = pos + 6; // length of "<head>"
        let mut result = html[..insert_pos].to_string();
        result.push_str(polyfill);
        result.push_str(&html[insert_pos..]);
        result
    } else if let Some(pos) = lower.find("<head ") {
        // <head with attributes
        if let Some(end) = html[pos..].find('>') {
            let insert_pos = pos + end + 1;
            let mut result = html[..insert_pos].to_string();
            result.push_str(polyfill);
            result.push_str(&html[insert_pos..]);
            result
        } else {
            format!("{}{}", polyfill, html)
        }
    } else if let Some(pos) = lower.find("<html>") {
        let insert_pos = pos + 6;
        let mut result = html[..insert_pos].to_string();
        result.push_str(polyfill);
        result.push_str(&html[insert_pos..]);
        result
    } else if let Some(pos) = lower.find("<html ") {
        if let Some(end) = html[pos..].find('>') {
            let insert_pos = pos + end + 1;
            let mut result = html[..insert_pos].to_string();
            result.push_str(polyfill);
            result.push_str(&html[insert_pos..]);
            result
        } else {
            format!("{}{}", polyfill, html)
        }
    } else {
        // No head or html tag found, prepend polyfill
        format!("{}{}", polyfill, html)
    }
}

/// Convert markdown text to styled HTML, or pass through raw HTML
pub fn markdown_to_html(
    markdown: &str,
    is_refining: bool,
    preset_prompt: &str,
    input_text: &str,
) -> String {
    if is_refining && crate::overlay::utils::SHOW_REFINING_CONTEXT_QUOTE {
        let combined = if input_text.is_empty() {
            preset_prompt.to_string()
        } else {
            format!("{}\n\n{}", preset_prompt, input_text)
        };
        let quote = crate::overlay::utils::get_context_quote(&combined);
        return format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    {}
    <style>
        {} 
        body {{ 
            display: flex; 
            align-items: center; 
            justify-content: center; 
            text-align: center; 
            height: 100vh; 
            margin: 0; 
            padding: 24px;
            font-style: italic;
            color: #aaa;
            font-size: 16px;
        }}
    </style>
</head>
<body>{}</body>
</html>"#,
            GOOGLE_FONTS_LINK, MARKDOWN_CSS, quote
        );
    }

    // If input is already HTML, inject localStorage polyfill for WebView2 compatibility
    // WebView2's with_html() runs in a sandboxed context that denies localStorage access
    if is_html_content(markdown) {
        return inject_storage_polyfill(markdown);
    }

    // Otherwise, parse as Markdown
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    {}
    <style>{}</style>
</head>
<body>{}</body>
</html>"#,
        GOOGLE_FONTS_LINK, MARKDOWN_CSS, html_output
    )
}

/// Create a WebView child window for markdown rendering
/// Must be called from the main thread!
pub fn create_markdown_webview(parent_hwnd: HWND, markdown_text: &str, is_hovered: bool) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;
    let (is_refining, preset_prompt, input_text) = {
        let states = super::state::WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get(&hwnd_key) {
            (
                state.is_refining,
                state.preset_prompt.clone(),
                state.input_text.clone(),
            )
        } else {
            (false, String::new(), String::new())
        }
    };
    create_markdown_webview_ex(
        parent_hwnd,
        markdown_text,
        is_hovered,
        is_refining,
        &preset_prompt,
        &input_text,
    )
}

/// Create a WebView child window for markdown rendering (Internal version, call without lock if possible)
pub fn create_markdown_webview_ex(
    parent_hwnd: HWND,
    markdown_text: &str,
    is_hovered: bool,
    is_refining: bool,
    preset_prompt: &str,
    input_text: &str,
) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;

    // Check if we already have a webview
    let exists = WEBVIEWS.with(|webviews| webviews.borrow().contains_key(&hwnd_key));

    if exists {
        return update_markdown_content_ex(
            parent_hwnd,
            markdown_text,
            is_refining,
            preset_prompt,
            input_text,
        );
    }

    // Get parent window rect
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(parent_hwnd, &mut rect);
    }

    let html_content = markdown_to_html(markdown_text, is_refining, preset_prompt, input_text);

    let wrapper = HwndWrapper(parent_hwnd);

    // Small margin on edges for resize handle accessibility (2px)
    // 52px at bottom for buttons (btn_size 28 + margin 12 * 2) if hovered
    let edge_margin = 2.0;
    let button_area_height = if is_hovered { 52.0 } else { 0.0 };
    let content_width = ((rect.right - rect.left) as f64 - edge_margin * 2.0).max(50.0);
    let content_height =
        ((rect.bottom - rect.top) as f64 - edge_margin - button_area_height).max(50.0);

    // Create WebView with small margins so resize handles remain accessible
    // Use Physical coordinates since GetClientRect returns physical pixels
    let _hwnd_copy = parent_hwnd;
    let hwnd_key_for_nav = hwnd_key;

    // SIMPLIFIED FOR DEBUGGING - minimal WebView creation
    // CRITICAL: with_transparent(false) matches text_input's working config
    let result = WebViewBuilder::new()
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                edge_margin as i32,
                edge_margin as i32,
            )),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                content_width as u32,
                content_height as u32,
            )),
        })
        .with_html(&html_content)
        .with_transparent(false)
        .with_navigation_handler(move |url: String| {
            // Check if we should skip this navigation (triggered by history.back())
            let should_skip = {
                let mut skip_map = SKIP_NEXT_NAVIGATION.lock().unwrap();
                if skip_map.get(&hwnd_key_for_nav).copied().unwrap_or(false) {
                    skip_map.insert(hwnd_key_for_nav, false);
                    true
                } else {
                    false
                }
            };

            if should_skip {
                // This navigation was from history.back(), don't increment depth
                return true;
            }

            // Detect when user navigates to an external URL (clicked a link)
            // CRITICAL: Exclude wry internal URLs to prevent counting original content as browsing
            let is_internal = url.contains("wry.localhost")
                || url.contains("localhost")
                || url.contains("127.0.0.1")
                || url.starts_with("data:")
                || url.starts_with("about:");
            let is_external =
                (url.starts_with("http://") || url.starts_with("https://")) && !is_internal;

            if is_external {
                // Update browsing state and increment depth counter
                if let Ok(mut states) = super::state::WINDOW_STATES.lock() {
                    if let Some(state) = states.get_mut(&hwnd_key_for_nav) {
                        state.is_browsing = true;
                        state.navigation_depth += 1;
                        // For a new navigation (not history back/forward), reset max depth to current depth
                        state.max_navigation_depth = state.navigation_depth;

                        if state.is_editing {
                            state.is_editing = false;
                            super::refine_input::hide_refine_input(HWND(
                                hwnd_key_for_nav as *mut std::ffi::c_void,
                            ));
                        }
                    }
                }
            } else if is_internal {
                // If we hit an internal URL, we are likely back at the start (or initial load)
                // Force reset depth and browsing state to correct any drift
                if let Ok(mut states) = super::state::WINDOW_STATES.lock() {
                    if let Some(state) = states.get_mut(&hwnd_key_for_nav) {
                        if state.is_browsing {
                            // Only reset if we were browsing - this handles the "Back to Start" drift
                            state.is_browsing = false;
                            state.navigation_depth = 0;
                            state.max_navigation_depth = 0;
                            // Ensure repaint to hide buttons
                            unsafe {
                                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(
                                    Some(HWND(hwnd_key_for_nav as *mut std::ffi::c_void)),
                                    None,
                                    false,
                                );
                            }
                        }
                    }
                }
            }

            // Allow all navigation
            true
        })
        .build_as_child(&wrapper);

    match result {
        Ok(webview) => {
            WEBVIEWS.with(|webviews| {
                webviews.borrow_mut().insert(hwnd_key, webview);
            });

            let mut states = WEBVIEW_STATES.lock().unwrap();
            states.insert(hwnd_key, true);
            true
        }
        Err(_e) => {
            // WebView creation failed - warmup may not have completed
            false
        }
    }
}

/// Navigate back in browser history
pub fn go_back(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    // Determine if we need to recreate the webview (returning to original content)
    // or just go back in browser history.
    let (returning_to_original, markdown_text, is_hovered) = {
        let mut states = super::state::WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            if state.navigation_depth > 0 {
                state.navigation_depth -= 1;
            }

            // If depth is now 0, we are returning to the starting result content.
            // We recreate the WebView to ensure a clean state and avoid "white screen"
            // issues that happen when document.write is blocked by website CSP.
            if state.navigation_depth == 0 {
                state.is_browsing = false;
                state.max_navigation_depth = 0; // History is reset on recreation
                (true, state.full_text.clone(), state.is_hovered)
            } else {
                (false, String::new(), false)
            }
        } else {
            (false, String::new(), false)
        }
    };

    if returning_to_original {
        // Full recreation of the WebView with the desired content
        create_markdown_webview(parent_hwnd, &markdown_text, is_hovered);

        // Trigger repaint to hide navigation buttons
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(parent_hwnd), None, false);
        }
    } else {
        // Normal browser history back for deeper navigation
        // Set skip flag to prevent navigation_handler from re-incrementing depth
        {
            let mut skip_map = SKIP_NEXT_NAVIGATION.lock().unwrap();
            skip_map.insert(hwnd_key, true);
        }

        WEBVIEWS.with(|webviews| {
            if let Some(webview) = webviews.borrow().get(&hwnd_key) {
                let _ = webview.evaluate_script("history.back();");
            }
        });
    }
}

/// Navigate forward in browser history
pub fn go_forward(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    // Set skip flag to prevent navigation_handler from incrementing depth
    {
        let mut skip_map = SKIP_NEXT_NAVIGATION.lock().unwrap();
        skip_map.insert(hwnd_key, true);
    }

    // Increment navigation depth since we're going forward
    {
        let mut states = super::state::WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            if state.navigation_depth < state.max_navigation_depth {
                state.navigation_depth += 1;
                state.is_browsing = true;
            } else {
                return; // Cannot go forward
            }
        }
    }

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            let _ = webview.evaluate_script("history.forward();");
        }
    });
}

/// Update the markdown content in an existing WebView
pub fn update_markdown_content(parent_hwnd: HWND, markdown_text: &str) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;
    let (is_refining, preset_prompt, input_text) = {
        let states = super::state::WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get(&hwnd_key) {
            (
                state.is_refining,
                state.preset_prompt.clone(),
                state.input_text.clone(),
            )
        } else {
            (false, String::new(), String::new())
        }
    };
    update_markdown_content_ex(
        parent_hwnd,
        markdown_text,
        is_refining,
        &preset_prompt,
        &input_text,
    )
}

/// Check if HTML content contains scripts that need full browser capabilities
/// (localStorage, sessionStorage, IndexedDB, etc.)
fn content_needs_recreation(html: &str) -> bool {
    let lower = html.to_lowercase();
    // If content has <script> tags that might use storage APIs, it needs recreation
    // to get a proper origin instead of the sandboxed document.write context
    lower.contains("<script")
        && (lower.contains("localstorage") || 
     lower.contains("sessionstorage") || 
     lower.contains("indexeddb") ||
     lower.contains("const ") ||  // Variable declarations can conflict  
     lower.contains("let ") ||
     lower.contains("var "))
}

/// Update the markdown content in an existing WebView (Raw version, does not fetch state)
/// For interactive HTML with scripts: recreates WebView to get proper origin
/// For simple content: uses fast inline update
pub fn update_markdown_content_ex(
    parent_hwnd: HWND,
    markdown_text: &str,
    is_refining: bool,
    preset_prompt: &str,
    input_text: &str,
) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;
    let html = markdown_to_html(markdown_text, is_refining, preset_prompt, input_text);

    // Check if this content has scripts that need full browser capabilities
    // If so, we must recreate the WebView to get proper origin access
    if content_needs_recreation(&html) {
        // Destroy existing WebView and create fresh one
        destroy_markdown_webview(parent_hwnd);

        // Get hover state for sizing
        let is_hovered = {
            if let Ok(states) = super::state::WINDOW_STATES.lock() {
                states.get(&hwnd_key).map(|s| s.is_hovered).unwrap_or(false)
            } else {
                false
            }
        };

        // Recreate WebView with fresh content (will use with_html for proper origin)
        return create_markdown_webview_ex(
            parent_hwnd,
            markdown_text,
            is_hovered,
            is_refining,
            preset_prompt,
            input_text,
        );
    }

    // Fast path for simple content without scripts
    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            // For simple markdown, update body content via DOM manipulation
            // This is safe because we verified there are no conflicting scripts
            let escaped_html = html
                .replace('\\', "\\\\")
                .replace('`', "\\`")
                .replace("${", "\\${");
            let script = format!(
                "document.open(); document.write(`{}`); document.close();",
                escaped_html
            );
            let _ = webview.evaluate_script(&script);
            return true;
        }
        false
    })
}

/// Resize the WebView to match parent window
/// When hovered: leaves 52px at bottom for buttons
/// When not hovered: expands to full height for clean view
/// When refine input active: starts 44px from top (40px input + 4px gap)
pub fn resize_markdown_webview(parent_hwnd: HWND, is_hovered: bool) {
    let hwnd_key = parent_hwnd.0 as isize;

    // Check if refine input is active
    let refine_input_active = super::refine_input::is_refine_input_active(parent_hwnd);
    let top_offset = if refine_input_active { 44.0 } else { 2.0 }; // 40px input + 4px gap, or 2px edge margin

    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(parent_hwnd, &mut rect);

        // 2px edge margin for resize handles
        let edge_margin = 2.0;
        // Only reserve button area when hovered
        let button_area_height = if is_hovered { 52.0 } else { edge_margin };

        let content_width = ((rect.right - rect.left) as f64 - edge_margin * 2.0).max(50.0);
        let content_height =
            ((rect.bottom - rect.top) as f64 - top_offset - button_area_height).max(50.0);

        WEBVIEWS.with(|webviews| {
            if let Some(webview) = webviews.borrow().get(&hwnd_key) {
                // Use Physical coordinates since GetClientRect returns physical pixels
                let _ = webview.set_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                        edge_margin as i32,
                        top_offset as i32,
                    )),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        content_width as u32,
                        content_height as u32,
                    )),
                });
            }
        });
    }
}

/// Hide the WebView (toggle back to plain text)
pub fn hide_markdown_webview(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            let _ = webview.set_visible(false);
        }
    });
}

/// Show the WebView (toggle to markdown mode)
pub fn show_markdown_webview(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            let _ = webview.set_visible(true);
        }
    });
}

/// Destroy the WebView when window closes
pub fn destroy_markdown_webview(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    WEBVIEWS.with(|webviews| {
        webviews.borrow_mut().remove(&hwnd_key);
    });

    let mut states = WEBVIEW_STATES.lock().unwrap();
    states.remove(&hwnd_key);
}

/// Check if markdown webview exists for this window
pub fn has_markdown_webview(parent_hwnd: HWND) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;
    let states = WEBVIEW_STATES.lock().unwrap();
    states.get(&hwnd_key).copied().unwrap_or(false)
}

/// Save the current content as HTML file using Windows File Save dialog
/// Returns true if file was saved successfully
pub fn save_html_file(markdown_text: &str) -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG;
    use windows::Win32::UI::Shell::{
        FOLDERID_Downloads, FileSaveDialog, IFileSaveDialog, IShellItem,
        SHCreateItemFromParsingName, SHGetKnownFolderPath, FOS_OVERWRITEPROMPT,
        FOS_STRICTFILETYPES, SIGDN_FILESYSPATH,
    };

    unsafe {
        // Initialize COM
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // Create file dialog
        let dialog: IFileSaveDialog = match CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL) {
            Ok(d) => d,
            Err(_) => {
                CoUninitialize();
                return false;
            }
        };

        // Set file type filter - HTML files
        let filter_name: Vec<u16> = OsStr::new("HTML Files (*.html)")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let filter_pattern: Vec<u16> = OsStr::new("*.html")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let file_types = [COMDLG_FILTERSPEC {
            pszName: windows::core::PCWSTR(filter_name.as_ptr()),
            pszSpec: windows::core::PCWSTR(filter_pattern.as_ptr()),
        }];

        let _ = dialog.SetFileTypes(&file_types);
        let _ = dialog.SetFileTypeIndex(1);

        // Set default folder to Downloads
        if let Ok(downloads_path) =
            SHGetKnownFolderPath(&FOLDERID_Downloads, KNOWN_FOLDER_FLAG(0), None)
        {
            if let Ok(folder_item) =
                SHCreateItemFromParsingName::<PCWSTR, _, IShellItem>(PCWSTR(downloads_path.0), None)
            {
                let _ = dialog.SetFolder(&folder_item);
            }
        }

        // Set default extension
        let default_ext: Vec<u16> = OsStr::new("html")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = dialog.SetDefaultExtension(windows::core::PCWSTR(default_ext.as_ptr()));

        // Set default filename
        let default_name: Vec<u16> = OsStr::new("game.html")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = dialog.SetFileName(windows::core::PCWSTR(default_name.as_ptr()));

        // Set options
        let _ = dialog.SetOptions(FOS_OVERWRITEPROMPT | FOS_STRICTFILETYPES);

        // Show dialog
        if dialog.Show(None).is_err() {
            CoUninitialize();
            return false; // User cancelled
        }

        // Get result
        let result: windows::Win32::UI::Shell::IShellItem = match dialog.GetResult() {
            Ok(r) => r,
            Err(_) => {
                CoUninitialize();
                return false;
            }
        };

        // Get file path
        let path: windows::core::PWSTR = match result.GetDisplayName(SIGDN_FILESYSPATH) {
            Ok(p) => p,
            Err(_) => {
                CoUninitialize();
                return false;
            }
        };

        // Convert path to String
        let path_str = path.to_string().unwrap_or_default();

        // Free the path memory
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));

        CoUninitialize();

        // Generate HTML content
        let html_content = markdown_to_html(markdown_text, false, "", "");

        // Write to file
        match std::fs::write(&path_str, html_content) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}
