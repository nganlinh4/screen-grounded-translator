use super::html::{escape_js, generate_panel_css, generate_panel_html, get_favorite_presets_html};
use super::render::update_bubble_visual;
use super::state::*;
use super::utils::HwndWrapper;
use crate::APP;
use std::sync::atomic::Ordering;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetClientRect,
    GetForegroundWindow, GetSystemMetrics, GetWindowRect, LoadCursorW, PostMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, HTCAPTION,
    IDC_ARROW, SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_HOTKEY, WM_KILLFOCUS, WM_NCCALCSIZE,
    WM_NCLBUTTONDOWN, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use wry::{Rect, WebContext, WebViewBuilder};

// For focus restoration
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;

const WM_REFRESH_PANEL: u32 = WM_APP + 42;
pub const WM_FORCE_SHOW_PANEL: u32 = WM_APP + 43;

pub fn show_panel(bubble_hwnd: HWND) {
    if IS_EXPANDED.load(Ordering::SeqCst) {
        return;
    }

    // CRITICAL: Save the current foreground window BEFORE showing the panel.
    // The WebView will steal focus when clicked, but we need to restore focus
    // to the original window for text-select presets to work (they send Ctrl+C).
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.is_invalid() {
            LAST_FOREGROUND_HWND.store(fg.0 as isize, Ordering::SeqCst);
        }
    }

    // Ensure window AND webview exist (webview creation is deferred to here to avoid focus steal)
    ensure_panel_created(bubble_hwnd, true);

    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val == 0 {
        return;
    }

    unsafe {
        let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);

        // CRITICAL: Set state to true BEFORE refreshing or showing,
        // so that any incoming 'close_now' IPC messages (from a previous close)
        // will see that we are now EXPANDED and ignore the hide command.
        IS_EXPANDED.store(true, Ordering::SeqCst);

        if let Ok(app) = APP.lock() {
            let is_dark = match app.config.theme_mode {
                crate::config::ThemeMode::Dark => true,
                crate::config::ThemeMode::Light => false,
                crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
            };

            refresh_panel_layout_and_content(
                bubble_hwnd,
                panel_hwnd,
                &app.config.presets,
                &app.config.ui_language,
                is_dark,
            );
        }

        update_bubble_visual(bubble_hwnd);
    }
}

pub fn update_favorites_panel() {
    // Send a message to the Bubble Window (dedicated thread) to handle the update.
    // This prevents creating a duplicate/desync'd WebView on the main thread.
    let bubble_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if bubble_val != 0 {
        let bubble_hwnd = HWND(bubble_val as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(bubble_hwnd), WM_FORCE_SHOW_PANEL, WPARAM(0), LPARAM(0));
        }
    }
}

/// Ensure the panel window exists.
/// If `with_webview` is true, also create the WebView2 (deferred to avoid focus stealing during warmup).
pub fn ensure_panel_created(bubble_hwnd: HWND, with_webview: bool) {
    let panel_exists = PANEL_HWND.load(Ordering::SeqCst) != 0;

    if !panel_exists {
        create_panel_window_internal(bubble_hwnd);
    }

    // Create WebView2 only when requested AND it doesn't exist yet
    if with_webview {
        let has_webview = PANEL_WEBVIEW.with(|wv| wv.borrow().is_some());
        if !has_webview {
            let panel_val = PANEL_HWND.load(Ordering::SeqCst);
            if panel_val != 0 {
                let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
                create_panel_webview(panel_hwnd);
            }
        }
    }
}

// Triggers the animation-based close
pub fn close_panel() {
    // Set expanded to false immediately to allow re-opening
    if !IS_EXPANDED.swap(false, Ordering::SeqCst) {
        return;
    }

    let webview_exists = PANEL_WEBVIEW.with(|wv| {
        if let Some(webview) = wv.borrow().as_ref() {
            let _ = webview.evaluate_script("if(window.closePanel) window.closePanel();");
            true
        } else {
            false
        }
    });

    if !webview_exists {
        close_panel_internal();
    }
}

// Actually hides the window
fn close_panel_internal() {
    // CRITICAL: If IS_EXPANDED was set to true (e.g. by a quick click to re-open),
    // do NOT hide the window.
    if IS_EXPANDED.load(Ordering::SeqCst) {
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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
            // NOTE: WebView2 creation is deferred to show_panel() to avoid focus stealing
        }
    }
}

unsafe fn refresh_panel_layout_and_content(
    bubble_hwnd: HWND,
    panel_hwnd: HWND,
    presets: &[crate::config::Preset],
    lang: &str,
    is_dark: bool,
) {
    let mut bubble_rect = RECT::default();
    let _ = GetWindowRect(bubble_hwnd, &mut bubble_rect);

    let height_per_item = 48;

    let favs: Vec<_> = presets
        .iter()
        .filter(|p| p.is_favorite && !p.is_upcoming)
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

    // Buffer for padding (no bounce overshoot with smooth easing)
    let buffer_x = 40;
    let buffer_y = 60;

    let panel_width = if fav_count == 0 {
        (PANEL_WIDTH as i32 * 2).max(320)
    } else {
        (PANEL_WIDTH as usize * num_cols) as i32 + buffer_x
    };

    // Height for the keep-open toggle row
    let keep_open_row_height = 40;

    let panel_height = if fav_count == 0 {
        80 + buffer_y + keep_open_row_height
    } else {
        (items_per_col as i32 * height_per_item) + 24 + buffer_y + keep_open_row_height
    };
    let panel_height = panel_height.max(50);

    // Get DPI scale
    let dpi = unsafe { GetDpiForWindow(panel_hwnd) };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };

    let panel_width_physical = (panel_width as f32 * scale).ceil() as i32;
    let panel_height_physical = (panel_height as f32 * scale).ceil() as i32;

    let screen_w = GetSystemMetrics(SM_CXSCREEN);

    let (panel_x, panel_y, side) = if bubble_rect.left > screen_w / 2 {
        (
            bubble_rect.left - panel_width_physical - 4, // Closer to bubble
            bubble_rect.top - panel_height_physical / 2 + BUBBLE_SIZE / 2,
            "right",
        )
    } else {
        (
            bubble_rect.right + 4,
            bubble_rect.top - panel_height_physical / 2 + BUBBLE_SIZE / 2,
            "left",
        )
    };

    // Use the actual clamped panel_y for positioning
    let actual_panel_y = panel_y.max(10);

    let _ = SetWindowPos(
        panel_hwnd,
        None,
        panel_x,
        actual_panel_y,
        panel_width_physical,
        panel_height_physical,
        SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
    );

    // Explicitly show the window to ensure it's visible after a SW_HIDE
    let _ = ShowWindow(panel_hwnd, SW_SHOWNOACTIVATE);

    PANEL_WEBVIEW.with(|wv| {
        if let Some(webview) = wv.borrow().as_ref() {
            let _ = webview.set_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    panel_width_physical as u32,
                    panel_height_physical as u32,
                )),
            });
        }
    });

    // Check if theme changed and inject new CSS if needed
    let last_dark = LAST_THEME_IS_DARK.load(Ordering::SeqCst);
    if last_dark != is_dark {
        let new_css = generate_panel_css(is_dark);
        let escaped_css = escape_js(&new_css); // Reuse escape_js which escapes quotes and newlines
                                               // We need to be careful with escape_js for CSS.
                                               // Simple escape_js replaces " with \" and \n with \\n.
                                               // For inline script, we need to make sure we don't break the string.
        PANEL_WEBVIEW.with(|wv| {
            if let Some(webview) = wv.borrow().as_ref() {
                let script = format!(
                    "document.querySelector('style').innerHTML = \"{}\";",
                    escaped_css
                );
                let _ = webview.evaluate_script(&script);
            }
        });
        LAST_THEME_IS_DARK.store(is_dark, Ordering::SeqCst);
    }

    let favorites_html = get_favorite_presets_html(presets, lang, is_dark);
    update_panel_content(&favorites_html, num_cols);

    // Pass side and bubble center relative to panel to JS
    // Use actual_panel_y (clamped) to match the real window position
    let bx = if side == "left" {
        -(BUBBLE_SIZE as i32 / 2) - 4
    } else {
        (panel_width_physical / scale as i32) + (BUBBLE_SIZE as i32 / 2) + 4
    };
    let by = (bubble_rect.top + BUBBLE_SIZE as i32 / 2) - actual_panel_y;

    PANEL_WEBVIEW.with(|wv| {
        if let Some(webview) = wv.borrow().as_ref() {
            let script = format!(
                "if(window.setSide) window.setSide('{}'); if(window.animateIn) window.animateIn({}, {});",
                side, bx, by
            );
            let _ = webview.evaluate_script(&script);
        }
    });
}

fn create_panel_webview(panel_hwnd: HWND) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(panel_hwnd, &mut rect);
    }

    let html = if let Ok(app) = APP.lock() {
        let is_dark = match app.config.theme_mode {
            crate::config::ThemeMode::Dark => true,
            crate::config::ThemeMode::Light => false,
            crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        };
        // Update static state to match initial generation
        LAST_THEME_IS_DARK.store(is_dark, Ordering::SeqCst);
        generate_panel_html(
            &app.config.presets,
            &app.config.ui_language,
            is_dark,
            app.config.favorites_keep_open,
        )
    } else {
        String::new()
    };

    let wrapper = HwndWrapper(panel_hwnd);

    // Initialize shared WebContext if needed (uses same data dir as other modules)
    PANEL_WEB_CONTEXT.with(|ctx| {
        if ctx.borrow().is_none() {
            let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
            *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
        }
    });

    let result = PANEL_WEB_CONTEXT.with(|ctx| {
        let mut ctx_ref = ctx.borrow_mut();
        let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
            WebViewBuilder::new_with_web_context(web_ctx)
        } else {
            WebViewBuilder::new()
        };
        let builder = crate::overlay::html_components::font_manager::configure_webview(builder);

        // Store HTML in font server and get URL for same-origin font loading
        let page_url = crate::overlay::html_components::font_manager::store_html_page(html.clone())
            .unwrap_or_else(|| format!("data:text/html,{}", urlencoding::encode(&html)));

        builder
            .with_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    (rect.right - rect.left) as u32,
                    (rect.bottom - rect.top) as u32,
                )),
            })
            .with_url(&page_url)
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
                } else if body == "close_now" {
                    close_panel_internal();
                } else if body.starts_with("trigger:") {
                    if let Ok(idx) = body[8..].parse::<usize>() {
                        // trigger() in JS starts the close animation and will send close_now when done.
                        // We must set IS_EXPANDED to false so close_panel_internal (called by close_now)
                        // actually hides the window. We DON'T call close_panel_internal here to allow animation.
                        IS_EXPANDED.store(false, Ordering::SeqCst);
                        trigger_preset(idx);
                    }
                } else if body.starts_with("trigger_only:") {
                    // Keep Open mode: trigger preset without closing panel
                    if let Ok(idx) = body[13..].parse::<usize>() {
                        trigger_preset(idx);
                    }
                } else if body.starts_with("set_keep_open:") {
                    if let Ok(val) = body[14..].parse::<u32>() {
                        if let Ok(mut app) = APP.lock() {
                            app.config.favorites_keep_open = val == 1;
                            crate::config::save_config(&app.config);
                        }
                    }
                } else if body.starts_with("resize:") {
                    if let Ok(h) = body[7..].parse::<i32>() {
                        resize_panel_height(h);
                    }
                }
            })
            .build_as_child(&wrapper)
    });

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
        WM_ACTIVATE => {
            if wparam.0 == 0 {
                // Window deactivated logic (optional)
            }
            LRESULT(0)
        }
        WM_REFRESH_PANEL => {
            let bubble_hwnd = HWND(BUBBLE_HWND.load(Ordering::SeqCst) as *mut std::ffi::c_void);

            if let Ok(app) = APP.lock() {
                let is_dark = match app.config.theme_mode {
                    crate::config::ThemeMode::Dark => true,
                    crate::config::ThemeMode::Light => false,
                    crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
                };

                // Set expanded to true so it moves with bubble
                IS_EXPANDED.store(true, Ordering::SeqCst);

                refresh_panel_layout_and_content(
                    bubble_hwnd,
                    hwnd,
                    &app.config.presets,
                    &app.config.ui_language,
                    is_dark,
                );
            }
            // Lock released here

            // Correctly call update_bubble_visual outside the lock
            // (update_bubble_visual internally calls is_dark_mode() which locks APP)
            update_bubble_visual(bubble_hwnd);

            LRESULT(0)
        }
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
        // CRITICAL: Restore focus to the original foreground window before triggering.
        // This ensures that text-select presets can send Ctrl+C to the correct window
        // (the one that had text selected before the user clicked on the bubble panel).
        let saved_fg = LAST_FOREGROUND_HWND.load(Ordering::SeqCst);
        if saved_fg != 0 {
            let fg_hwnd = HWND(saved_fg as *mut std::ffi::c_void);
            if !fg_hwnd.is_invalid() {
                // SetForegroundWindow may not always work due to Windows focus stealing prevention,
                // but SetFocus on a window that's already visible should work.
                // We use a combination approach for best results.
                let _ = SetForegroundWindow(fg_hwnd);
                let _ = SetFocus(Some(fg_hwnd));
                // Small delay to allow focus to settle before triggering the preset
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        }

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

fn resize_panel_height(content_height: i32) {
    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val == 0 {
        return;
    }

    // Add a small buffer to ensure no scrollbars appear

    unsafe {
        let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);

        // Get DPI to scale the CSS pixels (content_height) to Physical pixels
        let dpi = GetDpiForWindow(panel_hwnd);
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };

        let new_height_pixels = (content_height as f32 * scale).ceil() as i32 + 20;

        let mut panel_rect = RECT::default();
        let _ = GetWindowRect(panel_hwnd, &mut panel_rect);
        let current_width = panel_rect.right - panel_rect.left;
        let current_height = panel_rect.bottom - panel_rect.top;

        // Only resize if significantly different to avoid jitter loops
        if (current_height - new_height_pixels).abs() < 4 {
            return;
        }

        let bubble_val = BUBBLE_HWND.load(Ordering::SeqCst);
        let bubble_hwnd = if bubble_val != 0 {
            HWND(bubble_val as *mut std::ffi::c_void)
        } else {
            return;
        };

        let mut bubble_rect = RECT::default();
        let _ = GetWindowRect(bubble_hwnd, &mut bubble_rect);

        // Recalculate Y position to keep centered on bubble
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let (_panel_x, panel_y) = if bubble_rect.left > screen_w / 2 {
            (
                bubble_rect.left - current_width - 4,
                bubble_rect.top - new_height_pixels / 2 + BUBBLE_SIZE / 2,
            )
        } else {
            (
                bubble_rect.right + 4,
                bubble_rect.top - new_height_pixels / 2 + BUBBLE_SIZE / 2,
            )
        };

        // Clamp Y
        let actual_panel_y = panel_y.max(10);

        let _ = SetWindowPos(
            panel_hwnd,
            None,
            panel_rect.left, // Keep X
            actual_panel_y,
            current_width,
            new_height_pixels,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS,
        );

        // Update WebView bounds
        PANEL_WEBVIEW.with(|wv| {
            if let Some(webview) = wv.borrow().as_ref() {
                let _ = webview.set_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        current_width as u32,
                        new_height_pixels as u32,
                    )),
                });
            }
        });
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
