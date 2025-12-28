use super::html::{escape_js, generate_panel_html, get_favorite_presets_html};
use super::render::update_bubble_visual;
use super::state::*;
use super::utils::HwndWrapper;
use crate::APP;
use std::sync::atomic::Ordering;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebViewBuilder};

pub fn show_panel(bubble_hwnd: HWND) {
    if IS_EXPANDED.load(Ordering::SeqCst) {
        return;
    }

    // Ensure window exists
    ensure_panel_created(bubble_hwnd);

    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val == 0 {
        return;
    }

    unsafe {
        let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);

        if let Ok(app) = APP.lock() {
            refresh_panel_layout_and_content(
                bubble_hwnd,
                panel_hwnd,
                &app.config.presets,
                &app.config.ui_language,
            );
        }

        IS_EXPANDED.store(true, Ordering::SeqCst);
        update_bubble_visual(bubble_hwnd);
    }
}

pub fn update_favorites_panel() {
    // Simply close the panel if it's open, forcing a reopen (and refresh) next time the user clicks.
    if IS_EXPANDED.load(Ordering::SeqCst) {
        close_panel();
    }
}

pub fn ensure_panel_created(bubble_hwnd: HWND) {
    if PANEL_HWND.load(Ordering::SeqCst) != 0 {
        return;
    }
    create_panel_window_internal(bubble_hwnd);
}

// Hides the panel but keeps it alive (warm)
pub fn close_panel() {
    if !IS_EXPANDED.swap(false, Ordering::SeqCst) {
        return;
    }

    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val != 0 {
        unsafe {
            let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
            let _ = ShowWindow(panel_hwnd, SW_HIDE);
        }
    }

    // Update bubble visual
    let bubble_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if bubble_val != 0 {
        let bubble_hwnd = HWND(bubble_val as *mut std::ffi::c_void);
        update_bubble_visual(bubble_hwnd);
    }

    // Save position
    save_bubble_position();
}

// Actually destroys the panel (cleanup)
pub fn destroy_panel() {
    let panel_val = PANEL_HWND.swap(0, Ordering::SeqCst);
    if panel_val != 0 {
        PANEL_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = None;
        });

        unsafe {
            let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
            let _ = DestroyWindow(panel_hwnd);
        }
    }
}

pub fn move_panel_to_bubble(bubble_x: i32, bubble_y: i32) {
    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val == 0 {
        return;
    }

    unsafe {
        let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
        let mut panel_rect = RECT::default();
        let _ = GetWindowRect(panel_hwnd, &mut panel_rect);
        let panel_w = panel_rect.right - panel_rect.left;
        let panel_h = panel_rect.bottom - panel_rect.top;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let (panel_x, panel_y) = if bubble_x > screen_w / 2 {
            (
                bubble_x - panel_w - 8,
                bubble_y - panel_h / 2 + BUBBLE_SIZE / 2,
            )
        } else {
            (
                bubble_x + BUBBLE_SIZE + 8,
                bubble_y - panel_h / 2 + BUBBLE_SIZE / 2,
            )
        };

        let _ = SetWindowPos(
            panel_hwnd,
            None,
            panel_x,
            panel_y.max(10),
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn create_panel_window_internal(_bubble_hwnd: HWND) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTFavoritePanel");

        REGISTER_PANEL_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(panel_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        let panel_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("FavPanel"),
            WS_POPUP,
            0,
            0,
            PANEL_WIDTH,
            100, // Dummy height
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if !panel_hwnd.is_invalid() {
            PANEL_HWND.store(panel_hwnd.0 as isize, Ordering::SeqCst);
            create_panel_webview(panel_hwnd);
        }
    }
}

unsafe fn refresh_panel_layout_and_content(
    bubble_hwnd: HWND,
    panel_hwnd: HWND,
    presets: &[crate::config::Preset],
    lang: &str,
) {
    let mut bubble_rect = RECT::default();
    let _ = GetWindowRect(bubble_hwnd, &mut bubble_rect);

    let height_per_item = 48;

    let favs: Vec<_> = presets
        .iter()
        .filter(|p| p.is_favorite && !p.is_upcoming && !p.is_master)
        .collect();

    let fav_count = favs.len();
    let num_cols = if fav_count > 15 {
        (fav_count + 14) / 15
    } else {
        1
    };

    let items_per_col = if fav_count > 0 {
        (fav_count + num_cols - 1) / num_cols
    } else {
        0
    };

    let panel_width = if fav_count == 0 {
        (PANEL_WIDTH as i32 * 2).max(320)
    } else {
        (PANEL_WIDTH as usize * num_cols) as i32
    };

    let panel_height = if fav_count == 0 {
        80
    } else {
        (items_per_col as i32 * height_per_item) + 24
    };
    let panel_height = panel_height.max(50);

    let screen_w = GetSystemMetrics(SM_CXSCREEN);

    let (panel_x, panel_y) = if bubble_rect.left > screen_w / 2 {
        (
            bubble_rect.left - panel_width - 8,
            bubble_rect.top - panel_height / 2 + BUBBLE_SIZE / 2,
        )
    } else {
        (
            bubble_rect.right + 8,
            bubble_rect.top - panel_height / 2 + BUBBLE_SIZE / 2,
        )
    };

    let _ = SetWindowPos(
        panel_hwnd,
        None,
        panel_x,
        panel_y.max(10),
        panel_width,
        panel_height,
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOCOPYBITS,
    );

    PANEL_WEBVIEW.with(|wv| {
        if let Some(webview) = wv.borrow().as_ref() {
            let _ = webview.set_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    panel_width as u32,
                    panel_height as u32,
                )),
            });
        }
    });

    let favorites_html = get_favorite_presets_html(presets, lang);
    update_panel_content(&favorites_html, num_cols);
}

fn create_panel_webview(panel_hwnd: HWND) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(panel_hwnd, &mut rect);
    }

    let html = if let Ok(app) = APP.lock() {
        generate_panel_html(&app.config.presets, &app.config.ui_language)
    } else {
        String::new()
    };

    let wrapper = HwndWrapper(panel_hwnd);

    let result = WebViewBuilder::new()
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            )),
        })
        .with_html(&html)
        .with_transparent(true)
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            let body = msg.body();

            if body == "drag" {
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
                    let _ = ReleaseCapture();
                    SendMessageW(
                        panel_hwnd,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION as usize)),
                        Some(LPARAM(0)),
                    );
                }
            } else if body == "close" {
                close_panel();
            } else if body.starts_with("trigger:") {
                if let Ok(idx) = body[8..].parse::<usize>() {
                    close_panel();
                    trigger_preset(idx);
                }
            }
        })
        .build_as_child(&wrapper);

    if let Ok(webview) = result {
        PANEL_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = Some(webview);
        });
    }
}

unsafe extern "system" fn panel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLOSE => {
            close_panel();
            LRESULT(0)
        }
        WM_KILLFOCUS => LRESULT(0),
        WM_NCCALCSIZE => {
            if wparam.0 != 0 {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn trigger_preset(preset_idx: usize) {
    unsafe {
        let class = w!("HotkeyListenerClass");
        let title = w!("Listener");
        let hwnd = FindWindowW(class, title).unwrap_or_default();

        if !hwnd.is_invalid() {
            let hotkey_id = (preset_idx as i32 * 1000) + 1;
            let _ = PostMessageW(Some(hwnd), WM_HOTKEY, WPARAM(hotkey_id as usize), LPARAM(0));
        }
    }
}

fn save_bubble_position() {
    let bubble_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if bubble_val == 0 {
        return;
    }

    unsafe {
        let bubble_hwnd = HWND(bubble_val as *mut std::ffi::c_void);
        let mut rect = RECT::default();
        let _ = GetWindowRect(bubble_hwnd, &mut rect);

        if let Ok(mut app) = APP.lock() {
            app.config.favorite_bubble_position = Some((rect.left, rect.top));
            crate::config::save_config(&app.config);
        }
    }
}

fn update_panel_content(html: &str, cols: usize) {
    PANEL_WEBVIEW.with(|wv| {
        if let Some(webview) = wv.borrow().as_ref() {
            let escaped = escape_js(html);
            let script = format!(
                "document.querySelector('.list').style.columnCount = '{}'; document.querySelector('.list').innerHTML = \"{}\"; if(window.fitText) window.fitText();",
                cols, escaped
            );
            let _ = webview.evaluate_script(&script);
        }
    });
}
