use pulldown_cmark::{html, Options, Parser};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::collections::HashMap;
use std::num::NonZeroIsize;
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, Once};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebViewBuilder};

lazy_static::lazy_static! {
    // Store WebViews per parent window - wrapped in thread-local storage to avoid Send issues
    static ref WEBVIEW_STATES: Mutex<HashMap<isize, bool>> = Mutex::new(HashMap::new());
    // Global flag to indicate WebView2 is ready
    static ref WEBVIEW_READY: Mutex<bool> = Mutex::new(false);
    // Flag to skip next navigation handler call (set before history.back())
    static ref SKIP_NEXT_NAVIGATION: Mutex<HashMap<isize, bool>> = Mutex::new(HashMap::new());
}

static MARKDOWN_PAGE_COUNTER: AtomicU64 = AtomicU64::new(1);

// Global hidden window handle for WebView warmup
static mut WARMUP_HWND: HWND = HWND(std::ptr::null_mut());
static REGISTER_WARMUP_CLASS: Once = Once::new();

// Thread-local storage for WebViews since they're not Send
thread_local! {
    static WEBVIEWS: std::cell::RefCell<HashMap<isize, wry::WebView>> = std::cell::RefCell::new(HashMap::new());
    // Hidden warmup WebView
    static WARMUP_WEBVIEW: std::cell::RefCell<Option<wry::WebView>> = std::cell::RefCell::new(None);
    // Shared WebContext for all WebViews on this thread - reduces RAM by sharing browser processes
    static SHARED_WEB_CONTEXT: std::cell::RefCell<Option<WebContext>> = std::cell::RefCell::new(None);
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

        // Initialize shared WebContext for this thread (reduces RAM by sharing browser processes)
        // All modules use the same data directory, so WebView2 shares browser processes
        let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
        SHARED_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        // Create a WebView to warm up WebView2 infrastructure using shared context
        // Include font CSS AND render text in those fonts to force browser to download them
        let warmup_html = format!(
            r#"<html>
<head>
<style>
{}
body {{ font-family: 'Google Sans Flex', sans-serif; }}
.icons {{ font-family: 'Material Symbols Rounded'; font-size: 24px; }}
</style>
</head>
<body>
    <span style="font-weight: 100">Thin</span>
    <span style="font-weight: 300">Light</span>
    <span style="font-weight: 400">Regular</span>
    <span style="font-weight: 500">Medium</span>
    <span style="font-weight: 700">Bold</span>
    <span class="icons">pause stop mic</span>
</body>
</html>"#,
            crate::overlay::html_components::font_manager::get_font_css()
        );
        let wrapper = HwndWrapper(hwnd);

        // Store HTML in font server and get URL for same-origin font loading
        let page_url =
            crate::overlay::html_components::font_manager::store_html_page(warmup_html.clone())
                .unwrap_or_else(|| format!("data:text/html,{}", urlencoding::encode(&warmup_html)));

        let result = SHARED_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            if let Some(web_ctx) = ctx_ref.as_mut() {
                let builder = WebViewBuilder::new_with_web_context(web_ctx)
                    .with_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(50, 50)),
                    })
                    .with_url(&page_url)
                    .with_transparent(true);

                crate::overlay::html_components::font_manager::configure_webview(builder)
                    .build_as_child(&wrapper)
            } else {
                // Fallback without context
                let builder = WebViewBuilder::new()
                    .with_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(50, 50)),
                    })
                    .with_url(&page_url)
                    .with_transparent(true);

                crate::overlay::html_components::font_manager::configure_webview(builder)
                    .build_as_child(&wrapper)
            }
        });

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

/// Get font CSS for markdown view (uses locally cached fonts)
fn get_font_style() -> String {
    format!(
        "<style>{}</style>",
        crate::overlay::html_components::font_manager::get_font_css()
    )
}

/// CSS styling for the markdown content
const MARKDOWN_CSS: &str = r#"
    :root {
        --primary: #4fc3f7;
        --secondary: #81d4fa;
        --bg: transparent;
        --glass: rgba(255, 255, 255, 0.03);
        --glass-border: rgba(255, 255, 255, 0.05);
    }
    * { box-sizing: border-box; }
    
    /* Animation definitions */
    @keyframes shimmer {
        0% { background-position: 100% 0; }
        100% { background-position: -100% 0; }
    }
    
    /* Appearing animation with blur dissolve - matches realtime overlay style */
    @keyframes content-appear {
        from {
            opacity: 0;
            filter: blur(8px);
            -webkit-backdrop-filter: blur(12px);
            backdrop-filter: blur(12px);
            transform: translateY(4px);
        }
        to {
            opacity: 1;
            filter: blur(0);
            -webkit-backdrop-filter: blur(0);
            backdrop-filter: blur(0);
            transform: translateY(0);
        }
    }

    body { 
        font-family: 'Google Sans Flex', 'Segoe UI', -apple-system, sans-serif;
        font-optical-sizing: auto;
        /* wdth 90 for more compact text as requested */
        font-variation-settings: 'wght' 400, 'wdth' 90, 'slnt' 0, 'ROND' 100;
        /* Default size 14px - JavaScript fit_font_to_window handles dynamic scaling for short content */
        font-size: 14px;
        line-height: 1.5; /* Reduced line height for compactness */
        background: var(--bg);
        /* Removed min-height: 100vh to enable proper overflow detection for font scaling */
        color: white;
        margin: 0;
        padding: 0; /* Padding now handled by WebView edge margin */
        overflow-x: hidden;
        word-wrap: break-word;
        /* Appearing animation */
        animation: content-appear 0.35s cubic-bezier(0.2, 0, 0.2, 1) forwards;
    }
    
    body > *:first-child { margin-top: 0; }
    
    h1 { 
        font-size: 1.8em; 
        color: var(--primary); 
        margin-top: 0;
        margin-bottom: 12px; /* Reduced from 16px */
        padding: 0px;
        border-radius: 42px;
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        
        font-variation-settings: 'wght' 600, 'wdth' 110, 'slnt' 0, 'ROND' 100;
        text-align: center;
        position: relative;
        overflow: hidden;
    }

    h2 { 
        font-size: 1.4em; 
        color: var(--secondary); 
        /* Removed border-bottom */
        padding-bottom: 4px; 
        margin-top: 1.0em; /* Reduced from 1.2em */
        margin-bottom: 0.5em;
        font-variation-settings: 'wght' 550, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }

    h3 { 
        font-size: 1.2em; 
        color: #b3e5fc; 
        margin-top: 0.8em; /* Reduced from 1.0em */
        margin-bottom: 0.4em;
        font-variation-settings: 'wght' 500, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    
    h4, h5, h6 { 
        color: #e1f5fe; 
        margin-top: 0.8em;
        margin-bottom: 0.4em;
        font-variation-settings: 'wght' 500, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }

    p { margin: 0 0; }
    
    /* Interactive Word Styling - COLOR ONLY, preserves font scaling */
    .word {
        display: inline;
        transition: color 0.2s ease, text-shadow 0.2s ease;
        cursor: text;
    }

    /* 1. Center (Hovered) - Bright cyan + glow */
    .word:hover {
        color: var(--primary);
        text-shadow: 0 0 12px rgba(79, 195, 247, 0.6);
    }

    /* 2. Immediate Neighbors (Distance: 1) - Light cyan */
    .word:hover + .word {
        color: #e1f5fe;
        text-shadow: 0 0 6px rgba(79, 195, 247, 0.3);
    }
    .word:has(+ .word:hover) {
        color: #e1f5fe;
        text-shadow: 0 0 6px rgba(79, 195, 247, 0.3);
    }

    /* 3. Secondary Neighbors (Distance: 2) - Lighter cyan */
    .word:hover + .word + .word {
        color: #b3e5fc;
    }
    .word:has(+ .word + .word:hover) {
        color: #b3e5fc;
    }

    /* Headers need specific overriding to ensure the fisheye works on top of their base styles */
    h1 .word:hover, h2 .word:hover, h3 .word:hover {
        color: var(--primary);
    }
    
    /* Ensure code blocks remain non-interactive */
    pre .word {
        display: inline;
        transition: none;
    }
    pre .word:hover, 
    pre .word:hover + .word,
    pre .word:has(+ .word:hover) {
        color: inherit;
        text-shadow: none;
    }
    
    pre code { 
        background: transparent; 
        padding: 0; 
        color: #d4d4d4;
    }
    
    a { color: #82b1ff; text-decoration: none; transition: all 0.2s; cursor: pointer; }
    a .word { cursor: pointer; } /* Ensure link words show hand cursor */
    a:hover { color: #448aff; text-shadow: 0 0 10px rgba(68,138,255,0.4); text-decoration: none; }
    
    ul, ol { padding-left: 20px; margin: 0 0; }
    li { margin: 2px 0; } /* Reduced from 4px */
    
    table { 
        width: 100%; 
        border-collapse: separate; 
        border-spacing: 0; 
        margin: 12px 0; /* Reduced from 16px */
        border-radius: 8px; 
        overflow: hidden; 
        border: 1px solid #333; 
        background: rgba(0,0,0,0.2);
    }
    th { 
        background: #222; 
        padding: 8px 10px; /* Reduced from 10px */
        color: var(--primary); 
        text-align: left;
        font-weight: 600;
        border-bottom: 1px solid #333;
        font-variation-settings: 'wght' 600, 'wdth' 100, 'slnt' 0, 'ROND' 100;
    }
    td { 
        padding: 6px 10px; /* Reduced from 8px */
        border-top: 1px solid #333;
    }
    tr:first-child td { border-top: none; }
    tr:hover td { background: rgba(255,255,255,0.03); }
    
    hr { border: none; height: 1px; background: #333; margin: 16px 0; } /* Reduced from 24px */
    img { max-width: 100%; border-radius: 8px; box-shadow: 0 4px 12px rgba(0,0,0,0.3); }
    
    /* Streaming chunk animation - blur-dissolve for ONLY new content */
    @keyframes stream-chunk-in {
        from {
            opacity: 0;
            filter: blur(4px);
            transform: translateX(-2px);
        }
        to {
            opacity: 1;
            filter: blur(0);
            transform: translateX(0);
        }
    }
    
    /* Legacy chunk-appear kept for compatibility */
    @keyframes chunk-appear {
        from {
            opacity: 0;
            filter: blur(4px);
        }
        to {
            opacity: 1;
            filter: blur(0);
        }
    }
    
    /* Class for newly streamed text */
    .streaming-new {
        display: inline;
        animation: stream-chunk-in 0.25s ease-out forwards;
    }
    
    /* Smooth transition for all direct body children during updates */
    body > * {
        transition: opacity 0.15s ease-out, filter 0.15s ease-out;
    }
    
    ::-webkit-scrollbar { display: none; }
"#;

use pulldown_cmark::{Event, Tag, TagEnd};

/// Minimal HTML escaping for text content
fn escape_html_text(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&#39;")
}

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

/// Inject Grid.js into raw HTML if tables are present
fn inject_gridjs(html: &str) -> String {
    if !html.contains("<table") {
        return html.to_string();
    }

    let (css_url, js_url) = crate::overlay::html_components::grid_js::get_lib_urls();
    let gridjs_head = format!(
        r#"<link href="{}" rel="stylesheet" />
        <script src="{}"></script>
        <style>{}</style>"#,
        css_url,
        js_url,
        crate::overlay::html_components::grid_js::get_css()
    );
    let gridjs_body = format!(
        r#"<script>{}</script>"#,
        crate::overlay::html_components::grid_js::get_init_script()
    );

    let lower = html.to_lowercase();
    let mut result = html.to_string();

    // Inject CSS/JS into <head>
    if let Some(pos) = lower.find("</head>") {
        result.insert_str(pos, &gridjs_head);
    } else if let Some(pos) = lower.find("<body>") {
        result.insert_str(pos, &gridjs_head);
    } else {
        result.insert_str(0, &gridjs_head);
    }

    // Inject init script into <body>
    let lower_updated = result.to_lowercase();
    if let Some(pos) = lower_updated.find("</body>") {
        result.insert_str(pos, &gridjs_body);
    } else {
        result.push_str(&gridjs_body);
    }

    result
}

/// Inject CSS to hide scrollbars while preserving scrolling functionality
fn inject_scrollbar_css(html: &str) -> String {
    let css = "<style>::-webkit-scrollbar { display: none; }</style>";
    let lower = html.to_lowercase();
    let mut result = html.to_string();

    if let Some(pos) = lower.find("</head>") {
        result.insert_str(pos, css);
    } else if let Some(pos) = lower.find("<body>") {
        result.insert_str(pos, css);
    } else {
        result.insert_str(0, css);
    }
    result
}

/// Auto-scaling is now handled purely via CSS clamp() in MARKDOWN_CSS
/// This function is kept as a no-op for compatibility
fn inject_auto_scaling(html: &str) -> String {
    html.to_string()
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
            padding: 12px;
            font-style: italic;
            color: #aaa;
            font-size: 16px;
        }}
    </style>
</head>
<body>
    {}
    {}
</body>
</html>"#,
            get_font_style(),
            MARKDOWN_CSS,
            quote,
            "" // No extra script
        );
    }

    // If input is already HTML, inject localStorage polyfill, Grid.js, and hidden scrollbar styles
    if is_html_content(markdown) {
        let with_storage = inject_storage_polyfill(markdown);
        let with_grid = inject_gridjs(&with_storage);
        return inject_scrollbar_css(&with_grid);
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);

    // Custom wrapper to enable word-level interaction
    // We map text events to HTML events containing wrapped words
    let mut in_code_block = false;
    let mut in_table = false;

    let wrapped_parser = parser.map(|event| {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                event
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                event
            }
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                event
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                event
            }
            Event::Code(_) => {
                // Inline code event - return as is
                event
            }
            Event::Text(text) => {
                if !in_code_block && !in_table {
                    // Split text into words and wrap
                    let mut output = String::with_capacity(text.len() * 2);
                    let escaped = escape_html_text(&text);

                    for (i, part) in escaped.split(' ').enumerate() {
                        if i > 0 {
                            output.push(' ');
                        }
                        if part.trim().is_empty() {
                            output.push_str(part);
                        } else {
                            output.push_str("<span class=\"word\">");
                            output.push_str(part);
                            output.push_str("</span>");
                        }
                    }
                    Event::Html(output.into())
                } else {
                    Event::Text(text)
                }
            }
            Event::SoftBreak => Event::HardBreak,
            _ => event,
        }
    });

    let mut html_output = String::new();
    html::push_html(&mut html_output, wrapped_parser);

    // Grid.js Integration
    let has_table = html_output.contains("<table");
    let gridjs_head = if has_table {
        let (css_url, js_url) = crate::overlay::html_components::grid_js::get_lib_urls();
        format!(
            r#"<link href="{}" rel="stylesheet" />
            <script src="{}"></script>
            <style>{}</style>"#,
            css_url,
            js_url,
            crate::overlay::html_components::grid_js::get_css()
        )
    } else {
        String::new()
    };

    let gridjs_body = if has_table {
        format!(
            r#"<script>{}</script>"#,
            crate::overlay::html_components::grid_js::get_init_script()
        )
    } else {
        String::new()
    };

    let final_html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    {}
    <style>{}</style>
    {}
</head>
<body>
    {}
    {}
</body>
</html>"#,
        get_font_style(),
        MARKDOWN_CSS,
        gridjs_head,
        html_output,
        gridjs_body
    );

    inject_auto_scaling(&final_html)
}

/// Create a WebView child window for markdown rendering
/// Must be called from the main thread!
pub fn create_markdown_webview(parent_hwnd: HWND, markdown_text: &str, is_hovered: bool) -> bool {
    // Check if warmed up
    let is_ready = WEBVIEW_READY.lock().map(|g| *g).unwrap_or(false);
    if !is_ready {
        // Trigger warmup for recovery
        warmup();

        // Show localized message that feature is not ready yet
        let ui_lang = crate::APP.lock().unwrap().config.ui_language.clone();
        let locale = crate::gui::locale::LocaleText::get(&ui_lang);
        crate::overlay::auto_copy_badge::show_notification(locale.markdown_view_loading);

        // Wait up to 5 seconds
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if WEBVIEW_READY.lock().map(|g| *g).unwrap_or(false) {
                break;
            }
        }

        if !WEBVIEW_READY.lock().map(|g| *g).unwrap_or(false) {
            return false;
        }
    }

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

    // Edge margins: 4px left/right for resize handles, 2px top/bottom
    // 52px at bottom for buttons (btn_size 28 + margin 12 * 2) if hovered
    let margin_x = 4.0;
    let margin_y = 2.0;
    let button_area_height = margin_y;
    let content_width = ((rect.right - rect.left) as f64 - margin_x * 2.0).max(50.0);
    let content_height = ((rect.bottom - rect.top) as f64 - margin_y - button_area_height).max(0.0); // No min height - allow shrink for button bar

    // Create WebView with small margins so resize handles remain accessible
    // Use Physical coordinates since GetClientRect returns physical pixels
    let hwnd_key_for_nav = hwnd_key;

    // Use shared WebContext to match working realtime_webview behavior
    // This ensures we share the same browser process/cache/state
    let data_dir = crate::overlay::get_shared_webview_data_dir();
    let mut web_context = WebContext::new(Some(data_dir));

    // html_content is already a full HTML document from markdown_to_html
    let full_html = html_content;

    // Use store_html_page with safe, minimal retry (max 100ms total block)
    let mut page_url = String::new();
    for _ in 0..10 {
        if let Some(url) =
            crate::overlay::html_components::font_manager::store_html_page(full_html.clone())
        {
            page_url = url;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    if page_url.is_empty() {
        eprintln!("Failed to store markdown page in font server!");
        let error_html = "<html><body style='color:white'>Error: Could not connect to internal font server.</body></html>";
        if let Some(url) =
            crate::overlay::html_components::font_manager::store_html_page(error_html.to_string())
        {
            page_url = url;
        } else {
            page_url = "data:text/html,<html>Error</html>".to_string();
        }
    }

    let mut builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                margin_x as i32,
                margin_y as i32,
            )),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                content_width as u32,
                content_height as u32,
            )),
        })
        .with_url(&page_url)
        .with_transparent(true)
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            let body = msg.body();
            handle_markdown_ipc(parent_hwnd, body);
        });

    builder = crate::overlay::html_components::font_manager::configure_webview(builder);

    let result = builder
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
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            // Handle IPC messages from the WebView
            let body = msg.body();
            if body.starts_with("opacity:") {
                if let Ok(opacity_percent) = body["opacity:".len()..].parse::<f32>() {
                    // Opacity comes in as 0-100 from the slider
                    let alpha = ((opacity_percent / 100.0) * 255.0) as u8;
                    unsafe {
                        use windows::Win32::Foundation::COLORREF;
                        use windows::Win32::UI::WindowsAndMessaging::{
                            SetLayeredWindowAttributes, LWA_ALPHA,
                        };
                        // Set the actual WINDOW opacity
                        let _ =
                            SetLayeredWindowAttributes(parent_hwnd, COLORREF(0), alpha, LWA_ALPHA);
                    }
                }
            }
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
        && (lower.contains("localstorage")
            || lower.contains("sessionstorage")
            || lower.contains("indexeddb")
            || lower.contains("const ") // Variable declarations can conflict
            || lower.contains("let ")
            || lower.contains("var "))
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

/// Stream markdown content - optimized for rapid updates during streaming
/// Uses innerHTML instead of document.write to avoid document recreation
/// Call this during streaming, then call update_markdown_content at the end for final render
pub fn stream_markdown_content(parent_hwnd: HWND, markdown_text: &str) -> bool {
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

    stream_markdown_content_ex(
        parent_hwnd,
        markdown_text,
        is_refining,
        &preset_prompt,
        &input_text,
    )
}

/// Stream markdown content - internal version for rapid streaming updates
/// Uses innerHTML on body to avoid document recreation overhead
pub fn stream_markdown_content_ex(
    parent_hwnd: HWND,
    markdown_text: &str,
    is_refining: bool,
    preset_prompt: &str,
    input_text: &str,
) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;

    // Check if webview exists
    let exists = WEBVIEWS.with(|webviews| webviews.borrow().contains_key(&hwnd_key));

    if !exists {
        // Create the webview first if it doesn't exist
        return create_markdown_webview_ex(
            parent_hwnd,
            markdown_text,
            false, // is_hovered - during streaming, use compact view
            is_refining,
            preset_prompt,
            input_text,
        );
    }

    // For streaming, we just update the body innerHTML
    // This is much faster than document.write and doesn't recreate the document
    let html = markdown_to_html(markdown_text, is_refining, preset_prompt, input_text);

    // Extract just the body content from the full HTML
    // The HTML structure is: ....<body>CONTENT</body>....
    let body_content = if let Some(body_start) = html.find("<body>") {
        let after_body = &html[body_start + 6..];
        if let Some(body_end) = after_body.find("</body>") {
            &after_body[..body_end]
        } else {
            &html[..] // Fallback to full html
        }
    } else {
        &html[..] // Fallback to full html
    };

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            // Escape for JS template literal
            let escaped_content = body_content
                .replace('\\', "\\\\")
                .replace('`', "\\`")
                .replace("${", "\\${");

            // Animate only NEW .word spans (markdown_to_html wraps words in <span class="word">)
            // Track previous word count, add animation only to new words
            // Animate only NEW .word spans (markdown_to_html wraps words in <span class="word">)
            // Track previous word count, add animation only to new words
            // Animate only NEW .word spans (markdown_to_html wraps words in <span class="word">)
            // Track previous word count, add animation only to new words
            // Animate only NEW .word spans (markdown_to_html wraps words in <span class="word">)
            // Track previous word count, add animation only to new words
            let script = format!(
                r#"(function() {{
    const newContent = `{}`;
    const prevWordCount = window._streamWordCount || 0;
    
    // Update content
    document.body.innerHTML = newContent;
    
    // --- INTEGRATED FONT SIZING (Heuristic Optimized) ---
    var body = document.body;
    var doc = document.documentElement;
    var winH = window.innerHeight;
    var winW = window.innerWidth;
    
    // Detect new session
    var textLen = (body.innerText || body.textContent || '').trim().length;
    var isNewSession = (!window._streamWordCount || window._streamWordCount < 5 || textLen < 50);
    
    // Dynamic Minimum Size
    // If text is short (< 200 chars), we allow shrinking to 6px to fit purely visual content.
    // If text is longer, we enforce 14px floor for readability.
    var minSize = (textLen < 200) ? 6 : 14;
    
    if (isNewSession) {{
         var maxPossible = Math.min(200, winH); 
         
         // Heuristic estimate
         var estimated = Math.sqrt((winW * winH) / (textLen + 1));
         
         var low = Math.max(minSize, Math.floor(estimated * 0.5));
         var high = Math.min(maxPossible, Math.ceil(estimated * 1.5));
         
         if (low > high) low = high;
         
         body.style.fontVariationSettings = "'wght' 400, 'wdth' 90, 'slnt' 0, 'ROND' 100";
         
         // Binary search
         var best = low;
         while (low <= high) {{
             var mid = Math.floor((low + high) / 2);
             body.style.fontSize = mid + 'px';
             if (doc.scrollHeight <= (winH + 2)) {{
                 best = mid;
                 low = mid + 1;
             }} else {{
                 high = mid - 1;
             }}
         }}
         // Enforce floor
         if (best < minSize) best = minSize;
         body.style.fontSize = best + 'px';
         
    }} else {{
        // Incrementally adjust font size if overflow occurs (Heuristic skip for speed)
        var hasOverflow = doc.scrollHeight > (winH + 2);
        if (hasOverflow) {{
            var currentSize = parseFloat(body.style.fontSize) || 14;
            if (currentSize > minSize) {{
                // Quick jump instead of loop for streaming smoothness
                body.style.fontSize = (currentSize - 1) + 'px';
            }}
        }}
    }}
    // ----------------------------
    
    // Get all word spans
    const words = document.querySelectorAll('.word');
    const newWordCount = words.length;
    
    // SKIP animation for the very first chunk (isNewSession)
    if (!isNewSession) {{
        let newWords = [];
        for (let i = prevWordCount; i < newWordCount; i++) {{
            newWords.push(words[i]);
        }}
        
        if (newWords.length > 0) {{
            // Set initial state
            newWords.forEach(w => {{
                w.style.opacity = '0';
                w.style.filter = 'blur(2px)';
            }});
            
            requestAnimationFrame(() => {{
                 newWords.forEach(w => {{
                    w.style.transition = 'opacity 0.35s ease-out, filter 0.35s ease-out';
                    w.style.opacity = '1';
                    w.style.filter = 'blur(0)';
                 }});
            }});
        }}
    }}
    
    window._streamWordCount = newWordCount;
    window.scrollTo({{ top: document.body.scrollHeight, behavior: 'smooth' }});
}})()"#,
                escaped_content
            );
            let _ = webview.evaluate_script(&script);
            return true;
        }
        false
    })
}

/// Reset the stream content tracker (call when streaming ends)
/// This ensures the next streaming session starts fresh
pub fn reset_stream_counter(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            // Reset stream counters only - font will be reset at start of next session
            let _ = webview.evaluate_script(
                "window._streamPrevLen = 0; window._streamPrevContent = ''; window._streamWordCount = 0;"
            );
        }
    });
}

/// Fit font size to window - call after streaming ends or on content update
/// This runs a ONE-TIME font fit calculation (no loops, no observers, safe)
/// Scales font UP if there's unfilled space, scales DOWN if overflow (but never below 8px)
/// Also adjusts font width (wdth) to prevent text wrapping when possible
pub fn fit_font_to_window(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    // One-time font fitting script - runs once, no loops, no observers
    // 1. First fits font size (8px-32px)
    // 2. If text would wrap (single line wider than container), condense wdth
    let script = r#"
    (function() {
        if (window._sgtFitting) return;
        window._sgtFitting = true;
        
        setTimeout(function() {
            var body = document.body;
            var doc = document.documentElement;
            var winH = window.innerHeight;
            var winW = body.clientWidth || window.innerWidth;
            
            // Get content and length
            var text = body.innerText || body.textContent || '';
            var textLen = text.trim().length;
            
            // Unified fitting logic
            // 1. If text is extremely short (< 50 chars), allow massive scaling (up to 200px)
            // 2. If text is "short" (< 1500 chars), allow shrinking down to 6px to fit
            // 3. If text is long, use default 14px and scroll
            
            var isShortContent = textLen < 1500;
            var isTinyContent = textLen < 50;
            
            // Allowed ranges
            var minSize = isShortContent ? 6 : 14; 
            var maxSize = isTinyContent ? 200 : (isShortContent ? 32 : 16);
            
            // Reset to baseline for measurement - VERY IMPORTANT: clears previous scaling to prevent "sticky" spacing
            body.style.fontVariationSettings = "'wght' 400, 'wdth' 90, 'slnt' 0, 'ROND' 100";
            body.style.letterSpacing = '0px';
            body.style.lineHeight = '1.5';
            var resetBlocks = body.querySelectorAll('p, h1, h2, h3, li, blockquote');
            for (var i = 0; i < resetBlocks.length; i++) {
                resetBlocks[i].style.marginBottom = '';
            }
            var startSize = parseFloat(window.getComputedStyle(body).fontSize) || 14;
            
            // Check overflow at current size
            var hasOverflow = doc.scrollHeight > (winH + 2);
            
            if (hasOverflow && isShortContent) {
                // Shrink Mode: Binary search [minSize, currentSize]
                var low = minSize;
                var high = Math.max(minSize, startSize);
                var best = minSize;
                
                // If even current size is too big, start search
                while (low <= high) {
                    var mid = Math.floor((low + high) / 2);
                    body.style.fontSize = mid + 'px';
                    if (doc.scrollHeight <= (winH + 2)) {
                        best = mid;
                        // Try to find a larger size that still fits
                        low = mid + 1;
                    } else {
                        high = mid - 1;
                    }
                }
                body.style.fontSize = best + 'px';
                
            } else if (!hasOverflow) {
                // Grow Mode: Binary search [currentSize, maxSize]
                var low = startSize;
                var high = maxSize;
                var best = startSize;
                
                while (low <= high) {
                    var mid = Math.floor((low + high) / 2);
                    body.style.fontSize = mid + 'px';
                    if (doc.scrollHeight <= (winH + 2)) {
                        best = mid;
                        low = mid + 1;
                    } else {
                        high = mid - 1;
                    }
                }
                body.style.fontSize = best + 'px';
            }
            
            // NEW: Adaptive Horizontal Pass (wdth + letterSpacing)
            // After settling font size, determine best width properties to fill container OR fix overflow
            var initialHeight = doc.scrollHeight;
            var isOverflowing = initialHeight > (winH + 2);
            
            if (isShortContent) {
                if (isOverflowing) {
                    // --- SHRINK MODE ---
                    // 1. First try condensing 'wdth' (90 -> 50)
                    var lowW = 50, highW = 90, bestW = 90;
                    while (lowW <= highW) {
                        var midW = Math.floor((lowW + highW) / 2);
                        body.style.fontVariationSettings = "'wght' 400, 'wdth' " + midW + ", 'slnt' 0, 'ROND' 100";
                        if (doc.scrollHeight <= (winH + 2)) {
                            bestW = midW;
                            highW = midW - 1;
                        } else {
                            lowW = midW + 1;
                        }
                    }
                    body.style.fontVariationSettings = "'wght' 400, 'wdth' " + bestW + ", 'slnt' 0, 'ROND' 100";
                    
                    // 2. If STILL overflowing, try condensing letter-spacing (0 -> -2px)
                    if (doc.scrollHeight > (winH + 2)) {
                        var lowL = -2.0, highL = 0, bestL = 0;
                        while (highL - lowL > 0.05) {
                            var midL = (lowL + highL) / 2;
                            body.style.letterSpacing = midL + 'px';
                            if (doc.scrollHeight <= (winH + 2)) {
                                bestL = midL;
                                lowL = midL;
                            } else {
                                highL = midL;
                            }
                        }
                        body.style.letterSpacing = bestL + 'px';
                    }
                } else {
                    // --- STRETCH MODE ---
                    // 1. Try stretching 'wdth' (90 -> 1000)
                    var lowW = 90, highW = 1000, bestW = 90;
                    while (lowW <= highW) {
                        var midW = Math.floor((lowW + highW) / 2);
                        body.style.fontVariationSettings = "'wght' 400, 'wdth' " + midW + ", 'slnt' 0, 'ROND' 100";
                        if (doc.scrollHeight <= initialHeight) {
                            bestW = midW;
                            lowW = midW + 1;
                        } else {
                            highW = midW - 1;
                        }
                    }
                    body.style.fontVariationSettings = "'wght' 400, 'wdth' " + bestW + ", 'slnt' 0, 'ROND' 100";

                    // 2. Try stretching letter-spacing (0 -> 500px)
                    var lowL = 0, highL = 500, bestL = 0;
                    while (highL - lowL > 0.1) {
                        var midL = (lowL + highL) / 2;
                        body.style.letterSpacing = midL + 'px';
                        if (doc.scrollHeight <= initialHeight) {
                            bestL = midL;
                            lowL = midL;
                        } else {
                            highL = midL;
                        }
                    }
                    body.style.letterSpacing = bestL + 'px';
                }
            }
            
            // NEW: Line Height AND Vertical Spacing Scaling
            // If we have substantial vertical space remaining, increase line-height and margins to fill the window
            var finalHeight = doc.scrollHeight;
            var finalOverflow = finalHeight > (winH + 2);
            if (!finalOverflow && finalHeight < winH * 0.95) {
                var lowScale = 1.0;
                var highScale = 2.0; // Max scale factor for spacing 
                var bestScale = 1.0;
                
                while (highScale - lowScale > 0.05) {
                    var midScale = (lowScale + highScale) / 2;
                    body.style.lineHeight = (1.5 * midScale);
                    
                    // Also adjust margins of block elements for better distribution
                    var blocks = body.querySelectorAll('p, h1, h2, h3, li, blockquote');
                    for (var i = 0; i < blocks.length; i++) {
                        blocks[i].style.marginBottom = (0.8 * (midScale - 1)) + 'em';
                    }
                    
                    if (doc.scrollHeight <= (winH + 2)) {
                        bestScale = midScale;
                        lowScale = midScale;
                    } else {
                        highScale = midScale;
                    }
                }
                
                body.style.lineHeight = (1.5 * bestScale);
                var blocks = body.querySelectorAll('p, h1, h2, h3, li, blockquote');
                for (var i = 0; i < blocks.length; i++) {
                    blocks[i].style.marginBottom = (0.8 * (bestScale - 1)) + 'em';
                }
            }
            
            window._sgtFitting = false;
        }, 50);
    })();
    "#;

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            let _ = webview.evaluate_script(script);
        }
    });
}

/// Trigger Grid.js initialization on any tables in the WebView
/// Call this after streaming ends to convert tables to interactive Grid.js tables
pub fn init_gridjs(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;

    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            // Trigger the table initialization via the MutationObserver's mechanism
            // The observer watches for DOM changes and schedules initGridJs via window.gridJsTimeout
            // We can simulate this by triggering a DOM change or directly calling the init logic
            let script = r#"
                (function() {
                    if (typeof gridjs === 'undefined') return;
                    
                    var tables = document.querySelectorAll('table:not(.gridjs-table):not([data-processed-table="true"])');
                    for (var i = 0; i < tables.length; i++) {
                        var table = tables[i];
                        if (table.closest('.gridjs-container') || table.closest('.gridjs-injected-wrapper')) continue;
                        
                        table.setAttribute('data-processed-table', 'true');
                        
                        var wrapper = document.createElement('div');
                        wrapper.className = 'gridjs-injected-wrapper';
                        table.parentNode.insertBefore(wrapper, table);
                        
                        try {
                            var grid = new gridjs.Grid({
                                from: table,
                                sort: true,
                                fixedHeader: true,
                                search: false,
                                resizable: false,
                                autoWidth: false,
                                style: {
                                    table: { 'width': '100%' },
                                    td: { 'border': '1px solid #333' },
                                    th: { 'border': '1px solid #333' }
                                },
                                className: {
                                    table: 'gridjs-table-premium',
                                    th: 'gridjs-th-premium',
                                    td: 'gridjs-td-premium'
                                }
                            });
                            grid.on('ready', function() {
                                table.classList.add('gridjs-hidden-source');
                            });
                            grid.render(wrapper);
                        } catch (e) {
                            console.error('Grid.js streaming init error:', e);
                            if(wrapper.parentNode) wrapper.parentNode.removeChild(wrapper);
                        }
                    }
                })();
            "#;
            let _ = webview.evaluate_script(script);
        }
    });
}

/// Resize the WebView to match parent window
/// When hovered: leaves 52px at bottom for buttons
/// When not hovered: expands to full height for clean view
/// When refine input active: starts 44px from top (40px input + 4px gap)
pub fn resize_markdown_webview(parent_hwnd: HWND, _is_hovered: bool) {
    let hwnd_key = parent_hwnd.0 as isize;

    // Check if refine input is active
    let refine_input_active = super::refine_input::is_refine_input_active(parent_hwnd);
    let top_offset = if refine_input_active { 44.0 } else { 2.0 }; // 40px input + 4px gap, or 2px edge margin

    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(parent_hwnd, &mut rect);

        // Edge margins: 4px left/right for resize handles, 2px top/bottom
        let margin_x = 4.0;
        let margin_y = 2.0;
        // Always use margin_y, as buttons are now floating on a separate canvas
        let button_area_height = margin_y;

        let content_width = ((rect.right - rect.left) as f64 - margin_x * 2.0).max(50.0);
        let content_height =
            ((rect.bottom - rect.top) as f64 - top_offset - button_area_height).max(0.0);

        WEBVIEWS.with(|webviews| {
            if let Some(webview) = webviews.borrow().get(&hwnd_key) {
                // Use Physical coordinates since GetClientRect returns physical pixels
                let _ = webview.set_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                        margin_x as i32,
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

/// Generate a filename using Cerebras' gpt-oss-120b model
fn generate_filename(content: &str) -> String {
    let default_name = "game.html".to_string();

    // Get API Key
    let cerebras_key = if let Ok(app) = crate::APP.lock() {
        app.config.cerebras_api_key.clone()
    } else {
        return default_name;
    };

    if cerebras_key.is_empty() {
        return default_name;
    }

    // Truncate content to avoid token limits (first 4000 chars should be enough for context)
    let prompt_content = if content.len() > 4000 {
        &content[..4000]
    } else {
        content
    };

    let prompt = format!(
        "Generate a short, kebab-case filename (without extension) for the following content. \
        Do NOT include 'html' in the name. \
        The filename must be descriptive but concise (max 5 words). \
        Output ONLY the filename, nothing else. No markdown, no quotes, no explanations.\n\nContent:\n{}",
        prompt_content
    );

    let payload = serde_json::json!({
        "model": "gpt-oss-120b",
        "messages": [
            { "role": "user", "content": prompt }
        ],
        "temperature": 0.3,
        "max_tokens": 60
    });

    match crate::api::client::UREQ_AGENT
        .post("https://api.cerebras.ai/v1/chat/completions")
        .header("Authorization", &format!("Bearer {}", cerebras_key))
        .send_json(payload)
    {
        Ok(resp) => {
            if let Ok(json) = resp.into_body().read_json::<serde_json::Value>() {
                if let Some(choice) = json
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.first())
                {
                    if let Some(content) = choice
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|s| s.as_str())
                    {
                        let mut name = content.trim().to_string();

                        // Clean up quotes/markdown
                        name = name.replace('"', "").replace('\'', "").replace('`', "");

                        // Remove potential .html extension if the model disobeyed
                        if name.to_lowercase().ends_with(".html") {
                            name = name[..name.len() - 5].to_string();
                        }

                        // Remove trailing -html or _html if present to avoid redundancy
                        if name.to_lowercase().ends_with("-html") {
                            name = name[..name.len() - 5].to_string();
                        } else if name.to_lowercase().ends_with("_html") {
                            name = name[..name.len() - 5].to_string();
                        }

                        // Basic validation: remove invalid characters for Windows filenames
                        let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
                        name = name
                            .chars()
                            .filter(|c| !invalid_chars.contains(c))
                            .collect();

                        if name.is_empty() {
                            return default_name;
                        }

                        // Always append .html
                        name.push_str(".html");

                        return name;
                    }
                }
            }
            default_name
        }
        Err(e) => {
            eprintln!("Failed to generate filename: {}", e);
            default_name
        }
    }
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
        let filename = generate_filename(markdown_text);
        let default_name: Vec<u16> = OsStr::new(&filename)
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

/// Handle IPC messages from markdown WebView
pub fn handle_markdown_ipc(hwnd: HWND, msg: &str) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(msg) {
        if let Some(action) = json.get("action").and_then(|s| s.as_str()) {
            match action {
                "copy" => {
                    crate::overlay::result::trigger_copy(hwnd);
                }
                "close" | "broom_click" => unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                },
                "broom_drag_start" => {
                    unsafe {
                        // Native Window Drag
                        // ReleaseCapture required before SC_MOVE
                        use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
                        let _ = ReleaseCapture();
                        let _ = PostMessageW(
                            Some(hwnd),
                            WM_SYSCOMMAND,
                            WPARAM(0xF012), // SC_MOVE (0xF010) + HTCAPTION (2)
                            LPARAM(0),
                        );
                    }
                }
                _ => {}
            }
        }
    }
}
