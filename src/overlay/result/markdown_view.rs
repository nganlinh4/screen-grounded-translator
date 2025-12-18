use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::HiDpi::*;
use std::sync::{Mutex, Once};
use std::collections::HashMap;
use std::num::NonZeroIsize;
use pulldown_cmark::{Parser, Options, html};
use wry::{WebViewBuilder, Rect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle, Win32WindowHandle, HandleError};
use windows::core::w;

lazy_static::lazy_static! {
    // Store WebViews per parent window - wrapped in thread-local storage to avoid Send issues
    static ref WEBVIEW_STATES: Mutex<HashMap<isize, bool>> = Mutex::new(HashMap::new());
    // Global flag to indicate WebView2 is ready
    static ref WEBVIEW_READY: Mutex<bool> = Mutex::new(false);
}

// Global hidden window handle for WebView warmup
static mut WARMUP_HWND: HWND = HWND(0);
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
        let hwnd = self.0.0 as isize;
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
            wc.hInstance = instance;
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(0);
            let _ = RegisterClassW(&wc);
        });

        // Create a small hidden window
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("MarkdownWarmup"),
            WS_POPUP,
            0, 0, 100, 100,
            None, None, instance, None
        );

        WARMUP_HWND = hwnd;
        
        // Make it transparent (invisible)
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

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
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn warmup_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// CSS styling for the markdown content
const MARKDOWN_CSS: &str = r#"
    * { box-sizing: border-box; }
    body { 
        font-family: 'Segoe UI', -apple-system, sans-serif;
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
    h1 { font-size: 1.8em; color: #4fc3f7; border-bottom: 1px solid #333; padding-bottom: 8px; margin-top: 0; }
    h2 { font-size: 1.5em; color: #81d4fa; border-bottom: 1px solid #2a2a2a; padding-bottom: 6px; margin-top: 0.5em; }
    h3 { font-size: 1.2em; color: #b3e5fc; margin-top: 0.5em; }
    h4, h5, h6 { color: #e1f5fe; margin-top: 0.5em; }
    p { margin: 0.5em 0; }
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
    th { background: #252525; color: #81d4fa; }
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
    (trimmed.contains("<head") && trimmed.contains("</head>"))
}

/// Convert markdown text to styled HTML, or pass through raw HTML
pub fn markdown_to_html(markdown: &str) -> String {
    // If input is already HTML, return it as-is
    if is_html_content(markdown) {
        return markdown.to_string();
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
    <style>{}</style>
</head>
<body>{}</body>
</html>"#,
        MARKDOWN_CSS,
        html_output
    )
}

/// Create a WebView child window for markdown rendering
/// Must be called from the main thread!
pub fn create_markdown_webview(parent_hwnd: HWND, markdown_text: &str, is_hovered: bool) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;
    
    // Check if we already have a webview
    let exists = WEBVIEWS.with(|webviews| {
        webviews.borrow().contains_key(&hwnd_key)
    });
    
    if exists {
        return update_markdown_content(parent_hwnd, markdown_text);
    }
    
    
    // Get parent window rect
    let mut rect = RECT::default();
    unsafe { GetClientRect(parent_hwnd, &mut rect); }
    
    let html_content = markdown_to_html(markdown_text);
    
    let wrapper = HwndWrapper(parent_hwnd);
    
    // Small margin on edges for resize handle accessibility (2px)
    // 52px at bottom for buttons (btn_size 28 + margin 12 * 2) if hovered
    let edge_margin = 2.0;
    let button_area_height = if is_hovered { 52.0 } else { 0.0 };
    let content_width = ((rect.right - rect.left) as f64 - edge_margin * 2.0).max(50.0);
    let content_height = ((rect.bottom - rect.top) as f64 - edge_margin - button_area_height).max(50.0);
    

    
    // Create WebView with small margins so resize handles remain accessible
    // Use Physical coordinates since GetClientRect returns physical pixels
    let hwnd_copy = parent_hwnd;
    
    // SIMPLIFIED FOR DEBUGGING - minimal WebView creation  
    // CRITICAL: with_transparent(false) matches text_input's working config
    let result = WebViewBuilder::new()
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(edge_margin as i32, edge_margin as i32)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                content_width as u32,
                content_height as u32
            )),
        })
        .with_html(&html_content)
        .with_transparent(false)
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

/// Navigate back in history
pub fn go_back(parent_hwnd: HWND) {
    let hwnd_key = parent_hwnd.0 as isize;
    
    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            // Evaluate JS to go back
            // We also need to check if we are back at the start
            // But for now, just go back. Ideally we'd hook into on_page_load
            // to reset is_browsing if we are back at local content.
            // For now, let's assume one level deep or just stay in "browsing" mode
            // until the user manually resets or closes?
            // Actually, if we inject a script to check location.href after back...
            let _ = webview.evaluate_script(r#"
                history.back();
                setTimeout(() => {
                    // Notify host if we are back at start? 
                    // This is complex without IPC setup.
                    // For now, we trust the user.
                }, 100);
            "#);
        }
    });
}

/// Update the markdown content in an existing WebView
pub fn update_markdown_content(parent_hwnd: HWND, markdown_text: &str) -> bool {
    let hwnd_key = parent_hwnd.0 as isize;
    let html = markdown_to_html(markdown_text);
    
    WEBVIEWS.with(|webviews| {
        if let Some(webview) = webviews.borrow().get(&hwnd_key) {
            // Navigate to the new HTML content using JavaScript
            let escaped_html = html.replace('\\', "\\\\")
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
pub fn resize_markdown_webview(parent_hwnd: HWND, is_hovered: bool) {
    let hwnd_key = parent_hwnd.0 as isize;
    
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(parent_hwnd, &mut rect);
        
        // 2px edge margin for resize handles
        let edge_margin = 2.0;
        // Only reserve button area when hovered
        let button_area_height = if is_hovered { 52.0 } else { edge_margin };
        
        let content_width = ((rect.right - rect.left) as f64 - edge_margin * 2.0).max(50.0);
        let content_height = ((rect.bottom - rect.top) as f64 - edge_margin - button_area_height).max(50.0);
        
        WEBVIEWS.with(|webviews| {
            if let Some(webview) = webviews.borrow().get(&hwnd_key) {
                // Use Physical coordinates since GetClientRect returns physical pixels
                let _ = webview.set_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(edge_margin as i32, edge_margin as i32)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        content_width as u32,
                        content_height as u32
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
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        IFileSaveDialog, FileSaveDialog, FOS_OVERWRITEPROMPT, FOS_STRICTFILETYPES,
        SIGDN_FILESYSPATH
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoCreateInstance, CoUninitialize, 
        CLSCTX_ALL, COINIT_APARTMENTTHREADED
    };
    use windows::core::Interface;
    
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
        let html_content = markdown_to_html(markdown_text);
        
        // Write to file
        match std::fs::write(&path_str, html_content) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

