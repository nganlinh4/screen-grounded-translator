use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::sync::{Once, Mutex};
use std::num::NonZeroIsize;
use std::cell::RefCell;
use crate::gui::locale::LocaleText;
use crate::APP;
use wry::{WebViewBuilder, Rect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle, Win32WindowHandle, HandleError};

static REGISTER_INPUT_CLASS: Once = Once::new();
static mut INPUT_HWND: HWND = HWND(0);

// Static storage for i18n and display state
// (Replaced by Mutex config below)

// Dragging State (Screen Coordinates)
static mut IS_DRAGGING: bool = false;
static mut DRAG_START_MOUSE: POINT = POINT { x: 0, y: 0 };
static mut DRAG_START_WIN_POS: POINT = POINT { x: 0, y: 0 };

// Callback storage
type SubmitCallback = Box<dyn Fn(String, HWND) + Send>;

// Colors
const COL_DARK_BG: u32 = 0x202020; // RGB(32, 32, 32)

// Global storage for submitted text (from webview IPC)
lazy_static::lazy_static! {
    static ref SUBMITTED_TEXT: Mutex<Option<String>> = Mutex::new(None);
    static ref SHOULD_CLOSE: Mutex<bool> = Mutex::new(false);
    static ref SHOULD_CLEAR_ONLY: Mutex<bool> = Mutex::new(false);
    
    // Config Storage (Thread-safe for persistent window)
    static ref CFG_TITLE: Mutex<String> = Mutex::new(String::new());
    static ref CFG_LANG: Mutex<String> = Mutex::new(String::new());
    static ref CFG_CANCEL: Mutex<String> = Mutex::new(String::new());
    static ref CFG_CALLBACK: Mutex<Option<Box<dyn Fn(String, HWND) + Send>>> = Mutex::new(None);
    static ref CFG_CONTINUOUS: Mutex<bool> = Mutex::new(false);
    
    // Cross-thread text injection (for auto-paste from transcription)
    static ref PENDING_TEXT: Mutex<Option<String>> = Mutex::new(None);
}

const WM_APP_SHOW: u32 = WM_USER + 99;
const WM_APP_SET_TEXT: u32 = WM_USER + 100; // New: trigger text injection from other threads

// Thread-local storage for WebView (not Send)
thread_local! {
    static TEXT_INPUT_WEBVIEW: RefCell<Option<wry::WebView>> = RefCell::new(None);
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

/// CSS for the modern text input editor
fn get_editor_css() -> &'static str {
    r#"
    * { box-sizing: border-box; margin: 0; padding: 0; }
    
    html, body {
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: #F0F0F0;
        font-family: 'Segoe UI', -apple-system, BlinkMacSystemFont, sans-serif;
    }
    
    .editor-container {
        width: 100%;
        height: 100%;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        background: linear-gradient(180deg, #FAFAFA 0%, #F0F0F0 100%);
        position: relative;
    }


    
    #editor {
        flex: 1;
        width: 100%;
        padding: 12px 14px;
        padding-right: 50px; /* Space for mic button */
        border: none;
        outline: none;
        resize: none;
        font-family: 'Segoe UI', -apple-system, BlinkMacSystemFont, sans-serif;
        font-size: 15px;
        line-height: 1.55;
        color: #1a1a1a;
        background: transparent;
        overflow-y: auto;
    }
    
    #editor::placeholder {
        color: #888;
        opacity: 1;
    }
    
    #editor:focus {
        outline: none;
    }
    
    /* Modern scrollbar */
    #editor::-webkit-scrollbar {
        width: 6px;
    }
    #editor::-webkit-scrollbar-track {
        background: transparent;
    }
    #editor::-webkit-scrollbar-thumb {
        background: #ccc;
        border-radius: 3px;
    }
    #editor::-webkit-scrollbar-thumb:hover {
        background: #aaa;
    }
    
    /* Character counter */
    .char-counter {
        position: absolute;
        bottom: 6px;
        right: 10px;
        font-size: 11px;
        color: #999;
        pointer-events: none;
    }
    
    /* Floating Mic Button */
    .mic-btn {
        position: absolute;
        right: 10px;
        top: 50%;
        transform: translateY(-50%);
        width: 36px;
        height: 36px;
        border-radius: 50%;
        border: none;
        background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        box-shadow: 0 2px 8px rgba(102, 126, 234, 0.4);
        transition: all 0.2s ease;
        z-index: 10;
    }
    
    .mic-btn:hover {
        transform: translateY(-50%) scale(1.08);
        box-shadow: 0 4px 12px rgba(102, 126, 234, 0.5);
    }
    
    .mic-btn:active {
        transform: translateY(-50%) scale(0.95);
    }
    
    .mic-btn svg {
        width: 18px;
        height: 18px;
        fill: white;
    }
    "#
}

/// Generate HTML for the text input webview
fn get_editor_html(placeholder: &str) -> String {
    let css = get_editor_css();
    let escaped_placeholder = placeholder
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    
    format!(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>{css}</style>
</head>
<body>
    <div class="editor-container">
        <textarea id="editor" placeholder="{escaped_placeholder}" autofocus></textarea>
        <button class="mic-btn" id="micBtn" title="Speech to text">
            <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3z"/>
                <path d="M17 11c0 2.76-2.24 5-5 5s-5-2.24-5-5H5c0 3.53 2.61 6.43 6 6.92V21h2v-3.08c3.39-.49 6-3.39 6-6.92h-2z"/>
            </svg>
        </button>
    </div>
    <script>
        const editor = document.getElementById('editor');
        const micBtn = document.getElementById('micBtn');
        
        // Auto focus on load
        window.onload = () => {{
            setTimeout(() => editor.focus(), 50);
        }};
        
        // Handle keyboard events
        editor.addEventListener('keydown', (e) => {{
            // Enter without Shift = Submit
            if (e.key === 'Enter' && !e.shiftKey) {{
                e.preventDefault();
                const text = editor.value.trim();
                if (text) {{
                    window.ipc.postMessage('submit:' + text);
                }}
            }}
            
            // Escape = Cancel
            if (e.key === 'Escape') {{
                e.preventDefault();
                window.ipc.postMessage('cancel');
            }}
        }});
        
        // Mic button click
        micBtn.addEventListener('click', (e) => {{
            e.preventDefault();
            window.ipc.postMessage('mic');
        }});
        
        // Prevent context menu
        document.addEventListener('contextmenu', e => e.preventDefault());
    </script>
</body>
</html>"#)
}

pub fn is_active() -> bool {
    unsafe { INPUT_HWND.0 != 0 && IsWindowVisible(INPUT_HWND).as_bool() }
}

pub fn cancel_input() {
    unsafe {
        if INPUT_HWND.0 != 0 {
            // Just hide the window, don't destroy
            ShowWindow(INPUT_HWND, SW_HIDE);
        }
    }
}

/// Get the edit control HWND of the active text input window
/// For webview-based input, this returns None as there's no native edit control
pub fn get_input_edit_hwnd() -> Option<HWND> {
    // Webview-based input doesn't expose a native HWND for the editor
    // Pasting is handled via JavaScript in the webview
    None
}

/// Set text content in the webview editor (for paste operations)
/// This is thread-safe and can be called from any thread
pub fn set_editor_text(text: &str) {
    unsafe {
        // Store the text in the mutex
        *PENDING_TEXT.lock().unwrap() = Some(text.to_string());
        
        // Post message to the text input window to trigger the injection
        if INPUT_HWND.0 != 0 {
            PostMessageW(INPUT_HWND, WM_APP_SET_TEXT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Internal function to apply pending text (called on the window's thread)
fn apply_pending_text() {
    let text = PENDING_TEXT.lock().unwrap().take();
    if let Some(text) = text {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace("${", "\\${")
            .replace('\n', "\\n")
            .replace('\r', "");
        
        TEXT_INPUT_WEBVIEW.with(|webview| {
            if let Some(wv) = webview.borrow().as_ref() {
                let script = format!(
                    r#"document.getElementById('editor').value = `{}`; document.getElementById('editor').focus();"#,
                    escaped
                );
                let _ = wv.evaluate_script(&script);
            }
        });
    }
}

/// Clear the webview editor content and refocus (for continuous input mode)
pub fn clear_editor_text() {
    TEXT_INPUT_WEBVIEW.with(|webview| {
        if let Some(wv) = webview.borrow().as_ref() {
            let script = r#"document.getElementById('editor').value = ''; document.getElementById('editor').focus();"#;
            let _ = wv.evaluate_script(script);
        }
    });
}

/// Update the UI text (header) and trigger a repaint
pub fn update_ui_text(header_text: String) {
    unsafe {
        if INPUT_HWND.0 != 0 {
            *CFG_TITLE.lock().unwrap() = header_text;
            InvalidateRect(INPUT_HWND, None, true);
        }
    }
}

/// Bring the text input window to foreground and focus the editor
/// Call this after closing modal windows like the preset wheel
pub fn refocus_editor() {
    unsafe {
        if INPUT_HWND.0 != 0 {
            use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, BringWindowToTop, SetTimer};
            use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
            
            // Aggressive focus: try multiple methods
            BringWindowToTop(INPUT_HWND);
            SetForegroundWindow(INPUT_HWND);
            SetFocus(INPUT_HWND);
            
            // Focus the webview editor immediately
            TEXT_INPUT_WEBVIEW.with(|webview| {
                if let Some(wv) = webview.borrow().as_ref() {
                    // First focus the WebView itself (native focus)
                    let _ = wv.focus();
                    // Then focus the textarea inside via JavaScript
                    let _ = wv.evaluate_script("document.getElementById('editor').focus();");
                }
            });
            
            // Schedule another focus attempt after 200ms via timer ID 3
            // This will be handled in WM_TIMER in the same thread
            SetTimer(INPUT_HWND, 3, 200, None);
        }
    }
}

/// Get the current window rect of the text input window (if active)
pub fn get_window_rect() -> Option<RECT> {
    unsafe {
        if INPUT_HWND.0 != 0 {
            let mut rect = RECT::default();
            if GetWindowRect(INPUT_HWND, &mut rect).as_bool() {
                return Some(rect);
            }
        }
    }
    None
}

/// Start the persistent hidden window (called from main)
pub fn warmup() {
    std::thread::spawn(|| {
        internal_create_window_loop();
    });
}

pub fn show(
    prompt_guide: String,
    ui_language: String,
    cancel_hotkey_name: String,
    continuous_mode: bool,
    on_submit: impl Fn(String, HWND) + Send + 'static
) {
    unsafe {
        // Update shared state
        *CFG_TITLE.lock().unwrap() = prompt_guide;
        *CFG_LANG.lock().unwrap() = ui_language;
        *CFG_CANCEL.lock().unwrap() = cancel_hotkey_name;
        *CFG_CONTINUOUS.lock().unwrap() = continuous_mode;
        *CFG_CALLBACK.lock().unwrap() = Some(Box::new(on_submit));
        
        *SUBMITTED_TEXT.lock().unwrap() = None;
        *SHOULD_CLOSE.lock().unwrap() = false;
        *SHOULD_CLEAR_ONLY.lock().unwrap() = false;

        if INPUT_HWND.0 != 0 {
            // Window exists, wake it up
            PostMessageW(INPUT_HWND, WM_APP_SHOW, WPARAM(0), LPARAM(0));
        } else {
            // Fallback (should normally be warmed up)
            warmup();
            // Sleep a bit and retry (simple handling for race on first cold start)
            std::thread::sleep(std::time::Duration::from_millis(100));
            if INPUT_HWND.0 != 0 {
                 PostMessageW(INPUT_HWND, WM_APP_SHOW, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn internal_create_window_loop() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextInputWry");

        REGISTER_INPUT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(input_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(0);
            let _ = RegisterClassW(&wc);
        });

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let win_w = 600;
        let win_h = 250;
        let x = (screen_w - win_w) / 2;
        let y = (screen_h - win_h) / 2;

        // Start HIDDEN logic
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("Text Input"),
            WS_POPUP, // Start invisible (not WS_VISIBLE)
            x, y, win_w, win_h,
            None, None, instance, None
        );

        INPUT_HWND = hwnd;

        // Initialize Layered (Transparent)
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

        // Window Region (Rounded)
        let rgn = CreateRoundRectRgn(0, 0, win_w, win_h, 16, 16);
        SetWindowRgn(hwnd, rgn, true);

        // Create webview
        init_webview(hwnd, win_w, win_h);
        
        // Message Loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup on exit
        TEXT_INPUT_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = None;
        });
        INPUT_HWND = HWND(0);
    }
}

unsafe fn init_webview(hwnd: HWND, w: i32, h: i32) {
        let edit_x = 20;
        let edit_y = 50;
        let edit_w = w - 40;
        let edit_h = h - 90;
        let corner_inset = 6;
        let webview_x = edit_x + corner_inset;
        let webview_y = edit_y + corner_inset;
        let webview_w = edit_w - (corner_inset * 2);
        let webview_h = edit_h - (corner_inset * 2);
        
        let placeholder = "Ready..."; 
        let html = get_editor_html(placeholder);
        let wrapper = HwndWrapper(hwnd);
        
        let result = WebViewBuilder::new()
            .with_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(webview_x, webview_y)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(webview_w as u32, webview_h as u32)),
            })
            .with_html(&html)
            .with_transparent(false)
            .with_ipc_handler(move |msg: wry::http::Request<String>| {
                let body = msg.body();
                if body.starts_with("submit:") {
                    let text = body.strip_prefix("submit:").unwrap_or("").to_string();
                    if !text.trim().is_empty() {
                        *SUBMITTED_TEXT.lock().unwrap() = Some(text);
                        *SHOULD_CLOSE.lock().unwrap() = true;
                    }
                } else if body == "cancel" {
                    *SHOULD_CLOSE.lock().unwrap() = true;
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
            })
            .build_as_child(&wrapper);
        
        if let Ok(webview) = result {
             TEXT_INPUT_WEBVIEW.with(|wv| {
                *wv.borrow_mut() = Some(webview);
            });
        }
}

unsafe extern "system" fn input_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // State variables for this window instance
    static mut FADE_ALPHA: i32 = 0;
    // IS_DRAGGING is no longer needed with native drag

    match msg {
        WM_APP_SHOW => {
            // Reset state
            FADE_ALPHA = 0;
            
            // Get current config
            let prompt_guide = CFG_TITLE.lock().unwrap().clone();
            let ui_language = CFG_LANG.lock().unwrap().clone();

            // Update window title
            SetWindowTextW(hwnd, &HSTRING::from(prompt_guide));

            // Update webview placeholder and clear text (but NOT focus yet - window not visible)
            let locale = LocaleText::get(&ui_language);
            let placeholder = locale.text_input_placeholder.to_string();
            TEXT_INPUT_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                     let script = format!(
                         "document.getElementById('editor').placeholder = '{}'; document.getElementById('editor').value = '';",
                         placeholder.replace("'", "\\'")
                     );
                     let _ = webview.evaluate_script(&script);
                }
            });
            
            // RE-CENTER WINDOW
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect);
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            let x = (screen_w - w) / 2;
            let y = (screen_h - h) / 2;
            SetWindowPos(hwnd, HWND(0), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            
            // Reset alpha to 0 before show
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
            
            // Show and bring to front
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd); // CRITICAL: Set keyboard focus to window
            UpdateWindow(hwnd);
            
            // Start Fade Timer
            SetTimer(hwnd, 1, 16, None);
            
            // IPC check timer
            SetTimer(hwnd, 2, 50, None);

            LRESULT(0)
        }

        WM_APP_SET_TEXT => {
            // Apply pending text from cross-thread call
            apply_pending_text();
            LRESULT(0)
        }

        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            KillTimer(hwnd, 1);
            KillTimer(hwnd, 2);
            KillTimer(hwnd, 3);
            LRESULT(0)
        }

        WM_ERASEBKGND => LRESULT(1),

        WM_TIMER => {
            if wparam.0 == 1 { 
                // Fade In Logic
                if FADE_ALPHA < 245 {
                    FADE_ALPHA += 25;
                    if FADE_ALPHA > 245 { FADE_ALPHA = 245; }
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), FADE_ALPHA as u8, LWA_ALPHA);
                } else {
                    KillTimer(hwnd, 1);
                    
                    // CRITICAL: Focus the editor AFTER fade completes (window fully visible)
                    // WebView2 won't accept focus properly if window is transparent
                    SetForegroundWindow(hwnd);
                    SetFocus(hwnd);
                    TEXT_INPUT_WEBVIEW.with(|webview| {
                        if let Some(wv) = webview.borrow().as_ref() {
                            // First focus the WebView itself (native focus)
                            let _ = wv.focus();
                            // Then focus the textarea inside via JavaScript
                            let _ = wv.evaluate_script("document.getElementById('editor').focus();");
                        }
                    });
                }
            }
            
            if wparam.0 == 2 {
                // IPC messages
                let should_close = *SHOULD_CLOSE.lock().unwrap();
                if should_close {
                    *SHOULD_CLOSE.lock().unwrap() = false;
                    let submitted = SUBMITTED_TEXT.lock().unwrap().take();
                    if let Some(text) = submitted {
                        let continuous = *CFG_CONTINUOUS.lock().unwrap();
                        if continuous {
                            let cb_lock = CFG_CALLBACK.lock().unwrap();
                            if let Some(cb) = cb_lock.as_ref() { cb(text, hwnd); }
                            clear_editor_text();
                        } else {
                            ShowWindow(hwnd, SW_HIDE);
                            let cb_lock = CFG_CALLBACK.lock().unwrap();
                            if let Some(cb) = cb_lock.as_ref() { cb(text, hwnd); }
                        }
                    } else {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
            // Timer 3: focus logic (used by refocus_editor after preset wheel)
            if wparam.0 == 3 {
                KillTimer(hwnd, 3);
                TEXT_INPUT_WEBVIEW.with(|webview| {
                    if let Some(wv) = webview.borrow().as_ref() {
                        let _ = wv.focus();
                        let _ = wv.evaluate_script("document.getElementById('editor').focus();");
                    }
                });
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let w = rect.right;
            
            // Close Button
            let close_x = w - 30;
            let close_y = 20;
            if (x - close_x).abs() < 15 && (y - close_y).abs() < 15 {
                 PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                 return LRESULT(0);
            }

            // Title Bar Drag - Use Native Drag (Fix drifting issues)
            if y < 50 {
                ReleaseCapture();
                SendMessageW(hwnd, WM_SYSCOMMAND, WPARAM(0xF012), LPARAM(0));
                return LRESULT(0);
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            // WM_MOUSEMOVE drag logic removed in favor of native drag
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            // No capture cleanup needed for native drag
            LRESULT(0)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let w = rect.right;
            let h = rect.bottom;

            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bmp = CreateCompatibleBitmap(hdc, w, h);
            let old_bmp = SelectObject(mem_dc, mem_bmp);

            // 1. Draw Background (Dark)
            let brush_bg = CreateSolidBrush(COLORREF(COL_DARK_BG));
            FillRect(mem_dc, &rect, brush_bg);
            DeleteObject(brush_bg);

            // 2. Draw white rounded rectangle
            let edit_x = 20;
            let edit_y = 50;
            let edit_w = w - 40;
            let edit_h = h - 90;
            let corner_radius = 12.0f32;
            let fill_color: u32 = 0xF0F0F0;
            
            let cx = (edit_w as f32) / 2.0;
            let cy = (edit_h as f32) / 2.0;
            let half_w = cx;
            let half_h = cy;
            
            for py_local in 0..edit_h {
                for px_local in 0..edit_w {
                    let px_screen = edit_x + px_local;
                    let py_screen = edit_y + py_local;
                    let px_rel = (px_local as f32) - cx;
                    let py_rel = (py_local as f32) - cy;
                    let d = crate::overlay::paint_utils::sd_rounded_box(px_rel, py_rel, half_w, half_h, corner_radius);
                    
                    if d < -1.0 {
                        SetPixel(mem_dc, px_screen, py_screen, COLORREF(fill_color));
                    } else if d < 1.0 {
                        let t = (d + 1.0) / 2.0;
                        let alpha = 1.0 - t * t * (3.0 - 2.0 * t);
                        if alpha > 0.01 {
                            let bg_r = ((COL_DARK_BG >> 16) & 0xFF) as f32;
                            let bg_g = ((COL_DARK_BG >> 8) & 0xFF) as f32;
                            let bg_b = (COL_DARK_BG & 0xFF) as f32;
                            let fg_r = ((fill_color >> 16) & 0xFF) as f32;
                            let fg_g = ((fill_color >> 8) & 0xFF) as f32;
                            let fg_b = (fill_color & 0xFF) as f32;
                            let r = (fg_r * alpha + bg_r * (1.0 - alpha)) as u32;
                            let g = (fg_g * alpha + bg_g * (1.0 - alpha)) as u32;
                            let b = (fg_b * alpha + bg_b * (1.0 - alpha)) as u32;
                            SetPixel(mem_dc, px_screen, py_screen, COLORREF((r << 16) | (g << 8) | b));
                        }
                    }
                }
            }

            // 3. Draw Text Labels
            SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, COLORREF(0x00FFFFFF)); 
            
            let h_font = CreateFontW(19, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            let old_font = SelectObject(mem_dc, h_font);
            
            // USE NEW MUTEX CONFIG
            let title_str = CFG_TITLE.lock().unwrap().clone();
            let cur_lang = CFG_LANG.lock().unwrap().clone();
            let cur_cancel = CFG_CANCEL.lock().unwrap().clone();
            
            let locale = LocaleText::get(&cur_lang);
            let display_title = if !title_str.is_empty() { title_str } else { locale.text_input_title_default.to_string() };
            let mut title_w = crate::overlay::utils::to_wstring(&display_title);
            let mut r_title = RECT { left: 20, top: 15, right: w - 50, bottom: 45 };
            DrawTextW(mem_dc, &mut title_w, &mut r_title, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS);
            
            let h_font_small = CreateFontW(13, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            SelectObject(mem_dc, h_font_small);
            SetTextColor(mem_dc, COLORREF(0x00AAAAAA)); 
            
            let esc_text = if cur_cancel.is_empty() { "Esc".to_string() } else { format!("Esc / {}", cur_cancel) };
            let hint = format!("{}  |  {}  |  {} {}", locale.text_input_footer_submit, locale.text_input_footer_newline, esc_text, locale.text_input_footer_cancel);
            let mut hint_w = crate::overlay::utils::to_wstring(&hint);
            let mut r_hint = RECT { left: 20, top: h - 30, right: w - 20, bottom: h - 5 };
            DrawTextW(mem_dc, &mut hint_w, &mut r_hint, DT_CENTER | DT_SINGLELINE);

            SelectObject(mem_dc, old_font);
            DeleteObject(h_font);
            DeleteObject(h_font_small);

            // 4. Draw Close Button 'X'
            let c_cx = w - 30;
            let c_cy = 20;
            let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00AAAAAA));
            let old_pen = SelectObject(mem_dc, pen);
            MoveToEx(mem_dc, c_cx - 5, c_cy - 5, None);
            LineTo(mem_dc, c_cx + 5, c_cy + 5);
            MoveToEx(mem_dc, c_cx + 5, c_cy - 5, None);
            LineTo(mem_dc, c_cx - 5, c_cy + 5);
            SelectObject(mem_dc, old_pen);
            DeleteObject(pen);

            // Final Blit
            BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
            SelectObject(mem_dc, old_bmp);
            DeleteObject(mem_bmp);
            DeleteDC(mem_dc);
            
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
