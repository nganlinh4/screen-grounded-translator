use super::types::{SettingsApp, UserEvent, RESTORE_SIGNAL};
use crate::config::{Config, ThemeMode};
use crate::gui::settings_ui::ViewMode;
use crate::gui::utils::get_monitor_names;
use crate::updater::{UpdateStatus, Updater};
use auto_launch::AutoLaunch;
use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem},
    MouseButton, TrayIconBuilder, TrayIconEvent,
};
use windows::core::*;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

impl SettingsApp {
    pub fn new(
        mut config: Config,
        app_state: Arc<Mutex<crate::AppState>>,
        tray_menu: Menu,
        tray_settings_item: MenuItem,
        tray_quit_item: MenuItem,
        tray_favorite_bubble_item: CheckMenuItem,
        ctx: egui::Context,
        pending_file_path: Option<std::path::PathBuf>,
    ) -> Self {
        // Unified app name for both Debug and Release to share the same registry/task spot
        let app_name = "ScreenGoatedToolbox";
        let app_path = std::env::current_exe().unwrap();
        let app_path_str = app_path.to_str().unwrap_or("");
        let args: &[&str] = &[];

        let auto = AutoLaunch::new(app_name, app_path_str, args);

        // Check for current admin state early
        let current_admin_state = if cfg!(target_os = "windows") {
            crate::gui::utils::is_running_as_admin()
        } else {
            false
        };

        // --- STARTUP LOGIC REVAMP v2 (ONLY ONE WINS) ---
        let mut run_at_startup_ui = config.run_at_startup || config.run_as_admin_on_startup;

        // Ensure authorized path is set if startup is enabled
        if run_at_startup_ui && config.authorized_startup_path.is_empty() {
            config.authorized_startup_path = app_path_str.to_string();
        }

        // 1. Initial Sync: If system has it enabled but config doesn't, sync to true (Migration)
        let mut registry_enabled_in_system = false;
        #[cfg(target_os = "windows")]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok(key) = hkcu.open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                KEY_READ,
            ) {
                if key.get_value::<String, &str>(app_name).is_ok() {
                    registry_enabled_in_system = true;
                }
            }
        }
        if !registry_enabled_in_system && auto.is_enabled().unwrap_or(false) {
            registry_enabled_in_system = true;
        }

        if registry_enabled_in_system && !config.run_at_startup && !config.run_as_admin_on_startup {
            config.run_at_startup = true;
            // Also authorize the path currently in registry if ours is empty
            if config.authorized_startup_path.is_empty() {
                config.authorized_startup_path = app_path_str.to_string();
            }
        }

        let task_exists = crate::gui::utils::is_admin_startup_enabled();
        if task_exists && !config.run_as_admin_on_startup {
            config.run_as_admin_on_startup = true;
            if config.authorized_startup_path.is_empty() {
                config.authorized_startup_path = app_path_str.to_string();
            }
        }

        // 2. Determine if WE are authorized to manage startup
        let is_authorized = if config.authorized_startup_path.is_empty() {
            // No one is authorized? We take it.
            true
        } else if config.authorized_startup_path == app_path_str {
            // We are the chosen one.
            true
        } else {
            // Someone else is authorized. Do they still exist?
            let other_exists = std::path::Path::new(&config.authorized_startup_path).exists();
            if !other_exists {
                // The authorized version is gone (likely an update/rename). We take over.
                true
            } else {
                // The authorized version still exists (likely Debug vs Release co-existing).
                // We stay quiet to avoid "Both starting" or "Hijacking".
                false
            }
        };

        // 3. Apply intent & Auto-fix (ONLY if authorized)
        if is_authorized {
            // Update authorization if it was empty or changed due to "not exists"
            if config.authorized_startup_path != app_path_str
                && (config.run_at_startup || config.run_as_admin_on_startup)
            {
                config.authorized_startup_path = app_path_str.to_string();
            }

            if config.run_as_admin_on_startup {
                run_at_startup_ui = true;
                if !crate::gui::utils::is_admin_startup_pointing_to_current_exe()
                    && current_admin_state
                {
                    crate::gui::utils::set_admin_startup(true);
                }
            } else if config.run_at_startup {
                run_at_startup_ui = true;
                let _ = auto.enable();
            }
        } else {
            // If not authorized, UI state just reflects intent, but we don't repair/fix.
            run_at_startup_ui = config.run_at_startup || config.run_as_admin_on_startup;
        }

        let run_at_startup = run_at_startup_ui;

        let (tx, rx) = channel();

        // Tray thread
        let tx_tray = tx.clone();
        let ctx_tray = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = TrayIconEvent::receiver().recv() {
                match &event {
                    TrayIconEvent::Click {
                        button: MouseButton::Right,
                        ..
                    } => {
                        // Handle right-click directly - show popup even when main window is hidden
                        crate::overlay::tray_popup::show_tray_popup();
                    }
                    _ => {
                        // Other events go through the normal channel
                        let _ = tx_tray.send(UserEvent::Tray(event));
                        ctx_tray.request_repaint();
                    }
                }
            }
        });

        // Restore signal listener
        let ctx_restore = ctx.clone();
        std::thread::spawn(move || loop {
            unsafe {
                match OpenEventW(
                    EVENT_ALL_ACCESS,
                    false,
                    w!("Global\\ScreenGoatedToolboxRestoreEvent"),
                ) {
                    Ok(event_handle) => {
                        let result = WaitForSingleObject(event_handle, INFINITE);
                        if result == WAIT_OBJECT_0 {
                            let class_name = w!("eframe");
                            let mut hwnd = FindWindowW(class_name, None).unwrap_or_default();
                            if hwnd.is_invalid() {
                                let title = w!("Screen Goated Toolbox (SGT by nganlinh4)");
                                hwnd = FindWindowW(None, title).unwrap_or_default();
                            }
                            if !hwnd.is_invalid() {
                                let _ = ShowWindow(hwnd, SW_RESTORE);
                                let _ = ShowWindow(hwnd, SW_SHOW);
                                let _ = SetForegroundWindow(hwnd);
                                let _ = SetFocus(Some(hwnd));
                            }
                            RESTORE_SIGNAL.store(true, Ordering::SeqCst);
                            ctx_restore.request_repaint();
                            let _ = ResetEvent(event_handle);
                        }
                        let _ = CloseHandle(event_handle);
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
                }
            }
        });

        // Menu thread
        let tx_menu = tx.clone();
        let ctx_menu = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = MenuEvent::receiver().recv() {
                match event.id.0.as_str() {
                    "1001" => std::process::exit(0),
                    "1002" => {
                        unsafe {
                            let class_name = w!("eframe");
                            let hwnd = FindWindowW(class_name, None).unwrap_or_default();
                            let hwnd = if hwnd.is_invalid() {
                                let title = w!("Screen Goated Toolbox (SGT by nganlinh4)");
                                FindWindowW(None, title).unwrap_or_default()
                            } else {
                                hwnd
                            };
                            if !hwnd.is_invalid() {
                                let _ = ShowWindow(hwnd, SW_RESTORE);
                                let _ = ShowWindow(hwnd, SW_SHOW);
                                let _ = SetForegroundWindow(hwnd);
                                let _ = SetFocus(Some(hwnd));
                            }
                        }
                        RESTORE_SIGNAL.store(true, Ordering::SeqCst);
                        let _ = tx_menu.send(UserEvent::Menu(event.clone()));
                        ctx_menu.request_repaint();
                    }
                    _ => {
                        let _ = tx_menu.send(UserEvent::Menu(event));
                        ctx_menu.request_repaint();
                    }
                }
            }
        });

        let view_mode = if config.presets.is_empty() {
            ViewMode::Global
        } else {
            ViewMode::Preset(if config.active_preset_idx < config.presets.len() {
                config.active_preset_idx
            } else {
                0
            })
        };

        let cached_monitors = get_monitor_names();
        let (up_tx, up_rx) = channel();

        // --- Init Audio Device Cache ---
        let cached_audio_devices = Arc::new(Mutex::new(Vec::new()));
        let devices_clone = cached_audio_devices.clone();
        // Fetch in background
        std::thread::spawn(move || {
            let devices = crate::api::tts::TtsManager::get_output_devices();
            if let Ok(mut lock) = devices_clone.lock() {
                *lock = devices;
            }
        });

        // Detect initial system theme
        let system_dark = crate::gui::utils::is_system_in_dark_mode();

        // Determine effective initial theme
        let effective_dark = match config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_dark,
        };

        let initial_ui_language = config.ui_language.clone(); // Extract before move
        let rng_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;

        // Initialize tray item state
        tray_favorite_bubble_item.set_checked(config.show_favorite_bubble);

        // Capture bubble state before config is moved
        let initial_bubble_enabled = config.show_favorite_bubble;
        let initial_has_favorites = config.presets.iter().any(|p| p.is_favorite);

        // Create tray icon immediately to avoid splash delay
        let icon = crate::icon_gen::get_tray_icon(effective_dark);
        let tray_icon = match TrayIconBuilder::new()
            .with_tooltip("Screen Goated Toolbox (nganlinh4)")
            .with_icon(icon)
            .build()
        {
            Ok(t) => Some(t),
            Err(_) => None,
        };

        Self {
            config,
            app_state_ref: app_state,
            search_query: String::new(),
            tray_icon, // Created immediately
            _tray_menu: tray_menu,
            tray_settings_item,
            tray_quit_item,
            tray_favorite_bubble_item,
            last_ui_language: initial_ui_language,
            tray_retry_timer: 0.0,
            event_rx: rx,
            is_quitting: false,
            run_at_startup,
            auto_launcher: Some(auto),
            show_api_key: false,
            show_gemini_api_key: false,
            show_openrouter_api_key: false,
            show_cerebras_api_key: false,
            icon_dark: None,
            icon_light: None,
            view_mode,
            recording_hotkey_for_preset: None,
            hotkey_conflict_msg: None,
            recording_sr_hotkey: false,
            splash: None, // DELAYED CREATION to stage 35 for perfect $t=0$ timing
            fade_in_start: None,
            startup_stage: 0,
            cached_monitors,
            cached_audio_devices,
            snarl: None,
            last_edited_preset_idx: None,
            updater: Some(Updater::new(up_tx)),
            update_rx: up_rx,
            update_status: UpdateStatus::Idle,

            // --- NEW FIELD INIT ---
            current_admin_state,
            last_effective_theme_dark: effective_dark,
            last_system_theme_dark: system_dark,
            theme_check_timer: 0.0,
            // ----------------------

            // --- TIP INIT ---
            current_tip_idx: 0,
            tip_timer: 0.0,
            tip_fade_state: 0.0,
            tip_is_fading_in: true,
            show_tips_modal: false,
            rng_seed,
            // ---------------

            // --- USAGE MODAL INIT ---
            show_usage_modal: false,
            drop_overlay_fade: 0.0,
            // --- TTS SETTINGS MODAL INIT ---
            // --- TTS SETTINGS MODAL INIT ---
            show_tts_modal: false,
            // --- TOOLS MODAL INIT ---
            show_tools_modal: false,
            // -----------------------

            // --- FAVORITE BUBBLE STATE INIT ---
            last_bubble_enabled: initial_bubble_enabled,

            last_has_favorites: initial_has_favorites,
            // ----------------------------------

            // --- DOWNLOAD MANAGER INIT ---
            download_manager: crate::gui::settings_ui::download_manager::DownloadManager::new(),
            // -----------------------------

            // --- ARGUMENT HANDLING ---
            pending_file_path,
        }
    }
}
