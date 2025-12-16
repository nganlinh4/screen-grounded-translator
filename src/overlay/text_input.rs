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
use wry::{WebViewBuilder, Rect};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle, Win32WindowHandle, HandleError};

static REGISTER_INPUT_CLASS: Once = Once::new();
static mut INPUT_HWND: HWND = HWND(0);

// Static storage for i18n and display state
static mut CURRENT_UI_LANG: String = String::new();
static mut CURRENT_CANCEL_KEY: String = String::new();
static mut CURRENT_TITLE_OVERRIDE: String = String::new();
static mut FADE_ALPHA: i32 = 0;

// Dragging State (Screen Coordinates)
static mut IS_DRAGGING: bool = false;
static mut DRAG_START_MOUSE: POINT = POINT { x: 0, y: 0 };
static mut DRAG_START_WIN_POS: POINT = POINT { x: 0, y: 0 };

// Callback storage
type SubmitCallback = Box<dyn Fn(String, HWND) + Send>;
static mut ON_SUBMIT: Option<SubmitCallback> = None;

// Colors
const COL_DARK_BG: u32 = 0x202020; // RGB(32, 32, 32)

// Global storage for submitted text (from webview IPC)
lazy_static::lazy_static! {
    static ref SUBMITTED_TEXT: Mutex<Option<String>> = Mutex::new(None);
    static ref SHOULD_CLOSE: Mutex<bool> = Mutex::new(false);
}

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
    }


    
    #editor {
        flex: 1;
        width: 100%;
        padding: 12px 14px;
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
    </div>
    <script>
        const editor = document.getElementById('editor');
        
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
        
        // Prevent context menu
        document.addEventListener('contextmenu', e => e.preventDefault());
    </script>
</body>
</html>"#)
}

pub fn is_active() -> bool {
    unsafe { INPUT_HWND.0 != 0 }
}

pub fn cancel_input() {
    unsafe {
        if INPUT_HWND.0 != 0 {
            PostMessageW(INPUT_HWND, WM_CLOSE, WPARAM(0), LPARAM(0));
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
pub fn set_editor_text(text: &str) {
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

pub fn show(
    prompt_guide: String,
    ui_language: String,
    cancel_hotkey_name: String,
    on_submit: impl Fn(String, HWND) + Send + 'static
) {
    unsafe {
        if INPUT_HWND.0 != 0 {
            SetForegroundWindow(INPUT_HWND);
            return;
        }

        ON_SUBMIT = Some(Box::new(on_submit));
        
        // Store strings
        CURRENT_TITLE_OVERRIDE = prompt_guide;
        CURRENT_UI_LANG = ui_language;
        CURRENT_CANCEL_KEY = cancel_hotkey_name;
        FADE_ALPHA = 0;
        IS_DRAGGING = false;
        
        // Reset global state
        *SUBMITTED_TEXT.lock().unwrap() = None;
        *SHOULD_CLOSE.lock().unwrap() = false;

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

        // NOTE: WS_EX_TOOLWINDOW was removed because it prevents Vietnamese IMEs 
        // (EVkey, Unikey) from activating. Tool windows are treated specially by Windows
        // and some IMEs don't associate with them. Using WS_EX_APPWINDOW ensures
        // the window is treated as a normal application window for IME purposes.
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_APPWINDOW | WS_EX_LAYERED,
            class_name,
            w!("Text Input"),
            WS_POPUP,
            x, y, win_w, win_h,
            None, None, instance, None
        );

        INPUT_HWND = hwnd;

        // Start invisible for fade-in
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

        // Window Region (Rounded)
        let rgn = CreateRoundRectRgn(0, 0, win_w, win_h, 16, 16);
        SetWindowRgn(hwnd, rgn, true);

        // Create webview for the text editor area
        // Base dimensions for the editor area
        let edit_x = 20;
        let edit_y = 50;
        let edit_w = win_w - 40;
        let edit_h = win_h - 90;
        
        // Inset the webview by the corner radius so the parent's white rounded fill shows at corners
        let corner_inset = 6; // ~half of CSS border-radius to reveal the rounded corners from parent
        let webview_x = edit_x + corner_inset;
        let webview_y = edit_y + corner_inset;
        let webview_w = edit_w - (corner_inset * 2);
        let webview_h = edit_h - (corner_inset * 2);
        
        let locale = LocaleText::get(&CURRENT_UI_LANG);
        // Use the locale's placeholder text for the textarea
        let placeholder = locale.text_input_placeholder.to_string();

        
        let html = get_editor_html(&placeholder);
        let wrapper = HwndWrapper(hwnd);
        
        let result = WebViewBuilder::new()
            .with_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(webview_x, webview_y)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    webview_w as u32,
                    webview_h as u32
                )),
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
                }
            })
            .build_as_child(&wrapper);
        
        match result {
            Ok(webview) => {
                TEXT_INPUT_WEBVIEW.with(|wv| {
                    *wv.borrow_mut() = Some(webview);
                });
            }
            Err(e) => {
                eprintln!("Failed to create WebView for text input: {:?}", e);
                DestroyWindow(hwnd);
                INPUT_HWND = HWND(0);
                ON_SUBMIT = None;
                return;
            }
        }

        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        UpdateWindow(hwnd);
        
        // Start Fade Timer (16ms = ~60fps)
        SetTimer(hwnd, 1, 16, None);
        
        // IPC check timer (check for submit/cancel from webview)
        SetTimer(hwnd, 2, 50, None);

        // Message Loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(hwnd).as_bool() { break; }
        }

        // Cleanup webview
        TEXT_INPUT_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = None;
        });
        
        INPUT_HWND = HWND(0);
        ON_SUBMIT = None;
    }
}

unsafe extern "system" fn input_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Prevent flickering by handling erase background
        WM_ERASEBKGND => LRESULT(1),

        WM_TIMER => {
            if wparam.0 == 1 { 
                // Fade In Logic
                if FADE_ALPHA < 245 {
                    FADE_ALPHA += 25;
                    if FADE_ALPHA > 245 { FADE_ALPHA = 245; }
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), FADE_ALPHA as u8, LWA_ALPHA);
                } else {
                    // Stop timer once fade is complete to save CPU/battery
                    KillTimer(hwnd, 1);
                }
            }
            
            if wparam.0 == 2 {
                // Check for IPC messages from webview
                let should_close = *SHOULD_CLOSE.lock().unwrap();
                if should_close {
                    let submitted = SUBMITTED_TEXT.lock().unwrap().take();
                    if let Some(text) = submitted {
                        if let Some(cb) = ON_SUBMIT.as_ref() {
                            DestroyWindow(hwnd);
                            cb(text, hwnd);
                        }
                    } else {
                        DestroyWindow(hwnd);
                    }
                    *SHOULD_CLOSE.lock().unwrap() = false;
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            
            // Check Close Button Click
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let w = rect.right;
            let close_x = w - 30;
            let close_y = 20;
            if (x - close_x).abs() < 15 && (y - close_y).abs() < 15 {
                 PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                 return LRESULT(0);
            }

            // Only start drag if clicking on the title bar area (top 50px)
            if y < 50 {
                IS_DRAGGING = true;
                
                let mut pt_screen = POINT::default();
                GetCursorPos(&mut pt_screen);
                DRAG_START_MOUSE = pt_screen;
                
                let mut rect_win = RECT::default();
                GetWindowRect(hwnd, &mut rect_win);
                DRAG_START_WIN_POS = POINT { x: rect_win.left, y: rect_win.top };
                
                SetCapture(hwnd);
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                let mut pt_screen = POINT::default();
                GetCursorPos(&mut pt_screen);
                
                let dx = pt_screen.x - DRAG_START_MOUSE.x;
                let dy = pt_screen.y - DRAG_START_MOUSE.y;
                
                let new_x = DRAG_START_WIN_POS.x + dx;
                let new_y = DRAG_START_WIN_POS.y + dy;
                
                SetWindowPos(hwnd, HWND(0), new_x, new_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false;
                ReleaseCapture();
            }
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

            // 2. Draw white rounded rectangle background for webview area with ANTI-ALIASING
            // This fills the area behind the webview so its transparent corners show white
            let edit_x = 20;
            let edit_y = 50;
            let edit_w = w - 40;
            let edit_h = h - 90;
            
            // Use SDF-based anti-aliased rendering for smooth corners
            let corner_radius = 12.0f32;
            let fill_color: u32 = 0xF0F0F0; // Match editor gradient bottom color (BGR for GDI)
            
            // Draw each pixel with anti-aliasing using signed distance field
            let cx = (edit_w as f32) / 2.0; // Center X relative to edit area
            let cy = (edit_h as f32) / 2.0; // Center Y relative to edit area
            // NOTE: sd_rounded_box expects FULL half-extents, it handles radius internally
            let half_w = cx;
            let half_h = cy;

            
            for py_local in 0..edit_h {
                for px_local in 0..edit_w {
                    let px_screen = edit_x + px_local;
                    let py_screen = edit_y + py_local;
                    
                    // Calculate signed distance from rounded rect center
                    let px_rel = (px_local as f32) - cx;
                    let py_rel = (py_local as f32) - cy;
                    
                    let d = crate::overlay::paint_utils::sd_rounded_box(px_rel, py_rel, half_w, half_h, corner_radius);
                    
                    if d < -1.0 {
                        // Fully inside - solid white
                        SetPixel(mem_dc, px_screen, py_screen, COLORREF(fill_color));
                    } else if d < 1.0 {
                        // Edge - anti-aliased blend
                        let t = (d + 1.0) / 2.0; // 0.0 (inside) to 1.0 (outside)
                        let alpha = 1.0 - t * t * (3.0 - 2.0 * t); // Smooth step
                        
                        if alpha > 0.01 {
                            // Get the dark background color
                            let bg_r = ((COL_DARK_BG >> 16) & 0xFF) as f32;
                            let bg_g = ((COL_DARK_BG >> 8) & 0xFF) as f32;
                            let bg_b = (COL_DARK_BG & 0xFF) as f32;
                            
                            let fg_r = ((fill_color >> 16) & 0xFF) as f32;
                            let fg_g = ((fill_color >> 8) & 0xFF) as f32;
                            let fg_b = (fill_color & 0xFF) as f32;
                            
                            // Blend colors
                            let r = (fg_r * alpha + bg_r * (1.0 - alpha)) as u32;
                            let g = (fg_g * alpha + bg_g * (1.0 - alpha)) as u32;
                            let b = (fg_b * alpha + bg_b * (1.0 - alpha)) as u32;
                            
                            SetPixel(mem_dc, px_screen, py_screen, COLORREF((r << 16) | (g << 8) | b));
                        }
                        // else: leave dark background (already painted)
                    }
                    // else: outside - leave dark background
                }
            }



            
            // 3. Draw Text Labels
            SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, COLORREF(0x00FFFFFF)); 
            
            let h_font = CreateFontW(19, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            let old_font = SelectObject(mem_dc, h_font);
            
            let locale = LocaleText::get(&CURRENT_UI_LANG);
            let title_str = if !CURRENT_TITLE_OVERRIDE.is_empty() { CURRENT_TITLE_OVERRIDE.clone() } else { locale.text_input_title_default.to_string() };
            let mut title_w = crate::overlay::utils::to_wstring(&title_str);
            let mut r_title = RECT { left: 20, top: 15, right: w - 50, bottom: 45 };
            DrawTextW(mem_dc, &mut title_w, &mut r_title, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS);
            
            let h_font_small = CreateFontW(13, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            SelectObject(mem_dc, h_font_small);
            SetTextColor(mem_dc, COLORREF(0x00AAAAAA)); 
            
            let esc_text = if CURRENT_CANCEL_KEY.is_empty() { "Esc".to_string() } else { format!("Esc / {}", CURRENT_CANCEL_KEY) };
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
        WM_CLOSE => {
            DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
