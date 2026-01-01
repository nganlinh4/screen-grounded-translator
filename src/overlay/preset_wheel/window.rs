// Preset Wheel Window - Persistent Hidden Window for Instant Appearance

use super::html::{generate_css, generate_items_html, get_wheel_template};
use crate::config::Preset;
use crate::APP;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use std::sync::{Mutex, Once};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitialize, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebView, WebViewBuilder};

static REGISTER_WHEEL_CLASS: Once = Once::new();
static REGISTER_OVERLAY_CLASS: Once = Once::new();

// Custom Messages
const WM_APP_SHOW: u32 = WM_USER + 10;
const WM_APP_HIDE: u32 = WM_USER + 11;
const WM_APP_REAL_SHOW: u32 = WM_USER + 12;

// Large dimensions for wheel window - transparent so no visual impact
// Must fit on common screens (1366x768 minimum)
const WHEEL_WIDTH: i32 = 1200;
const WHEEL_HEIGHT: i32 = 700;

// Result communication
pub static WHEEL_RESULT: AtomicI32 = AtomicI32::new(-1);
pub static WHEEL_ACTIVE: AtomicBool = AtomicBool::new(false);

// Thread-safe handles
static WHEEL_HWND: AtomicIsize = AtomicIsize::new(0);
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
static IS_WARMING_UP: AtomicBool = AtomicBool::new(false);

// Shared data
lazy_static::lazy_static! {
    static ref PENDING_ITEMS_HTML: Mutex<String> = Mutex::new(String::new());
    static ref PENDING_DISMISS_LABEL: Mutex<String> = Mutex::new(String::new());
    static ref PENDING_CSS: Mutex<String> = Mutex::new(String::new());
    static ref PENDING_POS: Mutex<(i32, i32)> = Mutex::new((0, 0));
    static ref SELECTED_PRESET: Mutex<Option<usize>> = Mutex::new(None);
}

thread_local! {
    static WHEEL_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    static WHEEL_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

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

pub fn warmup() {
    // Prevent multiple warmup threads from spawning
    if IS_WARMING_UP
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        internal_create_window_loop();
    });
}

pub fn show_preset_wheel(
    filter_type: &str,
    filter_mode: Option<&str>,
    center_pos: POINT,
) -> Option<usize> {
    unsafe {
        WHEEL_RESULT.store(-1, Ordering::SeqCst);
        WHEEL_ACTIVE.store(true, Ordering::SeqCst);
        *SELECTED_PRESET.lock().unwrap() = None;

        let (presets, ui_lang, is_dark) = {
            let app = APP.lock().unwrap();
            let is_dark = match app.config.theme_mode {
                crate::config::ThemeMode::Dark => true,
                crate::config::ThemeMode::Light => false,
                crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
            };
            (
                app.config.presets.clone(),
                app.config.ui_language.clone(),
                is_dark,
            )
        };

        // Generate themed CSS for injection
        let themed_css = generate_css(is_dark);

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

        let dismiss_label = match ui_lang.as_str() {
            "vi" => "HỦY",
            "ko" => "취소",
            _ => "CANCEL",
        };

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let win_x = (center_pos.x - WHEEL_WIDTH / 2)
            .max(0)
            .min(screen_w - WHEEL_WIDTH);
        let win_y = (center_pos.y - WHEEL_HEIGHT / 2)
            .max(0)
            .min(screen_h - WHEEL_HEIGHT);

        let items_html = generate_items_html(&filtered, &ui_lang);

        *PENDING_ITEMS_HTML.lock().unwrap() = items_html;
        *PENDING_DISMISS_LABEL.lock().unwrap() = dismiss_label.to_string();
        *PENDING_CSS.lock().unwrap() = themed_css;
        *PENDING_POS.lock().unwrap() = (win_x, win_y);

        let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
        let wheel_hwnd = HWND(hwnd_val as *mut _);

        if !wheel_hwnd.is_invalid() {
            let _ = PostMessageW(Some(wheel_hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        } else {
            // Check if already warming up, if not, start it
            if !IS_WARMING_UP.load(Ordering::SeqCst) {
                warmup();
            }

            // Polling loop: Wait up to 2 seconds for initialization to complete
            // This prevents "failing once then working" or "spinning cursor" issues
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
                if hwnd_val != 0 {
                    let wheel_hwnd = HWND(hwnd_val as *mut _);
                    let _ = PostMessageW(Some(wheel_hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0));
                    break;
                }
            }
        }

        let mut msg = MSG::default();
        loop {
            let res = WHEEL_RESULT.load(Ordering::SeqCst);
            if res != -1 {
                break;
            }
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        WHEEL_ACTIVE.store(false, Ordering::SeqCst);
        let res = WHEEL_RESULT.load(Ordering::SeqCst);
        if res >= 0 {
            Some(res as usize)
        } else {
            None
        }
    }
}

pub fn dismiss_wheel() {
    unsafe {
        let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
        let wheel_hwnd = HWND(hwnd_val as *mut _);
        if !wheel_hwnd.is_invalid() {
            let _ = PostMessageW(Some(wheel_hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
        }
    }
    WHEEL_RESULT.store(-2, Ordering::SeqCst);
}

pub fn is_wheel_active() -> bool {
    WHEEL_ACTIVE.load(Ordering::SeqCst)
}

fn internal_create_window_loop() {
    unsafe {
        // Initialize COM for the thread (Critical for WebView2/Wry)
        let _ = CoInitialize(None);

        let instance = GetModuleHandleW(None).unwrap_or_default();
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let overlay_class = w!("SGTWheelOverlayPersistent");
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

        OVERLAY_HWND.store(overlay_hwnd.0 as isize, Ordering::SeqCst);
        let _ = SetLayeredWindowAttributes(overlay_hwnd, COLORREF(0), 1, LWA_ALPHA);

        let class_name = w!("SGTPresetWheelPersistent");
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("PresetWheel"),
            WS_POPUP, // Removed WS_VISIBLE to prevent initial flash/artifacts
            -4000,
            -4000,
            WHEEL_WIDTH,
            WHEEL_HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        // DELAYED STORE: Do not publish WHEEL_HWND yet. Wait until WebView is built.
        // WHEEL_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        // Use DWM to extend the "glass" frame into the entire client area for transparency
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        unsafe {
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }

        let wrapper = HwndWrapper(hwnd);

        WHEEL_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        let webview_res = WHEEL_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                WebViewBuilder::new_with_web_context(web_ctx)
            } else {
                WebViewBuilder::new()
            };
            let builder = crate::overlay::html_components::font_manager::configure_webview(builder);

            let template_html = get_wheel_template(true); // Default dark for warmup

            builder
                .with_transparent(true)
                .with_background_color((0, 0, 0, 0))
                .with_html(template_html)
                .with_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        WHEEL_WIDTH as u32,
                        WHEEL_HEIGHT as u32,
                    )),
                })
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    let body = msg.body();
                    if body == "ready_to_show" {
                        let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
                        let wheel_hwnd = HWND(hwnd_val as *mut _);
                        if !wheel_hwnd.is_invalid() {
                            let _ = PostMessageW(
                                Some(wheel_hwnd),
                                WM_APP_REAL_SHOW,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    } else if body == "dismiss" {
                        let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
                        let wheel_hwnd = HWND(hwnd_val as *mut _);
                        if !wheel_hwnd.is_invalid() {
                            let _ =
                                PostMessageW(Some(wheel_hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                        }
                        *SELECTED_PRESET.lock().unwrap() = None;
                        WHEEL_RESULT.store(-2, Ordering::SeqCst);
                    } else if let Some(idx_str) = body.strip_prefix("select:") {
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
                            let wheel_hwnd = HWND(hwnd_val as *mut _);
                            if !wheel_hwnd.is_invalid() {
                                let _ = PostMessageW(
                                    Some(wheel_hwnd),
                                    WM_APP_HIDE,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                            *SELECTED_PRESET.lock().unwrap() = Some(idx);
                            WHEEL_RESULT.store(idx as i32, Ordering::SeqCst);
                        }
                    }
                })
                .build(&wrapper)
        });

        if let Ok(wv) = webview_res {
            WHEEL_WEBVIEW.with(|cell| {
                *cell.borrow_mut() = Some(wv);
            });
            let _ = ShowWindow(hwnd, SW_HIDE);

            // Now that WebView is ready, publicize the HWND and mark warmup as done
            WHEEL_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            IS_WARMING_UP.store(false, Ordering::SeqCst);
        } else {
            // Initialization failed - cleanup and exit
            let _ = DestroyWindow(hwnd);
            let _ = DestroyWindow(overlay_hwnd);
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            OVERLAY_HWND.store(0, Ordering::SeqCst);
            WHEEL_HWND.store(0, Ordering::SeqCst);
            let _ = CoUninitialize(); // Cleanup COM
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        WHEEL_WEBVIEW.with(|cell| {
            *cell.borrow_mut() = None;
        });
        WHEEL_HWND.store(0, Ordering::SeqCst);
        OVERLAY_HWND.store(0, Ordering::SeqCst);
        IS_WARMING_UP.store(false, Ordering::SeqCst); // Ensure flag is cleared on exit
        let _ = CoUninitialize(); // Cleanup COM
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            let hwnd_val = WHEEL_HWND.load(Ordering::SeqCst);
            let wheel_hwnd = HWND(hwnd_val as *mut _);
            if !wheel_hwnd.is_invalid() {
                let _ = PostMessageW(Some(wheel_hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
            }
            WHEEL_RESULT.store(-2, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_CLOSE => LRESULT(0),
        WM_ERASEBKGND => LRESULT(1), // Prevent GDI from clearing background to black/white
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn wheel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW => {
            let items_html = PENDING_ITEMS_HTML.lock().unwrap().clone();
            let dismiss_label = PENDING_DISMISS_LABEL.lock().unwrap().clone();
            let themed_css = PENDING_CSS.lock().unwrap().clone();

            // 1. Ensure Off-screen (-4000, -4000) but VISIBLE
            // Remove calls to SetLayeredWindowAttributes here to prevent black artifacts
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                -4000,
                -4000,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );

            // Re-apply glass effect to ensure it's active
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            // 2. Inject themed CSS and update content via JS
            WHEEL_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    // Inject themed CSS
                    let css_escaped = themed_css
                        .replace("\\", "\\\\")
                        .replace("`", "\\`")
                        .replace("$", "\\$");
                    let css_script = format!(
                        "document.getElementById('theme-style').textContent = `{}`;",
                        css_escaped
                    );
                    let _ = webview.evaluate_script(&css_script);

                    // Update content
                    let script = format!(
                        "window.updateContent(`{}`, `{}`);",
                        items_html
                            .replace("\\", "\\\\")
                            .replace("`", "\\`")
                            .replace("$", "\\$"),
                        dismiss_label.replace("`", "\\`").replace("$", "\\$")
                    );
                    let _ = webview.evaluate_script(&script);

                    // Force bounds update to ensure WebView syncs with window size
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            WHEEL_WIDTH as u32,
                            WHEEL_HEIGHT as u32,
                        )),
                    });
                }
            });

            // 3. Fallback timer (150ms) - Increased robustness
            SetTimer(Some(hwnd), 99, 150, None);

            LRESULT(0)
        }

        WM_APP_REAL_SHOW => {
            let _ = KillTimer(Some(hwnd), 99);
            let (target_x, target_y) = *PENDING_POS.lock().unwrap();

            // Show overlay
            let overlay_val = OVERLAY_HWND.load(Ordering::SeqCst);
            let overlay = HWND(overlay_val as *mut _);
            if !overlay.is_invalid() {
                let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                let screen_h = GetSystemMetrics(SM_CYSCREEN);
                let _ = SetWindowPos(
                    overlay,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    screen_w,
                    screen_h,
                    SWP_NOACTIVATE | SWP_NOMOVE,
                );
            }

            // Move wheel on-screen
            let _ = InvalidateRect(Some(hwnd), None, true);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                target_x,
                target_y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE,
            );

            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == 99 {
                let _ = PostMessageW(Some(hwnd), WM_APP_REAL_SHOW, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }

        WM_APP_HIDE => {
            let _ = KillTimer(Some(hwnd), 99);
            let _ = ShowWindow(hwnd, SW_HIDE);
            let overlay_val = OVERLAY_HWND.load(Ordering::SeqCst);
            let overlay = HWND(overlay_val as *mut _);
            if !overlay.is_invalid() {
                let _ = ShowWindow(overlay, SW_HIDE);
            }

            WHEEL_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let _ =
                        webview.evaluate_script("document.getElementById('grid').innerHTML = '';");
                }
            });

            LRESULT(0)
        }

        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                let _ = PostMessageW(Some(hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                WHEEL_RESULT.store(-2, Ordering::SeqCst);
            }
            LRESULT(0)
        }

        // Handle DPI changes to maintain correct size
        WM_DPICHANGED => {
            let rect = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            // WebView bounds will be updated on next SHOW or we could enforce it here if active
            // For now, next JS check/show will resync.
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
