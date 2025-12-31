#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod config;
pub mod gui;
mod history;
mod icon_gen;
mod model_config;
mod overlay;
mod updater;
pub mod win_types;
mod repo_consolidator;

use config::{load_config, Config, ThemeMode};
use gui::locale::LocaleText;
use history::HistoryManager;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::panic;
use std::sync::{Arc, Mutex};
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::CoInitialize;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// Window dimensions - Increased to accommodate two-column sidebar and longer text labels
pub const WINDOW_WIDTH: f32 = 1230.0;
pub const WINDOW_HEIGHT: f32 = 620.0;

// Modifier Constants for Hook
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

// Wrappers for thread-safe types now imported from win_types
use crate::win_types::{SendHandle, SendHhook, SendHwnd};

// Global event for inter-process restore signaling (manual-reset event)
lazy_static! {
    pub static ref RESTORE_EVENT: Option<SendHandle> = unsafe {
        CreateEventW(None, true, false, w!("Global\\ScreenGoatedToolboxRestoreEvent")).ok().map(SendHandle)
    };
    // Global handle for the listener window (for the mouse hook to post messages to)
    static ref LISTENER_HWND: Mutex<SendHwnd> = Mutex::new(SendHwnd::default());
    // Global handle for the mouse hook
    static ref MOUSE_HOOK: Mutex<SendHhook> = Mutex::new(SendHhook::default());
}

// 1. Define a wrapper for the GDI Handle to ensure we clean it up
pub struct GdiCapture {
    pub hbitmap: HBITMAP,
    pub width: i32,
    pub height: i32,
}

// Make it safe to send between threads (Handles are process-global in Windows GDI)
unsafe impl Send for GdiCapture {}
unsafe impl Sync for GdiCapture {}

impl Drop for GdiCapture {
    fn drop(&mut self) {
        unsafe {
            if !self.hbitmap.is_invalid() {
                let _ = DeleteObject(self.hbitmap.into());
            }
        }
    }
}

pub struct AppState {
    pub config: Config,
    pub screenshot_handle: Option<GdiCapture>,
    pub hotkeys_updated: bool,
    pub registered_hotkey_ids: Vec<i32>, // Track IDs of currently registered hotkeys
    // New: Track API usage limits (Key: Model Full Name, Value: "Remaining / Total")
    pub model_usage_stats: HashMap<String, String>,
    pub history: Arc<HistoryManager>,         // NEW
    pub last_active_window: Option<SendHwnd>, // NEW: Store window handle for auto-paste focus restoration
}

lazy_static! {
    pub static ref APP: Arc<Mutex<AppState>> = Arc::new(Mutex::new({
        let config = load_config();
        let history = Arc::new(HistoryManager::new(config.max_history_items));
        AppState {
            config,
            screenshot_handle: None,
            hotkeys_updated: false,
            registered_hotkey_ids: Vec::new(),
            model_usage_stats: HashMap::new(),
            history,
            last_active_window: None, // NEW
        }
    }));
}

/// Enable dark mode for Win32 native menus (context menus, tray menus)
/// This uses the undocumented SetPreferredAppMode API from uxtheme.dll
fn enable_dark_mode_for_app() {
    use windows::core::w;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    // PreferredAppMode enum values
    const ALLOW_DARK: u32 = 1; // AllowDark mode

    unsafe {
        // Load uxtheme.dll
        if let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) {
            // SetPreferredAppMode is at ordinal 135 (undocumented)
            // MAKEINTRESOURCEA(135) is just the number 135 cast to PCSTR
            let ordinal = 135u16;
            let ordinal_ptr = ordinal as usize as *const u8;
            let proc_name = windows::core::PCSTR::from_raw(ordinal_ptr);

            if let Some(set_preferred_app_mode) = GetProcAddress(uxtheme, proc_name) {
                // Cast to function pointer: fn(u32) -> u32
                let func: extern "system" fn(u32) -> u32 =
                    std::mem::transmute(set_preferred_app_mode);
                func(ALLOW_DARK);
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    // --- INIT COM ---
    // Essential for Tray Icon and Shell interactions, especially in Admin/Task Scheduler context.
    unsafe {
        let _ = CoInitialize(None);
    }

    // --- ENABLE DARK MODE FOR NATIVE MENUS ---
    // Uses undocumented Windows API to make context menus respect system dark theme
    enable_dark_mode_for_app();

    // --- APPLY PENDING UPDATE ---
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let staging_path = exe_dir.join("update_pending.exe");
            let backup_path = exe_path.with_extension("exe.old");

            // If there's a pending update, apply it
            if staging_path.exists() {
                // Backup current exe
                let _ = std::fs::copy(&exe_path, &backup_path);
                // Replace with staged exe
                if std::fs::rename(&staging_path, &exe_path).is_ok() {
                    // Success - cleanup temp file
                    let _ = std::fs::remove_file("temp_download");
                }
            }

            // --- CLEANUP OLD EXE FILES ---
            let current_exe_name = exe_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if let Ok(entries) = std::fs::read_dir(exe_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();

                    // Delete old ScreenGoatedToolbox_v*.exe files (keep only current)
                    if (name_str.starts_with("ScreenGoatedToolbox_v") && name_str.ends_with(".exe"))
                        && name_str.as_ref() != current_exe_name
                    {
                        let _ = std::fs::remove_file(entry.path());
                    }

                    // Delete .old backup files
                    if name_str.ends_with(".exe.old") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }

    // --- CRASH HANDLER START ---
    panic::set_hook(Box::new(|panic_info| {
        // 1. Format the error message
        let location = if let Some(location) = panic_info.location() {
            format!("File: {}\nLine: {}", location.file(), location.line())
        } else {
            "Unknown location".to_string()
        };

        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        let error_msg = format!(
            "CRASH DETECTED!\n\nError: {}\n\nLocation:\n{}",
            payload, location
        );

        // Show a Windows Message Box so the user knows it crashed
        let wide_msg: Vec<u16> = error_msg.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_title: Vec<u16> = "SGT Crash Report"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            MessageBoxW(
                None,
                PCWSTR(wide_msg.as_ptr()),
                PCWSTR(wide_title.as_ptr()),
                MB_ICONERROR | MB_OK,
            );
        }
    }));
    // --- CRASH HANDLER END ---

    // Ensure the named event exists (for first instance, for second instance to signal)
    let _ = RESTORE_EVENT.as_ref();

    // Keep the handle alive for the duration of the program
    let _single_instance_mutex = unsafe {
        let instance = CreateMutexW(
            None,
            true,
            w!("Global\\ScreenGoatedToolboxSingleInstanceMutex"),
        );
        if let Ok(handle) = instance {
            if GetLastError() == ERROR_ALREADY_EXISTS {
                // Another instance is running - signal it to restore
                if let Some(event) = RESTORE_EVENT.as_ref() {
                    let _ = SetEvent(event.0);
                }
                let _ = CloseHandle(handle);
                return Ok(());
            }
            Some(handle)
        } else {
            None
        }
    };

    std::thread::spawn(|| {
        run_hotkey_listener();
    });

    // Initialize TTS for instant speech synthesis
    api::tts::init_tts();

    // Offload warmups to a sequenced thread to prevent splash screen lag
    std::thread::spawn(|| {
        // 0. Warmup fonts first (download/cache for instant display)
        // This runs in background and should complete before first WebView loads
        overlay::html_components::font_manager::warmup_fonts();

        // Helper: Wait for tray popup to close before proceeding
        // This prevents WebView2 focus stealing from closing the popup
        let wait_for_popup_close = || {
            while overlay::tray_popup::is_popup_open() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        };

        // 1. Wait briefly for main window to initialize and show
        // This prevents the warmup window from interfering with main window visibility
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 1. Warmup tray popup (with is_warmup=true to avoid focus stealing)
        wait_for_popup_close();
        overlay::tray_popup::warmup_tray_popup();

        // 1.5 Warmup preset wheel (persistent hidden window)
        overlay::preset_wheel::warmup();

        // 2. Wait for splash screen / main box to appear and settle
        std::thread::sleep(std::time::Duration::from_millis(1500));

        // 3. Warmup text input window first (more likely to be used quickly)
        wait_for_popup_close();
        overlay::text_input::warmup();

        // 4. Wait before next warmup to distribute CPU load
        std::thread::sleep(std::time::Duration::from_millis(2000));

        // 5. Warmup markdown WebView
        wait_for_popup_close();
        overlay::result::markdown_view::warmup();
    });

    // 1. Load config early to get theme setting and language for tray i18n
    let initial_config = APP.lock().unwrap().config.clone();

    // --- TRAY MENU SETUP (with i18n) ---
    let tray_locale = LocaleText::get(&initial_config.ui_language);
    let tray_menu = Menu::new();

    // Favorite bubble toggle - check if any presets are favorited
    let has_favorites = initial_config.presets.iter().any(|p| p.is_favorite);
    let favorite_bubble_text = if has_favorites {
        tray_locale.tray_favorite_bubble
    } else {
        tray_locale.tray_favorite_bubble_disabled
    };
    let tray_favorite_bubble_item = CheckMenuItem::with_id(
        "1003",
        favorite_bubble_text,
        has_favorites, // enabled only if has favorites
        initial_config.show_favorite_bubble && has_favorites,
        None,
    );

    let tray_settings_item = MenuItem::with_id("1002", tray_locale.tray_settings, true, None);
    let tray_quit_item = MenuItem::with_id("1001", tray_locale.tray_quit, true, None);
    let _ = tray_menu.append(&tray_favorite_bubble_item);
    let _ = tray_menu.append(&tray_settings_item);
    let _ = tray_menu.append(&tray_quit_item);

    // --- WINDOW SETUP ---
    let mut viewport_builder = eframe::egui::ViewportBuilder::default()
        .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
        .with_resizable(true)
        .with_visible(false) // Start invisible
        .with_transparent(false)
        .with_decorations(true); // FIX: Start WITH decorations, opaque window

    // 2. Detect System Theme
    let system_dark = gui::utils::is_system_in_dark_mode();

    // 3. Resolve Initial Theme
    let effective_dark = match initial_config.theme_mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => system_dark,
    };

    // 4. Use Effective Theme for initial icon
    let icon_data = crate::icon_gen::get_window_icon(effective_dark);
    viewport_builder = viewport_builder.with_icon(std::sync::Arc::new(icon_data));

    let options = eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    };

    eframe::run_native(
        "Screen Goated Toolbox (SGT by nganlinh4)",
        options,
        Box::new(move |cc| {
            gui::configure_fonts(&cc.egui_ctx);

            // Store global context for background threads
            *gui::GUI_CONTEXT.lock().unwrap() = Some(cc.egui_ctx.clone());

            // 5. Set Initial Visuals Explicitly
            if effective_dark {
                cc.egui_ctx.set_visuals(eframe::egui::Visuals::dark());
            } else {
                cc.egui_ctx.set_visuals(eframe::egui::Visuals::light());
            }

            // 6. Set Native Icon
            gui::utils::update_window_icon_native(effective_dark);

            Ok(Box::new(gui::SettingsApp::new(
                initial_config,
                APP.clone(),
                tray_menu,
                tray_settings_item,
                tray_quit_item,
                tray_favorite_bubble_item,
                cc.egui_ctx.clone(),
            )))
        }),
    )
}

fn register_all_hotkeys(hwnd: HWND) {
    let mut app = APP.lock().unwrap();
    let presets = &app.config.presets;

    let mut registered_ids = Vec::new();
    for (p_idx, preset) in presets.iter().enumerate() {
        for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
            // ID encoding: 1000 * preset_idx + hotkey_idx + 1

            // Skip Mouse Buttons for RegisterHotKey (handled via hook)
            if [0x04, 0x05, 0x06].contains(&hotkey.code) {
                continue;
            }

            let id = (p_idx as i32 * 1000) + (h_idx as i32) + 1;
            unsafe {
                let _ = RegisterHotKey(
                    Some(hwnd),
                    id,
                    HOT_KEY_MODIFIERS(hotkey.modifiers),
                    hotkey.code,
                );
            }
            registered_ids.push(id);
        }
    }
    app.registered_hotkey_ids = registered_ids;
}

fn unregister_all_hotkeys(hwnd: HWND) {
    let app = APP.lock().unwrap();
    for &id in &app.registered_hotkey_ids {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
}

// Low-Level Mouse Hook Procedure
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        let vk_code = match msg {
            WM_MBUTTONDOWN => Some(0x04), // VK_MBUTTON
            WM_XBUTTONDOWN => {
                let info = *(lparam.0 as *const MSLLHOOKSTRUCT);
                let xbutton = (info.mouseData >> 16) & 0xFFFF;
                if xbutton == 1 {
                    Some(0x05)
                }
                // VK_XBUTTON1
                else if xbutton == 2 {
                    Some(0x06)
                }
                // VK_XBUTTON2
                else {
                    None
                }
            }
            _ => None,
        };

        if let Some(vk) = vk_code {
            // Check modifiers using GetAsyncKeyState for real-time state
            let mut mods = 0;
            if (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
                mods |= MOD_ALT;
            }
            if (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
                mods |= MOD_CONTROL;
            }
            if (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 {
                mods |= MOD_SHIFT;
            }
            if (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
            {
                mods |= MOD_WIN;
            }

            // Check config for a match
            let mut found_id = None;
            if let Ok(app) = APP.lock() {
                for (p_idx, preset) in app.config.presets.iter().enumerate() {
                    for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
                        if hotkey.code == vk && hotkey.modifiers == mods {
                            // Synthesize ID same as register_all_hotkeys
                            found_id = Some((p_idx as i32 * 1000) + (h_idx as i32) + 1);
                            break;
                        }
                    }
                    if found_id.is_some() {
                        break;
                    }
                }
            }

            if let Some(id) = found_id {
                if let Ok(hwnd_target) = LISTENER_HWND.lock() {
                    if !hwnd_target.0.is_invalid() {
                        // Post WM_HOTKEY to the listener window logic
                        let _ = PostMessageW(
                            Some(hwnd_target.0),
                            WM_HOTKEY,
                            WPARAM(id as usize),
                            LPARAM(0),
                        );
                        return LRESULT(1); // Consume/Block input
                    }
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

const WM_RELOAD_HOTKEYS: u32 = WM_USER + 101;

fn run_hotkey_listener() {
    unsafe {
        // Error handling: GetModuleHandleW should not fail, but handle it
        let instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => {
                eprintln!("Error: Failed to get module handle for hotkey listener");
                return;
            }
        };

        let class_name = w!("HotkeyListenerClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(hotkey_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        // RegisterClassW can fail if class already exists, which is okay
        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Listener"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        // Error handling: hwnd is invalid if creation failed
        if hwnd.is_invalid() {
            eprintln!("Error: Failed to create hotkey listener window");
            return;
        }

        // Store HWND for the hook
        if let Ok(mut guard) = LISTENER_HWND.lock() {
            *guard = SendHwnd(hwnd);
        }

        // Install Mouse Hook
        if let Ok(hhook) =
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), Some(instance.into()), 0)
        {
            if let Ok(mut hook_guard) = MOUSE_HOOK.lock() {
                *hook_guard = SendHhook(hhook);
            }
        } else {
            eprintln!("Warning: Failed to install low-level mouse hook");
        }

        register_all_hotkeys(hwnd);

        let mut msg = MSG::default();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_RELOAD_HOTKEYS {
                    unregister_all_hotkeys(hwnd);
                    register_all_hotkeys(hwnd);

                    if let Ok(mut app) = APP.lock() {
                        app.hotkeys_updated = false;
                    }
                } else {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

unsafe extern "system" fn hotkey_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            if id > 0 {
                // CRITICAL: If preset wheel is active, dismiss it and return early
                // This allows pressing the hotkey again to dismiss the wheel
                if overlay::preset_wheel::is_wheel_active() {
                    overlay::preset_wheel::dismiss_wheel();
                    return LRESULT(0);
                }

                let preset_idx = ((id - 1) / 1000) as usize;

                // Determine context and fetch hotkey name
                let (preset_type, text_mode, is_audio_stopping, hotkey_name) = {
                    if let Ok(app) = APP.lock() {
                        if preset_idx < app.config.presets.len() {
                            let p = &app.config.presets[preset_idx];
                            let p_type = p.preset_type.clone();
                            let t_mode = p.text_input_mode.clone();
                            let stopping =
                                p_type == "audio" && overlay::is_recording_overlay_active();

                            // Find the specific hotkey name that triggered this
                            let hk_idx = ((id - 1) % 1000) as usize;
                            let hk_name = if hk_idx < p.hotkeys.len() {
                                p.hotkeys[hk_idx].name.clone()
                            } else {
                                String::new()
                            };

                            (p_type, t_mode, stopping, hk_name)
                        } else {
                            (
                                "image".to_string(),
                                "select".to_string(),
                                false,
                                String::new(),
                            )
                        }
                    } else {
                        (
                            "image".to_string(),
                            "select".to_string(),
                            false,
                            String::new(),
                        )
                    }
                };

                // FIX: Only capture target window if we are NOT stopping an audio recording.
                if !is_audio_stopping {
                    let target_window = crate::overlay::utils::get_target_window_for_paste();

                    if let Ok(mut app) = APP.lock() {
                        app.last_active_window = target_window.map(crate::win_types::SendHwnd);
                    }
                }

                if preset_type == "audio" {
                    // Check for realtime mode
                    let is_realtime = {
                        if let Ok(app) = APP.lock() {
                            if preset_idx < app.config.presets.len() {
                                app.config.presets[preset_idx].audio_processing_mode == "realtime"
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if is_realtime {
                        // Realtime mode - toggle realtime overlay
                        // Check if minimal or webview is active
                        let is_minimal_active = overlay::realtime_egui::MINIMAL_ACTIVE
                            .load(std::sync::atomic::Ordering::SeqCst);
                        let is_webview_active = overlay::is_realtime_overlay_active();

                        if is_webview_active {
                            // WebView active - stop it (toggle off)
                            overlay::stop_realtime_overlay();
                        } else if is_minimal_active {
                            // Minimal egui active - do NOT allow hotkey to close (user must use window X)
                            // This prevents buggy behavior
                        } else {
                            // Nothing active - Start
                            std::thread::spawn(move || {
                                overlay::show_realtime_overlay(preset_idx);
                            });
                        }
                    } else {
                        // Record-then-process mode
                        if overlay::is_recording_overlay_active() {
                            overlay::stop_recording_and_submit();
                        } else {
                            std::thread::spawn(move || {
                                overlay::show_recording_overlay(preset_idx);
                            });
                        }
                    }
                } else if preset_type == "text" {
                    // NEW TEXT LOGIC
                    if text_mode == "select" {
                        // Toggle Logic for Selection
                        if overlay::text_selection::is_active() {
                            overlay::text_selection::cancel_selection();
                        } else {
                            // NEW: Try instant processing if text is already selected
                            std::thread::spawn(move || {
                                // First, try to process any already-selected text
                                if !overlay::text_selection::try_instant_process(preset_idx) {
                                    // No pre-selected text - fall back to showing selection tag
                                    overlay::show_text_selection_tag(preset_idx);
                                }
                            });
                        }
                    } else {
                        // Type Mode - Toggle Logic for Input Window
                        if overlay::text_input::is_active() {
                            overlay::text_input::cancel_input();
                        } else {
                            if let Ok(app) = APP.lock() {
                                let config = app.config.clone();
                                let preset = config.presets[preset_idx].clone();
                                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                                let screen_h = GetSystemMetrics(SM_CYSCREEN);
                                let center_rect = RECT {
                                    left: (screen_w - 700) / 2,
                                    top: (screen_h - 300) / 2,
                                    right: (screen_w + 700) / 2,
                                    bottom: (screen_h + 300) / 2,
                                };

                                // Get localized preset name for display
                                let localized_name = gui::settings_ui::get_localized_preset_name(
                                    &preset.id,
                                    &config.ui_language,
                                );

                                let hotkey_name_clone = hotkey_name.clone();
                                std::thread::spawn(move || {
                                    overlay::process::start_text_processing(
                                        String::new(),
                                        center_rect,
                                        config,
                                        preset,
                                        localized_name,
                                        hotkey_name_clone,
                                    );
                                });
                            }
                        }
                    }
                } else {
                    // Image Mode
                    if overlay::is_selection_overlay_active_and_dismiss() {
                        return LRESULT(0);
                    }

                    let app_clone = APP.clone();
                    let p_idx = preset_idx;

                    std::thread::spawn(move || match capture_screen_fast() {
                        Ok(capture) => {
                            if let Ok(mut app) = app_clone.lock() {
                                app.screenshot_handle = Some(capture);
                            } else {
                                return;
                            }
                            overlay::show_selection_overlay(p_idx);
                        }
                        Err(e) => {
                            eprintln!("Capture Error: {}", e);
                        }
                    });
                }
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn capture_screen_fast() -> anyhow::Result<GdiCapture> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        // Validate dimensions
        if width <= 0 || height <= 0 {
            return Err(anyhow::anyhow!(
                "GDI Error: Invalid screen dimensions ({} x {})",
                width,
                height
            ));
        }

        let hdc_screen = GetDC(None);
        if hdc_screen.is_invalid() {
            return Err(anyhow::anyhow!(
                "GDI Error: Failed to get screen device context"
            ));
        }

        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        if hdc_mem.is_invalid() {
            let _ = ReleaseDC(None, hdc_screen);
            return Err(anyhow::anyhow!(
                "GDI Error: Failed to create compatible device context"
            ));
        }

        let hbitmap = CreateCompatibleBitmap(hdc_screen, width, height);

        if hbitmap.is_invalid() {
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(None, hdc_screen);
            return Err(anyhow::anyhow!(
                "GDI Error: Failed to create compatible bitmap."
            ));
        }

        SelectObject(hdc_mem, hbitmap.into());

        // This is the only "heavy" part, but it's purely GPU/GDI memory move. Very fast.
        BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            Some(hdc_screen),
            x,
            y,
            SRCCOPY,
        )?;

        // Cleanup DCs, but KEEP the HBITMAP
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        Ok(GdiCapture {
            hbitmap,
            width,
            height,
        })
    }
}
