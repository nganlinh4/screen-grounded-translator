// Preset Wheel Window - WebView2 with transparent overlay for outside click detection

use super::html::generate_wheel_html;
use crate::config::Preset;
use crate::APP;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, Once};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebView, WebViewBuilder};

static REGISTER_WHEEL_CLASS: Once = Once::new();
static REGISTER_OVERLAY_CLASS: Once = Once::new();

// Result communication
pub static WHEEL_RESULT: AtomicI32 = AtomicI32::new(-1); // -1 = pending, -2 = dismissed, >=0 = preset index
pub static WHEEL_ACTIVE: AtomicBool = AtomicBool::new(false);
static WHEEL_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static OVERLAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

thread_local! {
    static WHEEL_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    static WHEEL_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

// Selected result stored here
static SELECTED_PRESET: Mutex<Option<usize>> = Mutex::new(None);

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

/// Show preset wheel and return selected preset index (or None if dismissed)
/// This function blocks until user makes a selection
pub fn show_preset_wheel(
    filter_type: &str,
    filter_mode: Option<&str>,
    center_pos: POINT,
) -> Option<usize> {
    unsafe {
        // Reset state
        WHEEL_RESULT.store(-1, Ordering::SeqCst);
        WHEEL_ACTIVE.store(true, Ordering::SeqCst);
        *SELECTED_PRESET.lock().unwrap() = None;

        // Get filtered presets
        let (presets, ui_lang) = {
            let app = APP.lock().unwrap();
            (app.config.presets.clone(), app.config.ui_language.clone())
        };

        // Filter presets based on type and mode
        let filtered: Vec<(usize, Preset)> = presets
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                if p.is_master {
                    return false;
                }
                if p.is_upcoming {
                    return false;
                }
                if p.preset_type != filter_type {
                    return false;
                }
                if filter_type == "audio" && p.audio_processing_mode == "realtime" {
                    return false;
                }
                if let Some(mode) = filter_mode {
                    match filter_type {
                        "text" => {
                            if p.text_input_mode != mode {
                                return false;
                            }
                        }
                        "audio" => {
                            if p.audio_source != mode {
                                return false;
                            }
                        }
                        _ => {}
                    }
                }
                true
            })
            .map(|(i, p)| (i, p.clone()))
            .collect();

        if filtered.is_empty() {
            WHEEL_ACTIVE.store(false, Ordering::SeqCst);
            return None;
        }

        // Dismiss label
        let dismiss_label = match ui_lang.as_str() {
            "vi" => "HỦY",
            "ko" => "취소",
            _ => "CANCEL",
        };

        // Calculate window size based on preset count - generous sizing for fisheye effect
        let preset_count = filtered.len();
        let cols = if preset_count <= 3 {
            preset_count
        } else {
            ((preset_count as f32).sqrt().ceil() as usize).max(3).min(4)
        };
        let rows = (preset_count + cols - 1) / cols;

        // Larger window with extra padding for hover scaling room
        let wheel_width = (cols as i32 * 145 + 80).max(350);
        let wheel_height = (rows as i32 * 52 + 120).max(180);

        // Calculate window position
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let win_x = (center_pos.x - wheel_width / 2)
            .max(10)
            .min(screen_w - wheel_width - 10);
        let win_y = (center_pos.y - wheel_height / 2)
            .max(10)
            .min(screen_h - wheel_height - 10);

        let instance = GetModuleHandleW(None).unwrap_or_default();

        // === Create transparent full-screen overlay to catch outside clicks ===
        let overlay_class = w!("SGTWheelOverlay");
        REGISTER_OVERLAY_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(overlay_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: overlay_class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        let overlay_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            overlay_class,
            w!("WheelOverlay"),
            WS_POPUP,
            0,
            0,
            screen_w,
            screen_h,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if !overlay_hwnd.is_invalid() {
            // Make it almost completely transparent but still clickable
            let _ = SetLayeredWindowAttributes(overlay_hwnd, COLORREF(0), 1, LWA_ALPHA);
            OVERLAY_HWND.store(overlay_hwnd.0 as isize, Ordering::SeqCst);
        }

        // === Create the actual wheel window ===
        let class_name = w!("SGTPresetWheelWV");
        REGISTER_WHEEL_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wheel_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED, // Reverted: Removed WS_EX_NOACTIVATE
            class_name,
            w!("PresetWheel"),
            WS_POPUP,
            win_x,
            win_y,
            wheel_width,
            wheel_height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if hwnd.is_invalid() {
            if !overlay_hwnd.is_invalid() {
                let _ = DestroyWindow(overlay_hwnd);
            }
            WHEEL_ACTIVE.store(false, Ordering::SeqCst);
            return None;
        }

        WHEEL_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

        // Create WebView
        let wrapper = HwndWrapper(hwnd);
        let html = generate_wheel_html(&filtered, dismiss_label, &ui_lang);

        WHEEL_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        let webview = WHEEL_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                WebViewBuilder::new_with_web_context(web_ctx)
            } else {
                WebViewBuilder::new()
            };
            let builder = crate::overlay::html_components::font_manager::configure_webview(builder);
            builder
                .with_bounds(Rect {
                    position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        wheel_width as u32,
                        wheel_height as u32,
                    )),
                })
                .with_transparent(true)
                .with_html(&html)
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    let body = msg.body();
                    if body == "dismiss" {
                        *SELECTED_PRESET.lock().unwrap() = None;
                        WHEEL_RESULT.store(-2, Ordering::SeqCst);
                        close_wheel_windows();
                    } else if let Some(idx_str) = body.strip_prefix("select:") {
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            *SELECTED_PRESET.lock().unwrap() = Some(idx);
                            WHEEL_RESULT.store(idx as i32, Ordering::SeqCst);
                            close_wheel_windows();
                        }
                    }
                })
                .build(&wrapper)
        });

        if let Ok(wv) = webview {
            WHEEL_WEBVIEW.with(|cell| {
                *cell.borrow_mut() = Some(wv);
            });

            // Show overlay first (behind), then wheel (in front)
            if !overlay_hwnd.is_invalid() {
                let _ = ShowWindow(overlay_hwnd, SW_SHOWNOACTIVATE);
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

            // Make sure wheel is above overlay
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );

            // Message loop
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);

                if WHEEL_RESULT.load(Ordering::SeqCst) != -1 {
                    break;
                }
            }
        }

        // Cleanup
        WHEEL_WEBVIEW.with(|cell| {
            *cell.borrow_mut() = None;
        });
        WHEEL_HWND.store(0, Ordering::SeqCst);
        OVERLAY_HWND.store(0, Ordering::SeqCst);
        WHEEL_ACTIVE.store(false, Ordering::SeqCst);

        SELECTED_PRESET.lock().unwrap().take()
    }
}

fn close_wheel_windows() {
    unsafe {
        let h = WHEEL_HWND.load(Ordering::SeqCst);
        let o = OVERLAY_HWND.load(Ordering::SeqCst);
        if h != 0 {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if o != 0 {
            let _ = PostMessageW(Some(HWND(o as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

// Overlay window proc - catches clicks outside the wheel
unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            // Any click on the overlay = outside click = dismiss
            *SELECTED_PRESET.lock().unwrap() = None;
            WHEEL_RESULT.store(-2, Ordering::SeqCst);
            close_wheel_windows();
            LRESULT(0)
        }

        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// Wheel window proc
unsafe extern "system" fn wheel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                *SELECTED_PRESET.lock().unwrap() = None;
                WHEEL_RESULT.store(-2, Ordering::SeqCst);
                close_wheel_windows();
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            // Also close overlay
            let o = OVERLAY_HWND.load(Ordering::SeqCst);
            if o != 0 {
                let _ = DestroyWindow(HWND(o as *mut _));
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            // Do NOT call PostQuitMessage here because we share the thread with text_input!
            // PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn is_wheel_active() -> bool {
    WHEEL_ACTIVE.load(Ordering::SeqCst)
}

pub fn dismiss_wheel() {
    WHEEL_RESULT.store(-2, Ordering::SeqCst);
    close_wheel_windows();
}
