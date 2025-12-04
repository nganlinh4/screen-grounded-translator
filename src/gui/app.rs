#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use crate::config::{Config, save_config, Hotkey};
use std::sync::{Arc, Mutex};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent, MouseButton, menu::{Menu, MenuEvent}};
use auto_launch::AutoLaunch;
use std::sync::mpsc::{Receiver, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::Threading::*;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, POINT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST, GetMonitorInfoW};
use windows::core::*;

use crate::gui::locale::LocaleText;
use crate::gui::key_mapping::egui_key_to_vk;
use crate::updater::{Updater, UpdateStatus};
use crate::gui::settings_ui::{ViewMode, render_sidebar, render_global_settings, render_preset_editor, render_footer, render_history_panel};
use crate::gui::utils::get_monitor_names;



lazy_static::lazy_static! {
    static ref RESTORE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

pub struct SettingsApp {
    config: Config,
    app_state_ref: Arc<Mutex<crate::AppState>>,
    search_query: String, 
    tray_icon: Option<TrayIcon>,
    _tray_menu: Menu,
    tray_menu: Menu, // Store menu for lazy icon creation
    tray_retry_timer: f64, // Timer for lazy tray icon creation
    event_rx: Receiver<UserEvent>,
    is_quitting: bool,
    run_at_startup: bool,
    auto_launcher: Option<AutoLaunch>,
    show_api_key: bool,
    show_gemini_api_key: bool,
    
    view_mode: ViewMode,
    recording_hotkey_for_preset: Option<usize>,
    hotkey_conflict_msg: Option<String>,
    splash: Option<crate::gui::splash::SplashScreen>,
    fade_in_start: Option<f64>,
    
    // 0 = Init/Offscreen, 1 = Move Sent, 2 = Visible Sent
    startup_stage: u8, 
    
    cached_monitors: Vec<String>,
    
    updater: Option<Updater>,
    update_rx: Receiver<UpdateStatus>,
    update_status: UpdateStatus,
    
    // --- NEW FIELDS ---
    current_admin_state: bool, // Track runtime admin status
    // ------------------
}

impl SettingsApp {
    pub fn new(mut config: Config, app_state: Arc<Mutex<crate::AppState>>, tray_menu: Menu, ctx: egui::Context) -> Self {
        let app_name = "ScreenGroundedTranslator";
        let app_path = std::env::current_exe().unwrap();
        let args: &[&str] = &[];
        
        let auto = AutoLaunch::new(app_name, app_path.to_str().unwrap(), args);
        
        // 1. Check Registry for standard startup
        let mut run_at_startup = false;
        #[cfg(target_os = "windows")]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok(key) = hkcu.open_subkey_with_flags("Software\\Microsoft\\Windows\\CurrentVersion\\Run", KEY_READ) {
                if key.get_value::<String, &str>(app_name).is_ok() {
                    run_at_startup = true;
                }
            }
        }
        if !run_at_startup {
            run_at_startup = auto.is_enabled().unwrap_or(false);
        }

        // 2. Check Task Scheduler for Admin startup (FIX for persistence)
        // If the Task exists, we consider startup enabled AND admin mode enabled.
        if crate::gui::utils::is_admin_startup_enabled() {
            run_at_startup = true;
            config.run_as_admin_on_startup = true;
            // Don't enable registry when Task Scheduler is active
        } else if config.run_as_admin_on_startup {
            // Config thinks admin is on, but Task is missing? 
            // Trust the system state -> Task is missing, so it's off.
            config.run_as_admin_on_startup = false;
        }

        if run_at_startup && !config.run_as_admin_on_startup {
            // Ensure path is current in case exe moved
            let _ = auto.enable();
        }

        let (tx, rx) = channel();

        // Tray thread
        let tx_tray = tx.clone();
        let ctx_tray = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = TrayIconEvent::receiver().recv() {
                let _ = tx_tray.send(UserEvent::Tray(event));
                ctx_tray.request_repaint();
            }
        });

        // Restore signal listener
        let ctx_restore = ctx.clone();
        std::thread::spawn(move || {
            loop {
                unsafe {
                    match OpenEventW(EVENT_ALL_ACCESS, false, w!("Global\\ScreenGroundedTranslatorRestoreEvent")) {
                        Ok(event_handle) => {
                            let result = WaitForSingleObject(event_handle, INFINITE);
                            if result == WAIT_OBJECT_0 {
                                let class_name = w!("eframe");
                                let mut hwnd = FindWindowW(PCWSTR(class_name.as_ptr()), None);
                                if hwnd.0 == 0 {
                                    let title = w!("Screen Grounded Translator (SGT by nganlinh4)");
                                    hwnd = FindWindowW(None, PCWSTR(title.as_ptr()));
                                }
                                if hwnd.0 != 0 {
                                    ShowWindow(hwnd, SW_RESTORE);
                                    ShowWindow(hwnd, SW_SHOW);
                                    SetForegroundWindow(hwnd);
                                    SetFocus(hwnd);
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
                            let hwnd = FindWindowW(PCWSTR(class_name.as_ptr()), None);
                            let hwnd = if hwnd.0 == 0 {
                                let title = w!("Screen Grounded Translator (SGT by nganlinh4)");
                                FindWindowW(None, PCWSTR(title.as_ptr()))
                            } else { hwnd };
                            if hwnd.0 != 0 {
                                ShowWindow(hwnd, SW_RESTORE);
                                ShowWindow(hwnd, SW_SHOW);
                                SetForegroundWindow(hwnd);
                                SetFocus(hwnd);
                            }
                        }
                        RESTORE_SIGNAL.store(true, Ordering::SeqCst);
                        let _ = tx_menu.send(UserEvent::Menu(event.clone()));
                        ctx_menu.request_repaint();
                    }
                    _ => { let _ = tx_menu.send(UserEvent::Menu(event)); ctx_menu.request_repaint(); }
                }
            }
        });

        let view_mode = if config.presets.is_empty() {
             ViewMode::Global 
        } else {
             ViewMode::Preset(if config.active_preset_idx < config.presets.len() { config.active_preset_idx } else { 0 })
        };
        
        let cached_monitors = get_monitor_names();
        let (up_tx, up_rx) = channel();
        
        // Check for current admin state
        let current_admin_state = if cfg!(target_os = "windows") {
            crate::gui::utils::is_running_as_admin()
        } else { false };

        let start_in_tray = config.start_in_tray;

        Self {
            config,
            app_state_ref: app_state,
            search_query: String::new(),
            tray_icon: None, // INITIALIZE AS NONE - will be created lazily in update()
            _tray_menu: tray_menu.clone(),
            tray_menu, // Store for lazy initialization
            tray_retry_timer: -5.0, // Negative to force immediate retry if needed
            event_rx: rx,
            is_quitting: false,
            run_at_startup,
            auto_launcher: Some(auto),
            show_api_key: false,
            show_gemini_api_key: false,
            view_mode,
            recording_hotkey_for_preset: None,
            hotkey_conflict_msg: None,
            splash: if start_in_tray { None } else { Some(crate::gui::splash::SplashScreen::new(&ctx)) },
            fade_in_start: None,
            startup_stage: 0,
            cached_monitors,
            updater: Some(Updater::new(up_tx)),
            update_rx: up_rx,
            update_status: UpdateStatus::Idle,
            
            // --- NEW FIELD INIT ---
            current_admin_state,
            // ----------------------
        }
    }

    fn save_and_sync(&mut self) {
        if let ViewMode::Preset(idx) = self.view_mode {
            self.config.active_preset_idx = idx;
        }

        let mut state = self.app_state_ref.lock().unwrap();
        state.hotkeys_updated = true;
        state.config = self.config.clone();
        drop(state);
        save_config(&self.config);
        
        unsafe {
            let class = w!("HotkeyListenerClass");
            let title = w!("Listener");
            let hwnd = windows::Win32::UI::WindowsAndMessaging::FindWindowW(class, title);
            if hwnd.0 != 0 {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, 0x0400 + 101, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
            }
        }
    }
    
    fn restore_window(&self, ctx: &egui::Context) {
         ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
         ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
         ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
         ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
         ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
         ctx.request_repaint();
     }

    fn check_hotkey_conflict(&self, vk: u32, mods: u32, current_preset_idx: usize) -> Option<String> {
        for (idx, preset) in self.config.presets.iter().enumerate() {
            if idx == current_preset_idx { continue; }
            for hk in &preset.hotkeys {
                if hk.code == vk && hk.modifiers == mods {
                    return Some(format!("Conflict with '{}' in preset '{}'", hk.name, preset.name));
                }
            }
        }
        None
    }
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] { [0.0, 0.0, 0.0, 0.0] }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Updater
        while let Ok(status) = self.update_rx.try_recv() { self.update_status = status; }

        // --- LAZY TRAY ICON CREATION ---
        // Try to create the tray icon if it doesn't exist yet.
        // This waits for the Windows Shell to fully initialize, avoiding crashes and duplicates.
        if self.tray_icon.is_none() {
            let now = ctx.input(|i| i.time);
            
            // FALLBACK: If icon is missing after 30s, ensure window is visible
            if now > 30.0 {
                 ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }

            if now - self.tray_retry_timer > 1.0 { 
                self.tray_retry_timer = now;
                let icon = crate::icon_gen::generate_icon();
                if let Ok(tray) = TrayIconBuilder::new()
                    .with_menu(Box::new(self.tray_menu.clone()))
                    .with_tooltip("Screen Grounded Translator (nganlinh4)")
                    .with_icon(icon)
                    .build() 
                {
                    self.tray_icon = Some(tray);
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }

        // --- 3-Stage Startup Logic ---
        if self.startup_stage == 0 {
            unsafe {
                let mut cursor_pos = POINT::default();
                GetCursorPos(&mut cursor_pos);
                let h_monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO::default();
                mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                GetMonitorInfoW(h_monitor, &mut mi);
                
                let work_w = (mi.rcWork.right - mi.rcWork.left) as f32;
                let work_h = (mi.rcWork.bottom - mi.rcWork.top) as f32;
                let work_left = mi.rcWork.left as f32;
                let work_top = mi.rcWork.top as f32;
                
                let pixels_per_point = ctx.pixels_per_point();
                let win_w_physical = 635.0 * pixels_per_point;
                let win_h_physical = 500.0 * pixels_per_point;
                
                let center_x_physical = work_left + (work_w - win_w_physical) / 2.0;
                let center_y_physical = work_top + (work_h - win_h_physical) / 2.0;
                
                let x_logical = center_x_physical / pixels_per_point;
                let y_logical = center_y_physical / pixels_per_point;
                
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x_logical, y_logical)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(635.0, 500.0)));
                
                self.startup_stage = 1;
                ctx.request_repaint();
                return;
            }
        } else if self.startup_stage == 1 {
            self.startup_stage = 2;
            ctx.request_repaint(); 
        } else if self.startup_stage == 2 {
            if let Some(splash) = &mut self.splash { splash.reset_timer(ctx); }
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(635.0, 500.0)));
            
            // CRITICAL FIX: Only allow hiding if tray icon EXISTS.
            // Otherwise, stay visible so the update loop continues and creates the icon.
            let should_be_visible = !self.config.start_in_tray || self.tray_icon.is_none();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(should_be_visible));
            
            self.startup_stage = 3;
        }

        // Splash Update
        if let Some(splash) = &mut self.splash {
            match splash.update(ctx) {
                crate::gui::splash::SplashStatus::Ongoing => { return; }
                crate::gui::splash::SplashStatus::Finished => {
                    self.splash = None;
                    self.fade_in_start = Some(ctx.input(|i| i.time));
                }
            }
        }

        if RESTORE_SIGNAL.swap(false, Ordering::SeqCst) { self.restore_window(ctx); }

        // --- Hotkey Recording Logic ---
        if let Some(preset_idx) = self.recording_hotkey_for_preset {
            let mut key_recorded: Option<(u32, u32, String)> = None;
            let mut cancel = false;

            ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    cancel = true;
                } else {
                    let mut modifiers_bitmap = 0;
                    if i.modifiers.ctrl { modifiers_bitmap |= MOD_CONTROL; }
                    if i.modifiers.alt { modifiers_bitmap |= MOD_ALT; }
                    if i.modifiers.shift { modifiers_bitmap |= MOD_SHIFT; }
                    if i.modifiers.command { modifiers_bitmap |= MOD_WIN; }

                    for event in &i.events {
                        if let egui::Event::Key { key, pressed: true, .. } = event {
                            if let Some(vk) = egui_key_to_vk(key) {
                                if !matches!(vk, 16 | 17 | 18 | 91 | 92) {
                                    let key_name = format!("{:?}", key).trim_start_matches("Key").to_string();
                                    key_recorded = Some((vk, modifiers_bitmap, key_name));
                                }
                            }
                        }
                    }
                }
            });

            if cancel {
                self.recording_hotkey_for_preset = None;
                self.hotkey_conflict_msg = None;
            } else if let Some((vk, mods, key_name)) = key_recorded {
                if let Some(msg) = self.check_hotkey_conflict(vk, mods, preset_idx) {
                    self.hotkey_conflict_msg = Some(msg);
                } else {
                    let mut name_parts = Vec::new();
                    if (mods & MOD_CONTROL) != 0 { name_parts.push("Ctrl".to_string()); }
                    if (mods & MOD_ALT) != 0 { name_parts.push("Alt".to_string()); }
                    if (mods & MOD_SHIFT) != 0 { name_parts.push("Shift".to_string()); }
                    if (mods & MOD_WIN) != 0 { name_parts.push("Win".to_string()); }
                    name_parts.push(key_name);

                    let new_hotkey = Hotkey {
                        code: vk,
                        modifiers: mods,
                        name: name_parts.join(" + "),
                    };

                    if let Some(preset) = self.config.presets.get_mut(preset_idx) {
                        if !preset.hotkeys.iter().any(|h| h.code == vk && h.modifiers == mods) {
                            preset.hotkeys.push(new_hotkey);
                            self.save_and_sync();
                        }
                    }
                    self.recording_hotkey_for_preset = None;
                    self.hotkey_conflict_msg = None;
                }
            }
        }

        // --- Event Handling ---
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                UserEvent::Tray(tray_event) => {
                    if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = tray_event {
                        self.restore_window(ctx);
                    }
                }
                UserEvent::Menu(menu_event) => {
                    if menu_event.id.0 == "1002" {
                        self.restore_window(ctx);
                    }
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.is_quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        if self.config.dark_mode { ctx.set_visuals(egui::Visuals::dark()); } else { ctx.set_visuals(egui::Visuals::light()); }

        let text = LocaleText::get(&self.config.ui_language);

        // Fade In Overlay
        if let Some(start_time) = self.fade_in_start {
            let elapsed = ctx.input(|i| i.time) - start_time;
            if elapsed < 0.6 {
                let opacity = 1.0 - (elapsed / 0.6) as f32;
                let rect = ctx.input(|i| i.screen_rect());
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("fade_overlay")));
                painter.rect_filled(rect, 0.0, eframe::egui::Color32::from_black_alpha((opacity * 255.0) as u8));
                ctx.request_repaint();
            } else {
                self.fade_in_start = None;
            }
        }

        // --- UI LAYOUT ---
        let visuals = ctx.style().visuals.clone();
        let footer_bg = if visuals.dark_mode { egui::Color32::from_gray(20) } else { egui::Color32::from_gray(240) };
        
        egui::TopBottomPanel::bottom("footer_panel")
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(10.0, 4.0)).fill(footer_bg))
            .show(ctx, |ui| render_footer(ui, &text));

        egui::CentralPanel::default().show(ctx, |ui| {
            let available_width = ui.available_width();
            let left_width = available_width * 0.35;
            let right_width = available_width * 0.65;

            ui.horizontal(|ui| {
                // Left Sidebar
                ui.allocate_ui_with_layout(egui::vec2(left_width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                    if render_sidebar(ui, &mut self.config, &mut self.view_mode, &text) {
                        self.save_and_sync();
                    }
                });

                ui.add_space(10.0);

                // Right Detail View
                ui.allocate_ui_with_layout(egui::vec2(right_width - 20.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                    match self.view_mode {
                        ViewMode::Global => {
                            let usage_stats = {
                                let app = self.app_state_ref.lock().unwrap();
                                app.model_usage_stats.clone()
                            };
                            if render_global_settings(
                                ui, 
                                &mut self.config, 
                                &mut self.show_api_key, 
                                &mut self.show_gemini_api_key, 
                                &usage_stats, 
                                &self.updater, 
                                &self.update_status, 
                                &mut self.run_at_startup, 
                                &self.auto_launcher, 
                                self.current_admin_state, // <-- Pass current admin state
                                &text
                            ) {
                                self.save_and_sync();
                            }
                        },
                        ViewMode::History => {
                             let history_manager = {
                                 let app = self.app_state_ref.lock().unwrap();
                                 app.history.clone()
                             };
                             if render_history_panel(
                                 ui,
                                 &mut self.config,
                                 &history_manager,
                                 &mut self.search_query,
                                 &text
                             ) {
                                 self.save_and_sync();
                             }
                        },
                        ViewMode::Preset(idx) => {
                             if render_preset_editor(
                                 ui, 
                                 &mut self.config, 
                                 idx, 
                                 &mut self.search_query, 
                                 &mut self.cached_monitors, 
                                 &mut self.recording_hotkey_for_preset, 
                                 &self.hotkey_conflict_msg, 
                                 &text
                             ) {
                                 self.save_and_sync();
                             }
                        }
                    }
                });
            });
        });
    }
    
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray_icon = None;
    }
}
