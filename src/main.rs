#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod assets;
mod config;
mod debug_log;
pub mod gui;
mod history;
mod icon_gen;
mod model_config;
mod overlay;
mod registry_integration;
mod updater;
pub mod win_types;

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
pub const WINDOW_HEIGHT: f32 = 650.0;

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

/// Cleanup temporary files left by the application (e.g. restart scripts, partial downloads)
fn cleanup_temporary_files() {
    // 1. Clean up restart scripts in %TEMP%
    let temp_dir = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("sgt_restart_") && name_str.ends_with(".bat") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    // 2. Clean up partial downloads in the app's bin directory
    let bin_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("screen-goated-toolbox")
        .join("bin");

    if bin_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&bin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "tmp") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    // 3. Clean up any update-related files in current directory
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let temp_download = exe_dir.join("temp_download");
            if temp_download.exists() {
                let _ = std::fs::remove_file(temp_download);
            }
        }
    }
}

mod unpack_dlls;

fn main() -> eframe::Result<()> {
    crate::log_info!("========================================");
    crate::log_info!(
        "Screen Goated Toolbox v{} STARTUP",
        env!("CARGO_PKG_VERSION")
    );
    crate::log_info!("========================================");

    // --- UNPACK DLLS ---
    // Extract embedded CRT and DirectML DLLs so the app is truly portable
    unpack_dlls::unpack_dlls();

    // --- CLEANUP TEMP FILES ---
    // Remove leftover restart scripts or partial downloads
    cleanup_temporary_files();

    // --- ENSURE CONTEXT MENU ENTRY ---
    crate::log_info!("Ensuring context menu entry...");
    registry_integration::ensure_context_menu_entry();
    crate::log_info!("Context menu entry ensured.");

    // --- INIT COM ---
    // Essential for Tray Icon and Shell interactions, especially in Admin/Task Scheduler context.
    unsafe {
        let _ = CoInitialize(None);
        // Force Per-Monitor V2 DPI Awareness for correct screen metrics and sharp visuals
        if let Ok(hidpi) = LoadLibraryW(w!("user32.dll")) {
            if let Some(set_context) = GetProcAddress(
                hidpi,
                PCSTR::from_raw("SetProcessDpiAwarenessContext\0".as_ptr()),
            ) {
                let func: extern "system" fn(isize) -> BOOL = std::mem::transmute(set_context);
                // -4 is DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                let _ = func(-4);
            }
        }
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
                // Another instance is running - pass arguments via temp file and signal it
                let args: Vec<String> = std::env::args().collect();
                for arg in args.iter().skip(1) {
                    if arg.starts_with("--") {
                        continue;
                    }
                    let path = std::path::PathBuf::from(arg);
                    if path.exists() && path.is_file() {
                        let temp_file = std::env::temp_dir().join("sgt_pending_file.txt");
                        if let Ok(mut f) = std::fs::File::create(temp_file) {
                            use std::io::Write;
                            let _ = write!(f, "{}", path.to_string_lossy());
                        }
                        break;
                    }
                }

                if let Some(event) = RESTORE_EVENT.as_ref() {
                    let _ = SetEvent(event.0);
                }
                let _ = CloseHandle(handle);
                // Exit successfully after signaling
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

    // Initialize Gemini Live LLM connection pool
    api::gemini_live::init_gemini_live();

    // --- CHECK FOR RESTARTED FLAG AND FILE ARGUMENTS ---
    let args: Vec<String> = std::env::args().collect();
    let mut pending_file_path: Option<std::path::PathBuf> = None;

    if args.iter().any(|arg| arg == "--restarted") {
        std::thread::spawn(|| {
            // Wait for app and overlays to settle before showing notification
            std::thread::sleep(std::time::Duration::from_millis(2500));
            overlay::auto_copy_badge::show_update_notification(
                "Đã khởi động lại app để khôi phục hoàn toàn",
            );
        });
    }

    // Check for potential file path in arguments (drag-and-drop or context menu)
    for arg in args.iter().skip(1) {
        // Skip flags
        if arg.starts_with("--") {
            continue;
        }
        let path = std::path::PathBuf::from(arg);
        if path.exists() && path.is_file() {
            crate::log_info!("Check arguments: Found valid file path: {:?}", path);
            pending_file_path = Some(path);
            break; // Handle only one file for now
        } else {
            crate::log_info!("Check arguments: Invalid path or not a file: {:?}", arg);
        }
    }

    // --- CLEAR WEBVIEW DATA IF SCHEDULED (before any WebViews are created) ---
    {
        let mut config = APP.lock().unwrap();
        if config.config.clear_webview_on_startup {
            // Clear WebView data - should succeed since no WebViews exist yet
            overlay::clear_webview_permissions();
            // Reset the flag
            config.config.clear_webview_on_startup = false;
            // Save immediately
            config::save_config(&config.config);
        }
    }

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

        // 1. Start warmups immediately so they are DONE before the splash appears.
        // This ensures the tray popup and bubble are high-priority and ready.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 1. Warmup tray popup (with is_warmup=true to avoid focus stealing)
        wait_for_popup_close();
        overlay::tray_popup::warmup_tray_popup();

        // 1.5 Warmup preset wheel (persistent hidden window)
        overlay::preset_wheel::warmup();

        // 2. Wait for splash screen / main box to appear and settle
        std::thread::sleep(std::time::Duration::from_millis(3000));

        // 3. Warmup text input window first (more likely to be used quickly)
        wait_for_popup_close();
        overlay::text_input::warmup();

        // 3.5 Warmup auto copy badge
        wait_for_popup_close();
        overlay::auto_copy_badge::warmup();

        // 3.75 Warmup text selection tag (native GDI)
        wait_for_popup_close();
        overlay::text_selection::warmup();

        // 7. Wait before realtime warmup (Wait duration preserved for safety)
        std::thread::sleep(std::time::Duration::from_millis(5000));

        // 9. Warmup Recording Overlay
        wait_for_popup_close();
        overlay::recording::warmup_recording_overlay();
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
        .with_transparent(true)
        .with_decorations(false); // Enable custom title bar by disabling native decorations

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
                pending_file_path,
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

    // Register Global Screen Record Hotkey (ID: 9999)
    let sr_hotkey = &app.config.screen_record_hotkey;
    unsafe {
        let _ = RegisterHotKey(
            Some(hwnd),
            9999,
            HOT_KEY_MODIFIERS(sr_hotkey.modifiers),
            sr_hotkey.code,
        );
    }
}

fn unregister_all_hotkeys(hwnd: HWND) {
    let app = APP.lock().unwrap();
    for &id in &app.registered_hotkey_ids {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
    // Unregister Global SR Hotkey
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), 9999);
    }
}

// Low-Level Mouse Hook Procedure
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        let vk_code = match msg {
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                crate::overlay::screen_record::engine::IS_MOUSE_CLICKED
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                if msg == WM_MBUTTONDOWN {
                    Some(0x04)
                } else {
                    None
                }
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                crate::overlay::screen_record::engine::IS_MOUSE_CLICKED
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                None
            }
            WM_XBUTTONDOWN => {
                crate::overlay::screen_record::engine::IS_MOUSE_CLICKED
                    .store(true, std::sync::atomic::Ordering::SeqCst);
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
            WM_XBUTTONUP => {
                crate::overlay::screen_record::engine::IS_MOUSE_CLICKED
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                None
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

                // Check Global Screen Record Hotkey
                if found_id.is_none() {
                    let sr_hk = &app.config.screen_record_hotkey;
                    if sr_hk.code == vk && sr_hk.modifiers == mods {
                        found_id = Some(9999);
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
const WM_APP_PROCESS_PENDING_FILE: u32 = WM_USER + 102;

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

        // Spawn thread to wait for RESTORE_EVENT
        let listener_hwnd_val = hwnd.0 as isize;
        std::thread::spawn(move || {
            if let Some(event) = RESTORE_EVENT.as_ref() {
                loop {
                    // Wait indefinitely for signal
                    if WaitForSingleObject(event.0, INFINITE) == WAIT_OBJECT_0 {
                        // Signal received! Post message to main thread/listener window
                        let _ = PostMessageW(
                            Some(HWND(listener_hwnd_val as *mut _)),
                            WM_APP_PROCESS_PENDING_FILE,
                            WPARAM(0),
                            LPARAM(0),
                        );
                        // Reset event to wait for next signal
                        let _ = ResetEvent(event.0);
                    }
                }
            }
        });

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
        WM_APP_PROCESS_PENDING_FILE => {
            // Read temp file
            let temp_file = std::env::temp_dir().join("sgt_pending_file.txt");
            if temp_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&temp_file) {
                    let path = std::path::PathBuf::from(content.trim());
                    if path.exists() {
                        crate::log_info!("HOTKEY LISTENER: Processing pending file: {:?}", path);
                        // Spawn a thread to avoid blocking the hotkey listener (hook) thread
                        let path_clone = path.clone();
                        std::thread::spawn(move || {
                            crate::gui::app::input_handler::process_file_path(&path_clone);
                        });
                    }
                }
                // Cleanup
                let _ = std::fs::remove_file(temp_file);
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            if id == 9999 {
                // Toggle Screen Recording
                crate::overlay::screen_record::toggle_recording();
                return LRESULT(0);
            }
            if id > 0 {
                // debounce logic
                static mut LAST_HOTKEY_TIMESTAMP: Option<std::time::Instant> = None;

                let now = std::time::Instant::now();
                let is_repeat = unsafe {
                    if let Some(t) = LAST_HOTKEY_TIMESTAMP {
                        // 150ms debounce
                        if now.duration_since(t).as_millis() < 150 {
                            true
                        } else {
                            LAST_HOTKEY_TIMESTAMP = Some(now);
                            false
                        }
                    } else {
                        LAST_HOTKEY_TIMESTAMP = Some(now);
                        false
                    }
                };

                // Valid Hotkey Received - Update Heartbeat
                if !is_repeat {
                    overlay::continuous_mode::reset_heartbeat();
                }
                overlay::continuous_mode::update_last_trigger_time();

                if is_repeat {
                    return LRESULT(0);
                }

                // Check if continuous mode is active or pending
                let mut just_activated_continuous = false;
                if overlay::continuous_mode::is_active() {
                    // Continuous mode is ACTIVE.
                    // Ignore ALL hotkey presses - let the keyboard hooks handle exit (ESC or hotkey tap).
                    // This prevents deactivation during text processing when tag is hidden.
                    return LRESULT(0);
                } else if overlay::continuous_mode::is_pending_start() {
                    // Scenario 2. Continuous Mode is PENDING (e.g. from Bubble).
                    // We must NOT cancel. We promote to ACTIVE and let the logic proceed to trigger the preset.
                    let current = overlay::continuous_mode::get_preset_idx();
                    let hotkey = overlay::continuous_mode::get_hotkey_name();
                    overlay::continuous_mode::activate(current, hotkey);
                    just_activated_continuous = true;
                    // Do NOT return. Proceed to trigger logic below.
                }

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
                                // Store hotkey info for continuous mode detection
                                let hk = &p.hotkeys[hk_idx];
                                if overlay::continuous_mode::supports_continuous_mode(&p_type) {
                                    crate::log_info!("[Hotkey] Setting current hotkey for hold detection: mods={}, code={}, name='{}'", hk.modifiers, hk.code, hk.name);
                                    overlay::continuous_mode::set_current_hotkey(
                                        hk.modifiers,
                                        hk.code,
                                    );
                                    overlay::continuous_mode::set_latest_hotkey_name(
                                        hk.name.clone(),
                                    );
                                }
                                hk.name.clone()
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
                        if overlay::text_selection::is_active()
                            || overlay::text_selection::is_warming_up()
                        {
                            // Ignore repeat hotkeys to allow "hold to activate" or during warmup
                            overlay::continuous_mode::update_last_trigger_time();
                            return LRESULT(0);
                        } else if overlay::continuous_mode::is_active()
                            && !just_activated_continuous
                        {
                            // Continuous mode is active - the worker thread's retrigger handles showing the tag
                            // Just ignore hotkey repeats to prevent duplicate notifications
                            overlay::continuous_mode::update_last_trigger_time();
                            return LRESULT(0);
                        } else {
                            // NEW: Try instant processing if text is already selected
                            // Prevent duplicate processing if already running
                            if overlay::text_selection::is_processing() {
                                return LRESULT(0);
                            }

                            std::thread::spawn(move || {
                                // 1. Show Badge IMMEDIATELY (Decoupled)
                                overlay::show_text_selection_tag(preset_idx);

                                // 2. Try processing in background
                                let success =
                                    overlay::text_selection::try_instant_process(preset_idx);

                                if success {
                                    // If we successfully processed text, we don't need the badge anymore.
                                    overlay::text_selection::cancel_selection();
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
                    // STRICT Debounce/Blocking for "Hold to Activate"
                    if overlay::is_busy() || overlay::is_selection_overlay_active() {
                        // User is still holding/pressing the key
                        overlay::continuous_mode::update_last_trigger_time();
                        return LRESULT(0);
                    }

                    // Set BUSY flag immediately on Main Thread to block repeats
                    overlay::set_is_busy(true);

                    let app_clone = APP.clone();
                    let mut p_idx = preset_idx;
                    std::thread::spawn(move || {
                        loop {
                            // 1. Capture Logic
                            match capture_screen_fast() {
                                Ok(capture) => {
                                    if let Ok(mut app) = app_clone.lock() {
                                        app.screenshot_handle = Some(capture);
                                    } else {
                                        break;
                                    }

                                    // 2. Show Overlay (BLOCKING)
                                    overlay::show_selection_overlay(p_idx);
                                }
                                Err(e) => {
                                    eprintln!("Capture Error: {}", e);
                                    break;
                                }
                            }

                            // 3. Check for exit or update preset
                            if !overlay::continuous_mode::is_active() {
                                break;
                            }

                            // Update p_idx in case it changed (e.g. from Master Preset wheel selection)
                            let current_active_idx = overlay::continuous_mode::get_preset_idx();
                            if current_active_idx != p_idx {
                                p_idx = current_active_idx;
                            }

                            // Small delay before retriggering to prevent tight looping
                            std::thread::sleep(std::time::Duration::from_millis(200));
                        }
                        // Ensure flag is cleared on exit
                        overlay::set_is_busy(false);
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
