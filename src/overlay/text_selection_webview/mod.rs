use crate::APP;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Once,
};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Memory::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

mod html;

// Try to use the shared wrapper which likely implements what's needed
use crate::overlay::realtime_webview::state::HwndWrapper;

// Shared state struct (simplified for WebView)
struct TextSelectionState {
    hwnd: HWND,
    preset_idx: usize,
    is_selecting: bool,
    is_processing: bool,
    hook_handle: HHOOK,
    webview: Option<wry::WebView>,
}
unsafe impl Send for TextSelectionState {}

static SELECTION_STATE: Mutex<TextSelectionState> = Mutex::new(TextSelectionState {
    hwnd: HWND(std::ptr::null_mut()),
    preset_idx: 0,
    is_selecting: false,
    is_processing: false,
    hook_handle: HHOOK(std::ptr::null_mut()),
    webview: None,
});

static REGISTER_TAG_CLASS: Once = Once::new();

lazy_static::lazy_static! {
    pub static ref TAG_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub fn is_active() -> bool {
    !SELECTION_STATE.lock().unwrap().hwnd.is_invalid()
}

pub fn cancel_selection() {
    TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
    let hwnd = SELECTION_STATE.lock().unwrap().hwnd;
    unsafe {
        if !hwnd.is_invalid() {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

// Re-export processing function needed by logic (copied from original)
// Or better: keep original functions locally if possible, or import if moved.
// For now, I'll reproduce the necessary logic here since this module REPLACES the old one.

/// Try to process already-selected text instantly.
pub fn try_instant_process(preset_idx: usize) -> bool {
    unsafe {
        let original_clipboard = get_clipboard_text();
        if OpenClipboard(Some(HWND::default())).is_ok() {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
        std::thread::sleep(std::time::Duration::from_millis(30));

        let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
            let input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                        wScan: 0,
                    },
                },
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        };

        send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0));
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(0x43, KEYBD_EVENT_FLAGS(0));
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(0x43, KEYEVENTF_KEYUP);
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);

        let mut clipboard_text = String::new();
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            clipboard_text = get_clipboard_text();
            if !clipboard_text.is_empty() {
                break;
            }
        }

        if clipboard_text.trim().is_empty() {
            if !original_clipboard.is_empty() {
                crate::overlay::utils::copy_to_clipboard(&original_clipboard, HWND::default());
            }
            return false;
        }

        process_selected_text(preset_idx, clipboard_text);
        true
    }
}

unsafe fn get_clipboard_text() -> String {
    let mut result = String::new();
    if OpenClipboard(Some(HWND::default())).is_ok() {
        if let Ok(h_data) = GetClipboardData(13u32) {
            let h_global: HGLOBAL = std::mem::transmute(h_data);
            let ptr = GlobalLock(h_global);
            if !ptr.is_null() {
                let size = GlobalSize(h_global);
                let wide_slice = std::slice::from_raw_parts(ptr as *const u16, size / 2);
                if let Some(end) = wide_slice.iter().position(|&c| c == 0) {
                    result = String::from_utf16_lossy(&wide_slice[..end]);
                }
            }
            let _ = GlobalUnlock(h_global);
        }
        let _ = CloseClipboard();
    }
    result
}

fn process_selected_text(preset_idx: usize, clipboard_text: String) {
    unsafe {
        let (is_master, _original_mode) = {
            let app = APP.lock().unwrap();
            let p = &app.config.presets[preset_idx];
            (p.is_master, p.text_input_mode.clone())
        };

        let final_preset_idx = if is_master {
            let mut cursor_pos = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            let selected =
                crate::overlay::preset_wheel::show_preset_wheel("text", Some("select"), cursor_pos);
            if let Some(idx) = selected {
                idx
            } else {
                return;
            }
        } else {
            preset_idx
        };

        let (config, mut preset, screen_w, screen_h) = {
            let mut app = APP.lock().unwrap();
            app.config.active_preset_idx = final_preset_idx;
            (
                app.config.clone(),
                app.config.presets[final_preset_idx].clone(),
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        };

        preset.text_input_mode = "select".to_string();

        let center_rect = RECT {
            left: (screen_w - 700) / 2,
            top: (screen_h - 300) / 2,
            right: (screen_w + 700) / 2,
            bottom: (screen_h + 300) / 2,
        };
        let localized_name =
            crate::gui::settings_ui::get_localized_preset_name(&preset.id, &config.ui_language);
        let cancel_hotkey = preset
            .hotkeys
            .first()
            .map(|h| h.name.clone())
            .unwrap_or_default();

        crate::overlay::process::start_text_processing(
            clipboard_text,
            center_rect,
            config,
            preset,
            localized_name,
            cancel_hotkey,
        );
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kbd_struct = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize {
            if kbd_struct.vkCode == VK_ESCAPE.0 as u32 {
                TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

pub fn show_text_selection_tag(preset_idx: usize) {
    unsafe {
        // Scope 1: Check and Init
        {
            let mut state = SELECTION_STATE.lock().unwrap();
            if !state.hwnd.is_invalid() {
                // If already active, toggle off (cancel)
                // We need to drop the lock before calling cancel_selection to avoid deadlock
                // if cancel_selection tries to lock (it does).
                drop(state);
                cancel_selection();
                return;
            }

            state.preset_idx = preset_idx;
            state.is_selecting = false;
            state.is_processing = false;
            TAG_ABORT_SIGNAL.store(false, Ordering::SeqCst);
        }

        // Initialize COM for WebView
        unsafe {
            use windows::Win32::System::Com::*;
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextTag_Web");

        REGISTER_TAG_CLASS.call_once(|| {
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(tag_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassExW(&wc);
        });

        let hwnd = CreateWindowExW(
            // Key difference: WS_EX_TRANSPARENT + WS_EX_LAYERED for click-through and transparency
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT Tag Web"),
            WS_POPUP, // Initially hidden offscreen
            -1000,
            -1000,
            200,
            100, // Slightly taller for glow
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        {
            SELECTION_STATE.lock().unwrap().hwnd = hwnd;
        }

        // Install Keyboard Hook to swallow ESC
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            Some(GetModuleHandleW(None).unwrap().into()),
            0,
        );
        if let Ok(h) = hook {
            SELECTION_STATE.lock().unwrap().hook_handle = h;
        }

        // Initialize WebView
        let (is_dark, lang) = {
            let app = APP.lock().unwrap();
            let is_dark = match app.config.theme_mode {
                crate::config::ThemeMode::Dark => true,
                crate::config::ThemeMode::Light => false,
                crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
            };
            (is_dark, app.config.ui_language.clone())
        };

        let initial_text = match lang.as_str() {
            "vi" => "Bôi đen văn bản...",
            "ko" => "텍스트 선택...",
            _ => "Select text...",
        };

        let html = html::get_html(is_dark, initial_text);

        // Use ephemeral context to avoid lock issues
        let mut web_context = wry::WebContext::new(None);

        // Fix font loading: use data URL or robust loading
        let page_url = format!("data:text/html,{}", urlencoding::encode(&html));

        let builder = wry::WebViewBuilder::new_with_web_context(&mut web_context);
        let webview = {
            // LOCK SCOPE: Serialized build to prevent resource contention
            let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();
            crate::log_info!("[SelectionV2] Acquired init lock. Building...");

            let build_res = builder
                .with_bounds(wry::Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(200, 100)),
                })
                .with_url(&page_url) // Use data URL directly for simplicity in this ephemeral window
                .with_transparent(true)
                .build_as_child(&HwndWrapper(hwnd));

            crate::log_info!(
                "[SelectionV2] Build finished. Status: {}",
                if build_res.is_ok() { "OK" } else { "ERR" }
            );
            build_res
        };

        if let Ok(webview) = webview {
            SELECTION_STATE.lock().unwrap().webview = Some(webview);
        } else {
            eprintln!("Failed to create TextSelection WebView");
        }

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut msg = MSG::default();

        // Game Loop
        loop {
            // 1. Process messages
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                if msg.message == WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if msg.message == WM_QUIT {
                break;
            }

            // 2. Abort check
            if TAG_ABORT_SIGNAL.load(Ordering::SeqCst) {
                cleaned_exit(hwnd);
                break;
            }

            // 3. Logic & Movement
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let target_x = pt.x - 30;
            let target_y = pt.y - 60; // Offset to sit above cursor

            // Move Window
            let _ = MoveWindow(hwnd, target_x, target_y, 200, 100, false);

            // Logic for selection state
            let lbutton_down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0;
            let mut should_spawn_thread = false;
            let mut preset_idx_for_thread = 0;

            let update_js = {
                let mut state = SELECTION_STATE.lock().unwrap();

                let was_selecting = state.is_selecting;

                if !state.is_selecting && lbutton_down {
                    state.is_selecting = true;
                } else if state.is_selecting && !lbutton_down && !state.is_processing {
                    state.is_processing = true;
                    should_spawn_thread = true;
                    preset_idx_for_thread = state.preset_idx;
                }

                if state.is_selecting != was_selecting {
                    let new_text = if state.is_selecting {
                        match lang.as_str() {
                            "vi" => "Thả chuột để xử lý",
                            "ko" => "처리를 위해 마우스를 놓으세요",
                            _ => "Release to process",
                        }
                    } else {
                        initial_text
                    };

                    Some(format!(
                        "updateState({}, '{}')",
                        state.is_selecting, new_text
                    ))
                } else {
                    None
                }
            };

            if let Some(js) = update_js {
                if let Some(webview) = SELECTION_STATE.lock().unwrap().webview.as_ref() {
                    let _ = webview.evaluate_script(&js);
                }
            }

            // 4. Handle Thread Spawn
            if should_spawn_thread {
                let hwnd_val = hwnd.0 as isize;
                std::thread::spawn(move || {
                    let hwnd = HWND(hwnd_val as *mut _);
                    // logic similar to original, waiting for clipboard
                    // ...
                    // For brevity in this reimplantation step, I'll inline the core wait-and-process logic logic or reuse helpers?
                    // Cloning closure logic is cleanest to ensure thread safety
                    if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));

                    // Clear clipboard
                    if OpenClipboard(Some(HWND::default())).is_ok() {
                        let _ = EmptyClipboard();
                        let _ = CloseClipboard();
                    }

                    // Send Copy
                    // ... reuse key sending logic ...
                    let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
                        let input = INPUT {
                            r#type: INPUT_KEYBOARD,
                            Anonymous: INPUT_0 {
                                ki: KEYBDINPUT {
                                    wVk: VIRTUAL_KEY(vk),
                                    dwFlags: flags,
                                    time: 0,
                                    dwExtraInfo: 0,
                                    wScan: 0,
                                },
                            },
                        };
                        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                    };
                    send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0));
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    send_input_event(0x43, KEYBD_EVENT_FLAGS(0));
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    send_input_event(0x43, KEYEVENTF_KEYUP);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);

                    let mut clipboard_text = String::new();
                    for _ in 0..10 {
                        if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(25));
                        clipboard_text = get_clipboard_text();
                        if !clipboard_text.is_empty() {
                            break;
                        }
                    }

                    // We need to signal the MAIN loop to close.
                    // The HWND is valid.
                    // But we can't capture the HWND easily across threads safely if not Copy?
                    // HWND is Copy.
                    let hwnd_target = HWND(hwnd.0);

                    if !clipboard_text.trim().is_empty()
                        && !TAG_ABORT_SIGNAL.load(Ordering::Relaxed)
                    {
                        process_selected_text(preset_idx_for_thread, clipboard_text);
                        let _ = PostMessageW(Some(hwnd_target), WM_CLOSE, WPARAM(0), LPARAM(0));
                    } else {
                        // Reset state if no text found
                        // We need to synchronize this reset.
                        // But we can't easily access the mutex from here without Arc<Mutex>.
                        // Actually SELECTION_STATE is global static Mutex available everywhere.
                        let mut state = SELECTION_STATE.lock().unwrap();
                        state.is_selecting = false;
                        state.is_processing = false;
                    }
                });
            }

            // Sleep to yield CPU, standard framerate (~60fps)
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        // Ensure cleanup happens regardless of how the loop exited
        cleaned_exit(hwnd);
    }
}

unsafe fn cleaned_exit(hwnd: HWND) {
    let mut state = SELECTION_STATE.lock().unwrap();
    state.webview = None; // Drop WebView
    if !state.hook_handle.is_invalid() {
        let _ = UnhookWindowsHookEx(state.hook_handle);
        state.hook_handle = HHOOK::default();
    }
    state.hwnd = HWND::default();
    let _ = DestroyWindow(hwnd);
}

unsafe extern "system" fn tag_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // catch unwind
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match msg {
        WM_CLOSE => {
            TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }));
    match result {
        Ok(lresult) => lresult,
        Err(_) => {
            eprintln!("Panic in tag_wnd_proc");
            // Try to provide default processing if panic occurred
            windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}
