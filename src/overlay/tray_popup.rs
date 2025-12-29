// Tray Popup - Custom non-blocking popup window for tray icon menu
// Replaces native Windows tray context menu to avoid blocking the main UI thread

use crate::APP;
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Once,
};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebView, WebViewBuilder};

static REGISTER_POPUP_CLASS: Once = Once::new();
// 0=Closed, 1=Warmup, 2=Open, 3=PendingCancel
static POPUP_STATE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static POPUP_HWND: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static POPUP_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
}

const POPUP_WIDTH: i32 = 250;
const BASE_POPUP_HEIGHT: i32 = 118; // Base height at 100% scaling (96 DPI)

/// Get DPI-scaled popup height
fn get_scaled_popup_height() -> i32 {
    let dpi = unsafe {
        windows::Win32::UI::HiDpi::GetDpiForSystem()
    };
    // Scale: 96 DPI = 100%, 120 DPI = 125%, 144 DPI = 150%, etc.
    (BASE_POPUP_HEIGHT * dpi as i32) / 93
}

// HWND wrapper for wry
struct HwndWrapper(HWND);
unsafe impl Send for HwndWrapper {}
unsafe impl Sync for HwndWrapper {}
impl raw_window_handle::HasWindowHandle for HwndWrapper {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::Win32WindowHandle::new(
            std::num::NonZeroIsize::new(self.0 .0 as isize).expect("HWND cannot be null"),
        );
        let handle = raw_window_handle::RawWindowHandle::Win32(raw);
        unsafe { Ok(raw_window_handle::WindowHandle::borrow_raw(handle)) }
    }
}

/// Show the tray popup at cursor position
pub fn show_tray_popup() {

    
    // CAS loop to handle state transitions atomically-ish or just check current state
    // We used swap previously which is good, but we need to handle State 2 differently based on HWND.
    // Let's check current state first.
    
    let current = POPUP_STATE.load(Ordering::SeqCst);
    
    if current == 2 {
        // Already Open or Opening.
        let hwnd_val = POPUP_HWND.load(Ordering::SeqCst);
        
        // 1. Check if fully open (HWND != 0)
        if hwnd_val == 0 {
             return;
        }

        // 2. Validate HWND - Check for Zombie State
        // If the window was destroyed externally or cleanup failed, we might be stuck in State 2
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        let is_valid = unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() };

        
        if !is_valid {

            // Force reset state to 0 so we can respawn
            POPUP_STATE.store(0, Ordering::SeqCst);
            POPUP_HWND.store(0, Ordering::SeqCst);
            // Fall through to respawn logic below
        } else {

            hide_tray_popup();
            return;
        }
    }
    
    // If current is 3 (PendingCancel), we want to "Resurrect" it to 2.
    // If current is 0 (Closed), we want to go 0 -> 2 and spawn.
    // If current is 1 (Warmup), we want to go 1 -> 2 and let existing thread handle it.
    
    let prev = POPUP_STATE.swap(2, Ordering::SeqCst);
    
    if prev == 0 {
        // Was closed, start fresh
        std::thread::spawn(|| {
            create_popup_window(false);
        });
    } else if prev == 3 {
        // Was pending cancel. We swapped it back to 2.
        // The running thread will see 2 at checkpoint and SHOW the window.
        // Resurrection successful!

    }
    // If prev == 1, the running warmup thread will see the state change to 2 and upgrade itself.
}

/// Hide the tray popup
pub fn hide_tray_popup() {
    if POPUP_STATE.load(Ordering::SeqCst) == 0 {
        return;
    }


    let hwnd_val = POPUP_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }

    } else {
        // Window creating but not ready (HWND=0). Signal Cancel (State 3).
        POPUP_STATE.store(3, Ordering::SeqCst);
    }
}

pub fn warmup_tray_popup() {
    // Try to take lock 0 -> 1
    if POPUP_STATE.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
         std::thread::spawn(|| {
            create_popup_window(true);
        });
    } else {
    }
}

fn generate_popup_html() -> String {
    use crate::config::ThemeMode;
    
    let (settings_text, bubble_text, quit_text, bubble_checked, is_dark_mode) = if let Ok(app) = APP.lock() {
        let lang = &app.config.ui_language;
        let settings = match lang.as_str() {
            "vi" => "Cài đặt",
            "ko" => "설정",
            _ => "Settings",
        };
        let bubble = match lang.as_str() {
            "vi" => "Hiển thị bong bóng",
            "ko" => "즐겨찾기 버블",
            _ => "Favorite Bubble",
        };
        let quit = match lang.as_str() {
            "vi" => "Thoát",
            "ko" => "종료",
            _ => "Quit",
        };
        let checked = app.config.show_favorite_bubble;
        
        // Theme detection
        let is_dark = match app.config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        };
        
        (settings, bubble, quit, checked, is_dark)
    } else {
        ("Settings", "Favorite Bubble", "Quit", false, true)
    };

    // Define Colors based on theme
    let (bg_color, text_color, hover_color, border_color, separator_color) = if is_dark_mode {
        ("#2c2c2c", "#ffffff", "#3c3c3c", "#454545", "rgba(255,255,255,0.08)")
    } else {
        ("#f9f9f9", "#1a1a1a", "#eaeaea", "#dcdcdc", "rgba(0,0,0,0.06)")
    };

    let check_mark = if bubble_checked {
        r#"<svg class="check-icon" viewBox="0 0 16 16" fill="currentColor"><path d="M13.86 3.66a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.6 9.03a.75.75 0 1 1 1.06-1.06l2.42 2.42 6.72-6.72a.75.75 0 0 1 1.06 0z"/></svg>"#
    } else {
        ""
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
:root {{
    --bg-color: {bg};
    --text-color: {text};
    --hover-bg: {hover};
    --border-color: {border};
    --separator-color: {separator};
}}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--bg-color);
    font-family: 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
    user-select: none;
    color: var(--text-color);
    border: 1px solid var(--border-color);
    border-radius: 8px;
}}

.container {{
    display: flex;
    flex-direction: column;
    padding: 4px;
}}

.menu-item {{
    display: flex;
    align-items: center;
    padding: 6px 10px;
    border-radius: 4px;
    cursor: default;
    font-size: 13px;
    margin-bottom: 2px;
    background: transparent;
    transition: background 0.1s ease;
    height: 32px;
}}

.menu-item:hover {{
    background: var(--hover-bg);
}}

.icon {{
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    margin-right: 12px;
    opacity: 0.8;
}}

.label {{
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-bottom: 1px; /* Visual alignment */
}}

.check {{
    width: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-left: 8px;
}}

.separator {{
    height: 1px;
    background: var(--separator-color);
    margin: 4px 10px;
}}

svg {{
    width: 16px;
    height: 16px;
}}
</style>
</head>
<body>
<div class="container">
    <div class="menu-item" onclick="action('settings')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.09a2 2 0 0 1-1-1.74v-.47a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.39a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"></path>
                <circle cx="12" cy="12" r="3"></circle>
            </svg>
        </div>
        <div class="label">{settings}</div>
        <div class="check"></div>
    </div>
    
    <div class="menu-item" onclick="action('bubble')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
        </div>
        <div class="label">{bubble}</div>
        <div class="check" id="bubble-check-container">{check}</div>
    </div>
    
    <div class="separator"></div>
    
    <div class="menu-item" onclick="action('quit')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
        </div>
        <div class="label">{quit}</div>
        <div class="check"></div>
    </div>
</div>
<script>
function action(cmd) {{
    window.ipc.postMessage(cmd);
}}
// Close on click outside (detect blur)
window.addEventListener('blur', function() {{
    window.ipc.postMessage('close');
}});
</script>
</body>
</html>"#,
        bg = bg_color,
        text = text_color,
        hover = hover_color,
        border = border_color,
        separator = separator_color,
        settings = settings_text,
        bubble = bubble_text,
        quit = quit_text,
        check = check_mark
    )
}

// RAII Guard to ensure state reset
struct StateGuard;
impl Drop for StateGuard {
    fn drop(&mut self) {
        POPUP_ACTIVE_REF.store(0, Ordering::SeqCst);
        POPUP_HWND_REF.store(0, Ordering::SeqCst);
        
        // Also ensure WebView is dropped on thread exit which helps with cleanup
        POPUP_WEBVIEW.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}

// Accessors for Guard since it can't capture statics directly easily in Drop if they aren't accessible
// Actually statics are global so we can just use them.
// But to be clean we'll just refer to the statics in the Drop impl logic (which refers to global names).
// Wait, Drop implementation cannot capture 'self' context easily for statics unless I put them in a struct.
// But POPUP_STATE is static. I can access it directly.

// We need to define safe access or just use the statics.
// Since `POPUP_STATE` is static, we can access it.

lazy_static::lazy_static! {
    static ref POPUP_ACTIVE_REF: &'static std::sync::atomic::AtomicI32 = &POPUP_STATE;
    static ref POPUP_HWND_REF: &'static AtomicIsize = &POPUP_HWND;
}

fn create_popup_window(is_warmup: bool) {
    let _guard = StateGuard; // Will reset state to 0 on exit/panic

    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTTrayPopup");

        REGISTER_POPUP_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(popup_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        // Get DPI-scaled height once
        let popup_height = get_scaled_popup_height();

        // Get cursor position for placement (calculated later if warming up)
        let (popup_x, popup_y) = if is_warmup {
            (-3000, -3000)
        } else {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            // Position popup above and to the left of cursor (typical tray menu behavior)
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);

            let popup_x = (pt.x - POPUP_WIDTH / 2).max(0).min(screen_w - POPUP_WIDTH);
            let popup_y = (pt.y - popup_height - 10)
                .max(0)
                .min(screen_h - popup_height);

            (popup_x, popup_y)
        };

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("TrayPopup"),
            WS_POPUP,
            popup_x,
            popup_y,
            POPUP_WIDTH,
            popup_height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if hwnd.is_invalid() {
            // Guard will clean up
            return;
        }


        POPUP_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // Round corners
        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::addr_of!(corner_pref) as *const _,
            std::mem::size_of_val(&corner_pref) as u32,
        );

        // Create WebView
        let wrapper = HwndWrapper(hwnd);
        let html = generate_popup_html();

        let webview = WebViewBuilder::new()
            .with_bounds(Rect {
                position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    POPUP_WIDTH as u32,
                    popup_height as u32,
                )),
            })
            .with_transparent(true)
            .with_html(&html)
            .with_ipc_handler(move |msg: wry::http::Request<String>| {
                let body = msg.body();
                match body.as_str() {
                    "settings" => {
                        // Restore main window
                        let h = POPUP_HWND.load(Ordering::SeqCst);
                        if h != 0 {
                            let _ = PostMessageW(
                                Some(HWND(h as *mut _)),
                                WM_CLOSE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                        // Signal to open settings
                        crate::gui::signal_restore_window();
                    }
                    "bubble" => {
                        // Toggle bubble - don't close popup, just update the checkmark
                        let new_state = if let Ok(mut app) = APP.lock() {
                            app.config.show_favorite_bubble = !app.config.show_favorite_bubble;
                            let enabled = app.config.show_favorite_bubble;
                            crate::config::save_config(&app.config);

                            if enabled {
                                crate::overlay::favorite_bubble::show_favorite_bubble();
                            } else {
                                crate::overlay::favorite_bubble::hide_favorite_bubble();
                            }
                            enabled
                        } else {
                            false
                        };

                        // Update checkmark in popup via JavaScript (keep popup open)
                        POPUP_WEBVIEW.with(|cell| {
                            if let Some(webview) = cell.borrow().as_ref() {
                                let js = format!(
                                    "document.getElementById('bubble-check-container').innerHTML = '{}';",
                                    if new_state { 
                                        r#"<svg class="check-icon" viewBox="0 0 16 16" fill="currentColor"><path d="M13.86 3.66a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.6 9.03a.75.75 0 1 1 1.06-1.06l2.42 2.42 6.72-6.72a.75.75 0 0 1 1.06 0z"/></svg>"#
                                    } else { "" }
                                );
                                let _ = webview.evaluate_script(&js);
                            }
                        });
                    }
                    "quit" => {
                        // Close popup first
                        let h = POPUP_HWND.load(Ordering::SeqCst);
                        if h != 0 {
                            let _ = PostMessageW(
                                Some(HWND(h as *mut _)),
                                WM_CLOSE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                        // Small delay to let window close, then exit
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            std::process::exit(0);
                        });
                    }
                    "close" => {
                        let h = POPUP_HWND.load(Ordering::SeqCst);
                        if h != 0 {
                            let _ = PostMessageW(
                                Some(HWND(h as *mut _)),
                                WM_CLOSE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                    _ => {}
                }
            })
            .build(&wrapper);

        if let Ok(wv) = webview {
            POPUP_WEBVIEW.with(|cell| {
                *cell.borrow_mut() = Some(wv);
            });

            // CHECKPOINT: Only show if strictly Open (2)
            let current_state = POPUP_STATE.load(Ordering::SeqCst);
            
            if current_state == 2 {
                // Show it!
                
                // FORCE RESIZE/REPOSITION since we might be resurrecting a cancelled window
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                let screen_h = GetSystemMetrics(SM_CYSCREEN);

                let popup_x = (pt.x - POPUP_WIDTH / 2).max(0).min(screen_w - POPUP_WIDTH);
                let popup_y = (pt.y - popup_height - 10).max(0).min(screen_h - popup_height);
                
                let _ = SetWindowPos(hwnd, None, popup_x, popup_y, POPUP_WIDTH, popup_height, SWP_NOZORDER);
                
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetForegroundWindow(hwnd);
            } else {
                // State is 1 (Warmup) or 3 (Cancelled) -> Close immediately
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        } else {
             // Failed to create webview? Close.
             let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }

        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        
        // Guard will handle cleanup
    }
}

unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ACTIVATE => {
            // We rely on JS 'blur' to close the window, because WM_ACTIVATE is too aggressive
            // and often closes the popup when clicking a toggle (which might briefly steal focus)
            LRESULT(0)
        }

        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
