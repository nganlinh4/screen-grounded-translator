use eframe::egui;
use crate::config::{Config, save_config, get_all_languages, Preset, Hotkey};
use std::sync::{Arc, Mutex};
use tray_icon::{TrayIcon, TrayIconEvent, MouseButton, menu::{Menu, MenuEvent}};
use auto_launch::AutoLaunch;
use std::sync::mpsc::{Receiver, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::Threading::*;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR, GetMonitorInfoW, MONITORINFOEXW};
use windows::core::*;

use crate::gui::locale::LocaleText;
use crate::gui::key_mapping::egui_key_to_vk;
use crate::model_config::{get_all_models, ModelType, get_model_by_id};

// --- Monitor Enumeration Helper ---
struct MonitorEnumContext {
    monitors: Vec<String>,
}

unsafe extern "system" fn monitor_enum_proc(_hmonitor: HMONITOR, _hdc: HDC, _lprc: *mut RECT, dwdata: LPARAM) -> BOOL {
    let context = &mut *(dwdata.0 as *mut MonitorEnumContext);
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    
    if GetMonitorInfoW(_hmonitor, &mut mi as *mut _ as *mut _).as_bool() {
        let device_name = String::from_utf16_lossy(&mi.szDevice);
        let trimmed_name = device_name.trim_matches(char::from(0)).to_string();
        context.monitors.push(trimmed_name);
    }
    BOOL(1)
}

fn get_monitor_names() -> Vec<String> {
    let mut ctx = MonitorEnumContext { monitors: Vec::new() };
    unsafe {
        EnumDisplayMonitors(HDC(0), None, Some(monitor_enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.monitors
}
// ----------------------------------

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

lazy_static::lazy_static! {
    static ref RESTORE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

#[derive(PartialEq, Clone, Copy)]
enum ViewMode {
    Global,
    Preset(usize),
}

pub struct SettingsApp {
    config: Config,
    app_state_ref: Arc<Mutex<crate::AppState>>,
    search_query: String, // Shared search for languages
    tray_icon: Option<TrayIcon>,
    _tray_menu: Menu,
    event_rx: Receiver<UserEvent>,
    is_quitting: bool,
    run_at_startup: bool,
    auto_launcher: Option<AutoLaunch>,
    show_api_key: bool,
    show_gemini_api_key: bool,
    
    // New State
    view_mode: ViewMode,
    recording_hotkey_for_preset: Option<usize>,
    hotkey_conflict_msg: Option<String>,
    splash: Option<crate::gui::splash::SplashScreen>,
    fade_in_start: Option<f64>,
    startup_centered: bool,
    
    // Cache monitors
    cached_monitors: Vec<String>,
}

impl SettingsApp {
    pub fn new(config: Config, app_state: Arc<Mutex<crate::AppState>>, tray_icon: TrayIcon, tray_menu: Menu, ctx: egui::Context) -> Self {
        let app_name = "ScreenGroundedTranslator";
        let app_path = std::env::current_exe().unwrap();
        let args: &[&str] = &[];
        
        let auto = AutoLaunch::new(app_name, app_path.to_str().unwrap(), args);
        let run_at_startup = auto.is_enabled().unwrap_or(false);
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
                    match OpenEventW(EVENT_ALL_ACCESS, false, w!("ScreenGroundedTranslatorRestoreEvent")) {
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
                        // Try to find and restore window directly
                        unsafe {
                            let class_name = w!("eframe");
                            let hwnd = FindWindowW(PCWSTR(class_name.as_ptr()), None);
                            let hwnd = if hwnd.0 == 0 {
                                let title = w!("Screen Grounded Translator (SGT by nganlinh4)");
                                FindWindowW(None, PCWSTR(title.as_ptr()))
                            } else {
                                hwnd
                            };
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

        // Determine initial view mode
        let view_mode = if config.presets.is_empty() {
             ViewMode::Global 
        } else {
             ViewMode::Preset(if config.active_preset_idx < config.presets.len() { config.active_preset_idx } else { 0 })
        };
        
        let cached_monitors = get_monitor_names();

        Self {
            config,
            app_state_ref: app_state,
            search_query: String::new(),
            tray_icon: Some(tray_icon),
            _tray_menu: tray_menu,
            event_rx: rx,
            is_quitting: false,
            run_at_startup,
            auto_launcher: Some(auto),
            show_api_key: false,
            show_gemini_api_key: false,
            view_mode,
            recording_hotkey_for_preset: None,
            hotkey_conflict_msg: None,
            splash: Some(crate::gui::splash::SplashScreen::new(&ctx)),
            fade_in_start: None,
            startup_centered: false,
            cached_monitors,
        }
    }

    fn save_and_sync(&mut self) {
        // Update active preset index in config for persistence
        if let ViewMode::Preset(idx) = self.view_mode {
            self.config.active_preset_idx = idx;
        }

        let mut state = self.app_state_ref.lock().unwrap();
        
        // Check if hotkeys changed
        // Simplification: Always signal update on save. Overhead is low.
        state.hotkeys_updated = true;
        state.config = self.config.clone();
        
        drop(state);
        save_config(&self.config);
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
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Center window on startup
        if !self.startup_centered {
            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                let outer_rect = ctx.input(|i| i.viewport().outer_rect);
                let inner_rect = ctx.input(|i| i.viewport().inner_rect);
                
                let win_size = if let Some(rect) = outer_rect {
                    rect.size()
                } else if let Some(rect) = inner_rect {
                    rect.size()
                } else {
                    egui::vec2(600.0, 500.0)
                };

                let x = (monitor_size.x - win_size.x) / 2.0;
                let y = (monitor_size.y - win_size.y) / 2.0;
                
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                self.startup_centered = true;
            }
        }

        // Check Splash
        if let Some(splash) = &mut self.splash {
            // Hide decorations during splash
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            
            // Render Splash
            // We want the splash to cover everything, so we assume full window
            match splash.update(ctx) {
                crate::gui::splash::SplashStatus::Ongoing => {
                    return; // Don't draw the rest of the UI yet
                }
                crate::gui::splash::SplashStatus::Finished => {
                    self.splash = None; // Drop splash, proceed to normal UI
                    self.fade_in_start = Some(ctx.input(|i| i.time)); // Start fade in
                    // Restore decorations
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                }
            }
        }

        if RESTORE_SIGNAL.swap(false, Ordering::SeqCst) {
            self.restore_window(ctx);
        }

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
                // Conflict Check
                if let Some(msg) = self.check_hotkey_conflict(vk, mods, preset_idx) {
                    self.hotkey_conflict_msg = Some(msg);
                } else {
                    // No conflict
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

        if self.config.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        let text = LocaleText::get(&self.config.ui_language);

        // --- FADE IN OVERLAY (Dark Hyperspace Reveal) ---
        if let Some(start_time) = self.fade_in_start {
            let now = ctx.input(|i| i.time);
            let elapsed = now - start_time;
            let fade_duration = 0.6; // Faster fade, match warp duration
            
            if elapsed < fade_duration {
                let opacity = 1.0 - (elapsed / fade_duration) as f32;
                // Create a black overlay on top of everything
                let rect = ctx.input(|i| i.screen_rect());
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("fade_overlay")));
                
                // Black -> Transparent (Dark Hyperspace Jump effect)
                let color = eframe::egui::Color32::from_black_alpha((opacity * 255.0) as u8);
                painter.rect_filled(rect, 0.0, color);
                
                ctx.request_repaint();
            } else {
                self.fade_in_start = None;
            }
        }

        // --- UI LAYOUT ---
        egui::CentralPanel::default().show(ctx, |ui| {
            // Main Split (3.5 : 6.5 ratio)
            let available_width = ui.available_width();
            let left_width = available_width * 0.35;
            let right_width = available_width * 0.65; // Remaining width

            ui.horizontal(|ui| {
                // --- LEFT: SIDEBAR (Presets + Global) ---
                ui.allocate_ui_with_layout(egui::vec2(left_width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                    // Theme & Language Controls (Moved from Header)
                    ui.horizontal(|ui| {
                        let theme_icon = if self.config.dark_mode { "🌙" } else { "☀" };
                        if ui.button(theme_icon).on_hover_text("Toggle Theme").clicked() {
                            self.config.dark_mode = !self.config.dark_mode;
                            self.save_and_sync();
                        }
                        
                        let original_lang = self.config.ui_language.clone();
                        let lang_display = match self.config.ui_language.as_str() {
                            "vi" => "VI",
                            "ko" => "KO",
                            _ => "EN",
                        };
                        egui::ComboBox::from_id_source("header_lang_switch")
                            .width(60.0)
                            .selected_text(lang_display)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.ui_language, "en".to_string(), "English");
                                ui.selectable_value(&mut self.config.ui_language, "vi".to_string(), "Vietnamese");
                                ui.selectable_value(&mut self.config.ui_language, "ko".to_string(), "Korean");
                            });
                        if original_lang != self.config.ui_language {
                            self.save_and_sync();
                        }
                    });
                    ui.add_space(5.0);

                    // Global Settings Button
                    let is_global = matches!(self.view_mode, ViewMode::Global);
                    if ui.selectable_label(is_global, format!("⚙ {}", text.global_settings)).clicked() {
                        self.view_mode = ViewMode::Global;
                    }
                    
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(text.presets_section).strong());
                    
                    let mut preset_idx_to_delete = None;

                    // Removed ScrollArea wrapper as requested
                     for (idx, preset) in self.config.presets.iter().enumerate() {
                         ui.horizontal(|ui| {
                             let is_selected = matches!(self.view_mode, ViewMode::Preset(i) if i == idx);
                             
                             let icon = if preset.preset_type == "audio" { "🎤" } 
                             else if preset.preset_type == "video" { "🎥" } 
                             else { "🖼" };
                             
                             let label_text = format!("{} {}", icon, preset.name);
                            
                            if preset.is_upcoming {
                                ui.add_enabled_ui(false, |ui| {
                                    let _ = ui.selectable_label(is_selected, &label_text);
                                });
                            } else {
                                if ui.selectable_label(is_selected, &label_text).clicked() {
                                    self.view_mode = ViewMode::Preset(idx);
                                }
                                // Delete button (small x)
                                if self.config.presets.len() > 1 {
                                    if ui.small_button("x").clicked() {
                                        preset_idx_to_delete = Some(idx);
                                    }
                                }
                            }
                        });
                    }
                    
                    ui.add_space(5.0);
                    if ui.button(text.add_preset_btn).clicked() {
                        let mut new_preset = Preset::default();
                        new_preset.name = format!("Preset {}", self.config.presets.len() + 1);
                        self.config.presets.push(new_preset);
                        self.view_mode = ViewMode::Preset(self.config.presets.len() - 1);
                        self.save_and_sync();
                    }

                    if let Some(idx) = preset_idx_to_delete {
                        self.config.presets.remove(idx);
                        if let ViewMode::Preset(curr) = self.view_mode {
                            if curr >= idx && curr > 0 {
                                self.view_mode = ViewMode::Preset(curr - 1);
                            } else if self.config.presets.is_empty() {
                                self.view_mode = ViewMode::Global;
                            } else {
                                self.view_mode = ViewMode::Preset(0);
                            }
                        }
                        self.save_and_sync();
                    }
                });

                ui.add_space(10.0); // Spacing between columns

                // --- RIGHT: DETAIL VIEW ---
                ui.allocate_ui_with_layout(egui::vec2(right_width - 20.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                    match self.view_mode {
                        ViewMode::Global => {
                            // Removed Heading
                            ui.add_space(10.0);
                            
                            // API Keys
                            ui.group(|ui| {
                                ui.label(egui::RichText::new(text.api_section).strong());
                                ui.label(text.api_key_label);
                                ui.horizontal(|ui| {
                                    if ui.add(egui::TextEdit::singleline(&mut self.config.api_key).password(!self.show_api_key).desired_width(200.0)).changed() {
                                        self.save_and_sync();
                                    }
                                    if ui.button(if self.show_api_key { "👁" } else { "🔒" }).clicked() { self.show_api_key = !self.show_api_key; }
                                });
                                if ui.link(text.get_key_link).clicked() { let _ = open::that("https://console.groq.com/keys"); }
                                
                                ui.add_space(5.0);
                                ui.label(text.gemini_api_key_label);
                                ui.horizontal(|ui| {
                                    if ui.add(egui::TextEdit::singleline(&mut self.config.gemini_api_key).password(!self.show_gemini_api_key).desired_width(200.0)).changed() {
                                        self.save_and_sync();
                                    }
                                    if ui.button(if self.show_gemini_api_key { "👁" } else { "🔒" }).clicked() { self.show_gemini_api_key = !self.show_gemini_api_key; }
                                });
                                if ui.link(text.gemini_get_key_link).clicked() { let _ = open::that("https://aistudio.google.com/app/apikey"); }
                            });

                            ui.add_space(10.0);
                            
                            // --- NEW: USAGE STATISTICS ---
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(text.usage_statistics_title).strong());
                                    ui.label("ℹ").on_hover_text(text.usage_statistics_tooltip);
                                });
                                
                                let usage_stats = {
                                    let app = self.app_state_ref.lock().unwrap();
                                    app.model_usage_stats.clone()
                                };

                                egui::Grid::new("usage_grid").striped(true).show(ui, |ui| {
                                    ui.label(egui::RichText::new(text.usage_model_column).strong());
                                    ui.label(egui::RichText::new(text.usage_remaining_column).strong());
                                    ui.end_row();

                                    // Track shown models to avoid duplicates (by full_name)
                                    let mut shown_models = std::collections::HashSet::new();
                                    
                                    for model in get_all_models() {
                                        if !model.enabled { continue; }
                                        
                                        // Skip duplicates (same full_name)
                                        if shown_models.contains(&model.full_name) {
                                            continue;
                                        }
                                        shown_models.insert(model.full_name.clone());
                                        
                                        // Display model name without speed labels
                                        ui.label(model.full_name.clone());
                                        
                                        // 2. Real-time Status
                                        if model.provider == "groq" {
                                            // Look up by FULL NAME
                                            let status = usage_stats.get(&model.full_name).cloned().unwrap_or_else(|| {
                                                "??? / ?".to_string()
                                            });
                                            ui.label(status);
                                        } else if model.provider == "google" {
                                            // Link for Gemini
                                            ui.hyperlink_to(text.usage_check_link, "https://aistudio.google.com/usage?timeRange=last-1-day&tab=rate-limit");
                                        }
                                        ui.end_row();
                                    }
                                });
                            });
                            // -----------------------------

                            ui.add_space(10.0);

                            ui.horizontal(|ui| {
                                if let Some(launcher) = &self.auto_launcher {
                                    if ui.checkbox(&mut self.run_at_startup, text.startup_label).clicked() {
                                        if self.run_at_startup { let _ = launcher.enable(); } else { let _ = launcher.disable(); }
                                    }
                                }
                                if ui.button(text.reset_defaults_btn).clicked() {
                                    // Save API keys before resetting
                                    let saved_groq_key = self.config.api_key.clone();
                                    let saved_gemini_key = self.config.gemini_api_key.clone();
                                    
                                    // Reset to defaults
                                    self.config = Config::default();
                                    
                                    // Restore API keys
                                    self.config.api_key = saved_groq_key;
                                    self.config.gemini_api_key = saved_gemini_key;
                                    
                                    self.save_and_sync();
                                }
                            });
                        }
                        
                        ViewMode::Preset(idx) => {
                            // Ensure index is valid (could be invalid if just deleted)
                            if idx >= self.config.presets.len() {
                                self.view_mode = ViewMode::Global; 
                                return;
                            }

                            let mut preset = self.config.presets[idx].clone();
                            let mut preset_changed = false;
                            let _is_vietnamese = self.config.ui_language == "vi";

                            // Removed Heading
                            ui.add_space(5.0);

                            // 1. Name (Bigger)
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(text.preset_name_label).heading());
                                if ui.add(egui::TextEdit::singleline(&mut preset.name).font(egui::TextStyle::Heading)).changed() {
                                    preset_changed = true;
                                }
                            });
                            
                            // Type Dropdown
                             ui.horizontal(|ui| {
                                 ui.label(text.preset_type_label);
                                 let image_label = text.preset_type_image;
                                 let audio_label = text.preset_type_audio;
                                 let video_label = text.preset_type_video;
                                 
                                 let selected_text = match preset.preset_type.as_str() {
                                     "audio" => audio_label,
                                     "video" => video_label,
                                     _ => image_label,
                                 };
                                 
                                 egui::ComboBox::from_id_source("preset_type_combo")
                                     .selected_text(selected_text)
                                     .show_ui(ui, |ui| {
                                         if ui.selectable_value(&mut preset.preset_type, "image".to_string(), image_label).clicked() {
                                             preset.model = "scout".to_string(); 
                                             preset_changed = true;
                                         }
                                         if ui.selectable_value(&mut preset.preset_type, "audio".to_string(), audio_label).clicked() {
                                             preset.model = "whisper-fast".to_string(); 
                                             preset_changed = true;
                                         }
                                         // Grayed out Video Option
                                         ui.add_enabled_ui(false, |ui| {
                                             let _ = ui.selectable_value(&mut preset.preset_type, "video".to_string(), video_label);
                                         });
                                     });
                             });

                             let is_audio = preset.preset_type == "audio";
                             let is_video = preset.preset_type == "video";

                             // --- VIDEO PLACEHOLDER UI ---
                             if is_video {
                                 ui.group(|ui| {
                                     ui.label(egui::RichText::new(text.capture_method_label).strong());
                                     
                                     // FIX 2: Monitor List Refresh Button
                                     ui.horizontal(|ui| {
                                         if ui.button("↻").on_hover_text("Refresh Monitors").clicked() {
                                             self.cached_monitors = get_monitor_names();
                                         }

                                         egui::ComboBox::from_id_source("video_cap_method")
                                             .selected_text(if preset.video_capture_method == "region" {
                                                 text.region_capture.to_string()
                                             } else {
                                                 preset.video_capture_method.strip_prefix("monitor:").unwrap_or("Unknown").to_string()
                                             })
                                             .show_ui(ui, |ui| {
                                                 if ui.selectable_value(&mut preset.video_capture_method, "region".to_string(), text.region_capture).clicked() {
                                                     preset_changed = true;
                                                 }
                                                 for monitor in &self.cached_monitors {
                                                     let val = format!("monitor:{}", monitor);
                                                     let label = format!("Full screen ({})", monitor);
                                                     if ui.selectable_value(&mut preset.video_capture_method, val, label).clicked() {
                                                         preset_changed = true;
                                                     }
                                                 }
                                             });
                                     });
                                 });
                                 // Hide everything else for video placeholder
                             } else {
                                 // STANDARD UI (Image/Audio)
                                 
                                 // Show prompt controls if it's an image preset OR a Gemini audio model (which can use a prompt for translation/analysis)
                                 let show_prompt_controls = !is_audio || (is_audio && preset.model.contains("gemini"));

                                 // 2. Main Configuration (Different for Image vs Audio)
                                 if show_prompt_controls {
                                // --- IMAGE PROMPT SETTINGS / GEMINI AUDIO PROMPT SETTINGS ---
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(text.prompt_label).strong());
                                        if ui.button(text.insert_lang_btn).clicked() {
                                            // ... (existing insert lang logic) ...
                                            let mut max_num = 0;
                                            for i in 1..=10 {
                                                if preset.prompt.contains(&format!("{{language{}}}", i)) {
                                                    max_num = i;
                                                }
                                            }
                                            let next_num = max_num + 1;
                                            preset.prompt.push_str(&format!(" {{language{}}} ", next_num));
                                            let key = format!("language{}", next_num);
                                            if !preset.language_vars.contains_key(&key) {
                                                preset.language_vars.insert(key, "Vietnamese".to_string());
                                            }
                                            preset_changed = true;
                                        }
                                    });
                                    
                                    if ui.add(egui::TextEdit::multiline(&mut preset.prompt).desired_rows(3).desired_width(f32::INFINITY)).changed() {
                                        preset_changed = true;
                                    }
                                    
                                    // FIX 4: Empty Prompt Warning
                                    if preset.prompt.trim().is_empty() {
                                        ui.colored_label(egui::Color32::RED, text.empty_prompt_warning);
                                    }
                                    
                                    // ... (existing language tag selectors logic) ...
                                    let mut detected_langs = Vec::new();
                                    for i in 1..=10 {
                                        let pattern = format!("{{language{}}}", i);
                                        if preset.prompt.contains(&pattern) {
                                            detected_langs.push(i);
                                        }
                                    }
                                    
                                    for num in detected_langs {
                                        let key = format!("language{}", num);
                                        if !preset.language_vars.contains_key(&key) {
                                            preset.language_vars.insert(key.clone(), "Vietnamese".to_string());
                                        }
                                        let label = match self.config.ui_language.as_str() {
                                            "vi" => format!("Ngôn ngữ cho thẻ {{language{}}}:", num),
                                            "ko" => format!("{{language{}}} 태그 언어:", num),
                                            _ => format!("Language for {{language{}}} tag:", num),
                                        };
                                        ui.horizontal(|ui| {
                                            ui.label(label);
                                            let current_lang = preset.language_vars.get(&key).cloned().unwrap_or_else(|| "Vietnamese".to_string());
                                            ui.menu_button(current_lang.clone(), |ui| {
                                                ui.style_mut().wrap = Some(false);
                                                ui.set_min_width(150.0);
                                                ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text(text.search_placeholder));
                                                let q = self.search_query.to_lowercase();
                                                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                    for lang in get_all_languages().iter() {
                                                        if q.is_empty() || lang.to_lowercase().contains(&q) {
                                                            if ui.button(lang).clicked() {
                                                                preset.language_vars.insert(key.clone(), lang.clone());
                                                                preset_changed = true;
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    }
                                                });
                                            });
                                        });
                                    }
                                });
                            }

                            if is_audio {
                                // --- AUDIO SOURCE SETTINGS ---
                                ui.group(|ui| {
                                    ui.label(egui::RichText::new(text.audio_source_label).strong());
                                    
                                    ui.horizontal(|ui| {
                                        if ui.radio_value(&mut preset.audio_source, "mic".to_string(), text.audio_src_mic).clicked() {
                                            preset_changed = true;
                                        }
                                        if ui.radio_value(&mut preset.audio_source, "device".to_string(), text.audio_src_device).clicked() {
                                            preset_changed = true;
                                        }
                                        if ui.checkbox(&mut preset.hide_recording_ui, text.hide_recording_ui_label).clicked() {
                                            preset_changed = true;
                                        }
                                    });
                                });
                            }

                            // 3. Model & Settings (Shared structure, filtered by type)
                            ui.group(|ui| {
                                ui.label(egui::RichText::new(text.model_section).strong());
                                
                                // Model selector + Streaming on same line
                                ui.horizontal(|ui| {
                                    let selected_model = get_model_by_id(&preset.model);
                                    // Display only the speed name (Nhanh, Rất Nhanh, etc.)
                                    let display_label = selected_model.as_ref()
                                        .map(|m| match self.config.ui_language.as_str() {
                                            "vi" => &m.name_vi,
                                            "ko" => &m.name_ko,
                                            _ => &m.name_en,
                                        })
                                        .map(|s| s.as_str())
                                        .unwrap_or(&preset.model);

                                    egui::ComboBox::from_id_source("model_selector")
                                        .selected_text(display_label)
                                        .show_ui(ui, |ui| {
                                            let target_type = if is_audio { ModelType::Audio } else { ModelType::Vision };
                                            for model in get_all_models() {
                                                if model.enabled && model.model_type == target_type {
                                                    // Show full details in dropdown: "Nhanh (meta-llama/...) - 1000 lượt/ngày"
                                                    let dropdown_label = format!("{} ({}) - {}", 
                                                        match self.config.ui_language.as_str() {
                                                            "vi" => &model.name_vi,
                                                            "ko" => &model.name_ko,
                                                            _ => &model.name_en,
                                                        },
                                                        model.full_name,
                                                        model.quota_limit
                                                    );
                                                    if ui.selectable_value(&mut preset.model, model.id.clone(), dropdown_label).clicked() {
                                                                         preset_changed = true;
                                                                         
                                                                         // START: NEW LOGIC FOR GEMINI AUDIO PROMPT PRE-FILL
                                                                         if is_audio && preset.model.contains("gemini") && preset.prompt.trim().is_empty() {
                                                                             preset.prompt = "Transcribe the audio accurately.".to_string();
                                                                         } else if is_audio && !preset.model.contains("gemini") && preset.prompt == "Transcribe the audio accurately." {
                                                                             // Reset prompt when switching away from Gemini Audio if it's the default
                                                                             preset.prompt = "".to_string();
                                                                         }
                                                                         // END: NEW LOGIC
                                                                     }
                                                                 }
                                                             }
                                                         });

                                                     // Hide Streaming control when "Hide Overlay" is active
                                                     if !preset.hide_overlay {
                                                         ui.label(text.streaming_label);
                                                         egui::ComboBox::from_id_source("stream_combo")
                                                             .selected_text(if preset.streaming_enabled { text.streaming_option_stream } else { text.streaming_option_wait })
                                                             .show_ui(ui, |ui| {
                                                                 if ui.selectable_value(&mut preset.streaming_enabled, false, text.streaming_option_wait).clicked() { preset_changed = true; }
                                                                 if ui.selectable_value(&mut preset.streaming_enabled, true, text.streaming_option_stream).clicked() { preset_changed = true; }
                                                             });
                                                     }
                                                    });

                                // Auto copy + Hide overlay on same line
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut preset.auto_copy, text.auto_copy_label).clicked() {
                                        preset_changed = true;
                                        if preset.auto_copy { preset.retranslate_auto_copy = false; }
                                    }
                                    if preset.auto_copy {
                                        if ui.checkbox(&mut preset.hide_overlay, text.hide_overlay_label).clicked() {
                                            preset_changed = true;
                                        }
                                    }
                                });
                            });

                            // 4. Retranslate (Shared)
                            // Audio usually needs retranslation? Yes, Transcribe -> Translate.
                            if !preset.hide_overlay {
                                ui.group(|ui| {
                                    ui.label(egui::RichText::new(text.retranslate_section).strong());
                                    
                                    // Enable retranslate + Target Language on same line
                                    ui.horizontal(|ui| {
                                        if ui.checkbox(&mut preset.retranslate, text.retranslate_checkbox).clicked() {
                                            preset_changed = true;
                                        }
                                        
                                        if preset.retranslate {
                                            ui.label(text.retranslate_to_label);
                                            let retrans_label = preset.retranslate_to.clone();
                                            ui.menu_button(retrans_label, |ui| {
                                                ui.style_mut().wrap = Some(false);
                                                ui.set_min_width(150.0);
                                                ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text(text.search_placeholder));
                                                let q = self.search_query.to_lowercase();
                                                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                    for lang in get_all_languages().iter() {
                                                        if q.is_empty() || lang.to_lowercase().contains(&q) {
                                                            if ui.button(lang).clicked() {
                                                                preset.retranslate_to = lang.clone();
                                                                preset_changed = true;
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    }
                                                });
                                            });
                                        }
                                    });

                                    if preset.retranslate {
                                        // Text Model Selector + Auto Copy on same line
                                        ui.horizontal(|ui| {
                                            ui.label(text.retranslate_model_label);
                                            let text_model = get_model_by_id(&preset.retranslate_model);
                                            // Display only the speed name
                                            let text_display_label = text_model.as_ref()
                                                .map(|m| match self.config.ui_language.as_str() {
                                                    "vi" => &m.name_vi,
                                                    "ko" => &m.name_ko,
                                                    _ => &m.name_en,
                                                })
                                                .map(|s| s.as_str())
                                                .unwrap_or(&preset.retranslate_model);
                                            
                                            egui::ComboBox::from_id_source("text_model_selector")
                                                .selected_text(text_display_label)
                                                .show_ui(ui, |ui| {
                                                    for model in get_all_models() {
                                                        if model.enabled && model.model_type == ModelType::Text {
                                                            // Show full details in dropdown: "Nhanh (meta-llama/...) - 1000 lượt/ngày"
                                                            let dropdown_label = format!("{} ({}) - {}", 
                                                                match self.config.ui_language.as_str() {
                                                                    "vi" => &model.name_vi,
                                                                    "ko" => &model.name_ko,
                                                                    _ => &model.name_en,
                                                                },
                                                                model.full_name,
                                                                model.quota_limit
                                                            );
                                                            if ui.selectable_value(&mut preset.retranslate_model, model.id.clone(), dropdown_label).clicked() {
                                                                preset_changed = true;
                                                            }
                                                        }
                                                    }
                                                });
                                            
                                            if ui.checkbox(&mut preset.retranslate_auto_copy, text.auto_copy_label).clicked() {
                                                 preset_changed = true;
                                                 if preset.retranslate_auto_copy { preset.auto_copy = false; }
                                             }
                                            });

                                            // Retranslate Settings - Hide Streaming control when "Hide Overlay" is active
                                            if !preset.hide_overlay {
                                             ui.horizontal(|ui| {
                                                 ui.label(text.streaming_label);
                                                 egui::ComboBox::from_id_source("retrans_stream_combo")
                                                     .selected_text(if preset.retranslate_streaming_enabled { text.streaming_option_stream } else { text.streaming_option_wait })
                                                     .show_ui(ui, |ui| {
                                                         if ui.selectable_value(&mut preset.retranslate_streaming_enabled, false, text.streaming_option_wait).clicked() { preset_changed = true; }
                                                         if ui.selectable_value(&mut preset.retranslate_streaming_enabled, true, text.streaming_option_stream).clicked() { preset_changed = true; }
                                                     });
                                             });
                                            }
                                            }
                                            });
                            }
                             }

                            // 5. Hotkeys (hidden for video placeholder presets)
                            if !is_video {
                               ui.group(|ui| {
                                   ui.label(egui::RichText::new(text.hotkeys_section).strong());
                                   
                                   let mut hotkey_to_remove = None;
                                   for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
                                       ui.horizontal(|ui| {
                                           ui.label(&hotkey.name);
                                           if ui.small_button("x").clicked() {
                                               hotkey_to_remove = Some(h_idx);
                                           }
                                       });
                                   }
                                   if let Some(h_idx) = hotkey_to_remove {
                                       preset.hotkeys.remove(h_idx);
                                       preset_changed = true;
                                   }

                                   if self.recording_hotkey_for_preset == Some(idx) {
                                       ui.horizontal(|ui| {
                                           ui.colored_label(egui::Color32::YELLOW, text.press_keys);
                                           if ui.button(text.cancel_label).clicked() {
                                               self.recording_hotkey_for_preset = None;
                                               self.hotkey_conflict_msg = None;
                                           }
                                       });
                                       if let Some(msg) = &self.hotkey_conflict_msg {
                                           ui.colored_label(egui::Color32::RED, msg);
                                       }
                                   } else {
                                       if ui.button(text.add_hotkey_button).clicked() {
                                           self.recording_hotkey_for_preset = Some(idx);
                                       }
                                   }
                               });
                            }

                            // Update the preset in the config
                            if idx < self.config.presets.len() {
                                self.config.presets[idx] = preset;
                                // Save if anything changed
                                if preset_changed {
                                    self.save_and_sync();
                                }
                            }
                        }
                    }
                });
            });

            // Removed Footer
        });
    }
    
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray_icon = None;
    }
}

pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let viet_font_name = "segoe_ui";
    let viet_font_path = "C:\\Windows\\Fonts\\segoeui.ttf";
    let viet_fallback_path = "C:\\Windows\\Fonts\\arial.ttf";
    let viet_data = std::fs::read(viet_font_path).or_else(|_| std::fs::read(viet_fallback_path));

    let korean_font_name = "malgun_gothic";
    let korean_font_path = "C:\\Windows\\Fonts\\malgun.ttf";
    let korean_data = std::fs::read(korean_font_path);

    if let Ok(data) = viet_data {
        fonts.font_data.insert(viet_font_name.to_owned(), egui::FontData::from_owned(data));
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) { vec.insert(0, viet_font_name.to_owned()); }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) { vec.insert(0, viet_font_name.to_owned()); }
    }
    if let Ok(data) = korean_data {
        fonts.font_data.insert(korean_font_name.to_owned(), egui::FontData::from_owned(data));
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) { 
            let idx = if vec.contains(&viet_font_name.to_string()) { 1 } else { 0 };
            vec.insert(idx, korean_font_name.to_owned()); 
        }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) { 
             let idx = if vec.contains(&viet_font_name.to_string()) { 1 } else { 0 };
             vec.insert(idx, korean_font_name.to_owned()); 
        }
    }
    ctx.set_fonts(fonts);
}
