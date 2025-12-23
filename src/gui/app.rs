

// Simple Linear Congruential Generator for randomness without external crate
fn simple_rand(seed: u32) -> u32 {
    seed.wrapping_mul(1103515245).wrapping_add(12345)
}

use eframe::egui;
use crate::config::{Config, save_config, Hotkey, ThemeMode};
use crate::{WINDOW_WIDTH, WINDOW_HEIGHT};
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
use crate::gui::key_mapping::{egui_key_to_vk, egui_pointer_to_vk};
use crate::updater::{Updater, UpdateStatus};
use crate::gui::settings_ui::{ViewMode, render_sidebar, render_global_settings, render_preset_editor, render_footer, render_history_panel};
use crate::gui::settings_ui::node_graph::{ChainNode, blocks_to_snarl, snarl_to_graph};
use egui_snarl::Snarl;
use crate::gui::utils::get_monitor_names;
use crate::icon_gen;



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
    show_openrouter_api_key: bool,
    
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
    last_effective_theme_dark: bool, // Effective dark mode (considering System/Dark/Light)
    last_system_theme_dark: bool, // Track Windows system theme for icon switching
    theme_check_timer: f64, // Timer for polling system theme
    // ------------------
    
    // --- TIP UI STATE ---
    current_tip_idx: usize,
    tip_timer: f64,        // Time when the current tip started showing
    tip_fade_state: f32,   // 0.0 (Invisible) -> 1.0 (Visible)
    tip_is_fading_in: bool,
    show_tips_modal: bool,
    rng_seed: u32,
    
    // --- NODE GRAPH STATE ---
    snarl: Option<Snarl<ChainNode>>,
    last_edited_preset_idx: Option<usize>,
    // ------------------------
    
    // --- USAGE MODAL STATE ---
    show_usage_modal: bool,
    // --- TTS SETTINGS MODAL STATE ---
    show_tts_modal: bool,
    // --------------------
}


impl SettingsApp {
    pub fn new(mut config: Config, app_state: Arc<Mutex<crate::AppState>>, tray_menu: Menu, ctx: egui::Context) -> Self {
        let app_name = "ScreenGoatedToolbox";
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
                    match OpenEventW(EVENT_ALL_ACCESS, false, w!("Global\\ScreenGoatedToolboxRestoreEvent")) {
                        Ok(event_handle) => {
                            let result = WaitForSingleObject(event_handle, INFINITE);
                            if result == WAIT_OBJECT_0 {
                                let class_name = w!("eframe");
                                let mut hwnd = FindWindowW(PCWSTR(class_name.as_ptr()), None);
                                if hwnd.0 == 0 {
                                    let title = w!("Screen Goated Toolbox (SGT by nganlinh4)");
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
                                let title = w!("Screen Goated Toolbox (SGT by nganlinh4)");
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

        // Detect initial system theme
        let system_dark = crate::gui::utils::is_system_in_dark_mode();
        
        // Determine effective initial theme
        let effective_dark = match config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_dark,
        };

        let start_in_tray = config.start_in_tray;
        let rng_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u32;

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
            show_openrouter_api_key: false,
            view_mode,
            recording_hotkey_for_preset: None,
            hotkey_conflict_msg: None,
            splash: if start_in_tray { None } else { Some(crate::gui::splash::SplashScreen::new(&ctx)) },
            fade_in_start: None,
            startup_stage: 0,
            cached_monitors,
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
            // --- TTS SETTINGS MODAL INIT ---
            show_tts_modal: false,
            // -----------------------
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

        // --- THEME MONITORING ---
        let now = ctx.input(|i| i.time);
        
        // 1. Check if we need to poll system theme (only if in System mode)
        let mut current_system_dark = self.last_system_theme_dark;
        
        if now - self.theme_check_timer > 1.0 { 
            self.theme_check_timer = now;
            // Always update system state tracker, even if not currently used
            current_system_dark = crate::gui::utils::is_system_in_dark_mode();
            self.last_system_theme_dark = current_system_dark;
        }

        // 2. Calculate Effective Theme
        let effective_dark = match self.config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => current_system_dark,
        };

        // 3. Apply Changes if Effective Theme Changed
        if effective_dark != self.last_effective_theme_dark {
            self.last_effective_theme_dark = effective_dark;
            
            // A. Update Visuals (egui)
            if effective_dark {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }

            // B. Update Native Icons (Tray & Window) based on Effective Theme
            if let Some(tray) = &mut self.tray_icon {
                let new_icon = icon_gen::get_tray_icon(effective_dark);
                let _ = tray.set_icon(Some(new_icon));
            }
            crate::gui::utils::update_window_icon_native(effective_dark);
        }

        // --- LAZY TRAY ICON CREATION ---
        // Try to create the tray icon if it doesn't exist yet.
        // This waits for the Windows Shell to fully initialize, avoiding crashes and duplicates.
        if self.tray_icon.is_none() {
            // FALLBACK: If icon is missing after 30s, ensure window is visible
            if now > 30.0 {
                 ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }

            if now - self.tray_retry_timer > 1.0 { 
                self.tray_retry_timer = now;
                
                // Use the Helper with effective theme
                let icon = icon_gen::get_tray_icon(self.last_effective_theme_dark);
                
                if let Ok(tray) = TrayIconBuilder::new()
                    .with_menu(Box::new(self.tray_menu.clone()))
                    .with_tooltip("Screen Goated Toolbox (nganlinh4)")
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
                let win_w_physical = WINDOW_WIDTH * pixels_per_point;
                let win_h_physical = WINDOW_HEIGHT * pixels_per_point;
                
                let center_x_physical = work_left + (work_w - win_w_physical) / 2.0;
                let center_y_physical = work_top + (work_h - win_h_physical) / 2.0;
                
                let x_logical = center_x_physical / pixels_per_point;
                let y_logical = center_y_physical / pixels_per_point;
                
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x_logical, y_logical)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT)));
                
                self.startup_stage = 1;
                ctx.request_repaint();
                return;
            }
        } else if self.startup_stage == 1 {
            self.startup_stage = 2;
            ctx.request_repaint(); 
        } else if self.startup_stage == 2 {
            if let Some(splash) = &mut self.splash { splash.reset_timer(ctx); }
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT)));
            
            // CRITICAL FIX: Only allow hiding if tray icon EXISTS.
            // Otherwise, stay visible so the update loop continues and creates the icon.
            let should_be_visible = !self.config.start_in_tray || self.tray_icon.is_none();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(should_be_visible));
            
            self.startup_stage = 3;
        }

        // Splash Update
        // Splash Update
        if let Some(splash) = &mut self.splash {
            match splash.update(ctx) {
                crate::gui::splash::SplashStatus::Ongoing => { 
                    // Do NOT return here. Continue to render main UI underneath.
                }
                crate::gui::splash::SplashStatus::Finished => {
                    self.splash = None;
                    // self.fade_in_start = Some(ctx.input(|i| i.time)); // Disable separate fade-in, as splash dissolves naturally
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

                    // Check Keyboard Events
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

                    // Check Mouse Events (Middle, Extra1, Extra2)
                    if key_recorded.is_none() {
                        let mouse_buttons = [
                            egui::PointerButton::Middle, 
                            egui::PointerButton::Extra1, 
                            egui::PointerButton::Extra2
                        ];
                        
                        for btn in mouse_buttons {
                            if i.pointer.button_pressed(btn) {
                                if let Some(vk) = egui_pointer_to_vk(&btn) {
                                    let name = match btn {
                                        egui::PointerButton::Middle => "Middle Click",
                                        egui::PointerButton::Extra1 => "Mouse Back",
                                        egui::PointerButton::Extra2 => "Mouse Forward",
                                        _ => "Mouse",
                                    }.to_string();
                                    key_recorded = Some((vk, modifiers_bitmap, name));
                                    break;
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

        let text = LocaleText::get(&self.config.ui_language);

        // [TIP ANIMATION LOGIC]
        let now = ctx.input(|i| i.time);
        
        // Initialize timer on first run
        if self.tip_timer == 0.0 {
            self.tip_timer = now;
        }

        // Calculate duration based on text length (reading speed ~ 15 chars/sec + 2s base)
        let current_tip = text.tips_list.get(self.current_tip_idx).unwrap_or(&"").to_string();
        let display_duration = (2.0 + (current_tip.len() as f64 * 0.06)) as f32;
        let fade_duration = 0.5f32;

        let elapsed = (now - self.tip_timer) as f32;
        
        if self.tip_is_fading_in {
            // Fading In
            self.tip_fade_state = (elapsed / fade_duration as f32).min(1.0);
            if elapsed >= fade_duration {
                self.tip_fade_state = 1.0;
                // Fully visible, wait for duration
                if elapsed >= fade_duration + display_duration {
                    self.tip_is_fading_in = false; // Start fading out
                    self.tip_timer = now; // Reset timer for fade-out
                }
            }
            ctx.request_repaint();
        } else {
            // Fading Out
            self.tip_fade_state = (1.0 - (elapsed / fade_duration as f32)).max(0.0);
            if elapsed >= fade_duration {
                self.tip_fade_state = 0.0;
                
                // Switch to next random tip
                self.rng_seed = simple_rand(self.rng_seed);
                if !text.tips_list.is_empty() {
                    let next = (self.rng_seed as usize) % text.tips_list.len();
                    // Avoid repeating same tip if possible
                    if next == self.current_tip_idx && text.tips_list.len() > 1 {
                        self.current_tip_idx = (next + 1) % text.tips_list.len();
                    } else {
                        self.current_tip_idx = next;
                    }
                }
                
                self.tip_timer = now; // Reset timer
                self.tip_is_fading_in = true; // Start fading in
            }
            ctx.request_repaint();
        }

        // Fade In Overlay
        if let Some(start_time) = self.fade_in_start {
            let elapsed = ctx.input(|i| i.time) - start_time;
            if elapsed < 0.6 {
                let opacity = 1.0 - (elapsed / 0.6) as f32;
                let rect = ctx.input(|i| i.viewport().inner_rect.unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO)));
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
            .frame(egui::Frame::default().inner_margin(egui::Margin::symmetric(10, 4)).fill(footer_bg))
            .show(ctx, |ui| {
                render_footer(
                    ui, 
                    &text, 
                    current_tip.clone(), 
                    self.tip_fade_state, 
                    &mut self.show_tips_modal
                );
            });

        // [TIPS POPUP - Uses popup system so scroll is not captured by node graph]
        let tips_popup_id = egui::Id::new("tips_popup_modal");
        
        if self.show_tips_modal {
            // Register this as an open popup so any_popup_open() returns true
            egui::Popup::open_id(ctx, tips_popup_id);
            
            let tips_list_copy = text.tips_list.clone();
            let tips_title = text.tips_title;
            let screen_rect = ctx.input(|i| i.viewport().inner_rect.unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO)));
            
            // Dark semi-transparent backdrop
            let backdrop_layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("tips_backdrop"));
            let backdrop_painter = ctx.layer_painter(backdrop_layer);
            backdrop_painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(120));
            
            // Popup area centered on screen
            egui::Area::new(tips_popup_id)
                .order(egui::Order::Tooltip) // High priority layer
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_max_width(750.0);
                            
                            // Header with title and close button
                            ui.horizontal(|ui| {
                                ui.heading(tips_title);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if crate::gui::icons::icon_button(ui, crate::gui::icons::Icon::Close).clicked() {
                                        self.show_tips_modal = false;
                                    }
                                });
                            });
                            ui.separator();
                            ui.add_space(8.0);
                            
                            // Scrollable tips list
                            egui::ScrollArea::vertical()
                                .max_height(450.0)
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    for (i, tip) in tips_list_copy.iter().enumerate() {
                                        ui.label(egui::RichText::new(*tip).size(13.0).line_height(Some(18.0)));
                                        if i < tips_list_copy.len() - 1 {
                                            ui.add_space(8.0);
                                            ui.separator();
                                            ui.add_space(8.0);
                                        }
                                    }
                                });
                        });
                });
            
            // Close on click outside (check if clicked outside the popup area)
            if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    // Check if click is on the backdrop (outside popup content)
                    if let Some(layer) = ctx.layer_id_at(pos) {
                        if layer.id == egui::Id::new("tips_backdrop") {
                            self.show_tips_modal = false;
                        }
                    }
                }
            }
            
            // Close on Escape
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.show_tips_modal = false;
            }
        } else {
            // Ensure popup is closed in memory when not shown
            // No need to explicitly close, the popup system handles this automatically
        }

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
                                &mut self.show_openrouter_api_key,
                                &usage_stats, 
                                &self.updater, 
                                &self.update_status, 
                                &mut self.run_at_startup, 
                                &self.auto_launcher, 
                                self.current_admin_state, // <-- Pass current admin state
                                &text,
                                &mut self.show_usage_modal,
                                &mut self.show_tts_modal,
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
                             // Sync snarl state if switching presets or first load
                             if self.last_edited_preset_idx != Some(idx) {
                                if idx < self.config.presets.len() {
                                     self.snarl = Some(blocks_to_snarl(
                                         &self.config.presets[idx].blocks,
                                         &self.config.presets[idx].block_connections
                                     ));
                                     self.last_edited_preset_idx = Some(idx);
                                }
                             }

                             if let Some(snarl) = &mut self.snarl {
                                  if render_preset_editor(
                                      ui, 
                                      &mut self.config, 
                                      idx, 
                                      &mut self.search_query, 
                                      &mut self.cached_monitors, 
                                      &mut self.recording_hotkey_for_preset, 
                                      &self.hotkey_conflict_msg, 
                                      &text,
                                      snarl
                                  ) {
                                      // Sync back to blocks and connections
                                      if idx < self.config.presets.len() {
                                          let (blocks, connections) = snarl_to_graph(snarl);
                                          self.config.presets[idx].blocks = blocks;
                                          self.config.presets[idx].block_connections = connections;
                                      }
                                      self.save_and_sync();
                                  }
                             }
                        }
                    }
                });
            });
        });

        // --- RENDER SPLASH OVERLAY ---
        // Rendered LAST to be on top of everything, while Main UI rendered underneath.
        if let Some(splash) = &self.splash {
            splash.paint(ctx);
        }
    }
    
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray_icon = None;
    }
}
