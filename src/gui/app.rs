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
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::core::*;

use crate::gui::locale::LocaleText;
use crate::gui::key_mapping::egui_key_to_vk;
use crate::model_config::{get_all_models, ModelType, get_model_by_id};

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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                            
                            if preset.is_upcoming {
                                ui.add_enabled_ui(false, |ui| {
                                    let _ = ui.selectable_label(is_selected, &preset.name);
                                });
                            } else {
                                if ui.selectable_label(is_selected, &preset.name).clicked() {
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
                            if let Some(launcher) = &self.auto_launcher {
                                if ui.checkbox(&mut self.run_at_startup, text.startup_label).clicked() {
                                    if self.run_at_startup { let _ = launcher.enable(); } else { let _ = launcher.disable(); }
                                }
                            }

                            ui.add_space(20.0);
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
                        }
                        
                        ViewMode::Preset(idx) => {
                            // Ensure index is valid (could be invalid if just deleted)
                            if idx >= self.config.presets.len() {
                                self.view_mode = ViewMode::Global; 
                                return;
                            }

                            let mut preset = self.config.presets[idx].clone();
                            let mut preset_changed = false;
                            let is_vietnamese = self.config.ui_language == "vi";

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
                                
                                let selected_text = if preset.preset_type == "audio" { audio_label } else { image_label };
                                
                                egui::ComboBox::from_id_source("preset_type_combo")
                                    .selected_text(selected_text)
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_value(&mut preset.preset_type, "image".to_string(), image_label).clicked() {
                                            preset_changed = true;
                                        }
                                        ui.add_enabled_ui(false, |ui| {
                                            ui.selectable_value(&mut preset.preset_type, "audio".to_string(), audio_label);
                                        });
                                    });
                            });

                            // 2. Prompt & Language Tag
                            if !preset.is_upcoming {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(text.prompt_label).strong());
                                        if ui.button(text.insert_lang_btn).clicked() {
                                            // Find the next language badge number
                                            let mut max_num = 0;
                                            for i in 1..=10 {
                                                if preset.prompt.contains(&format!("{{language{}}}", i)) {
                                                    max_num = i;
                                                }
                                            }
                                            let next_num = max_num + 1;
                                            preset.prompt.push_str(&format!(" {{language{}}} ", next_num));
                                            
                                            // Initialize in language_vars if not present
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
                                    
                                    // Detect all {languageN} patterns in the prompt
                                    let mut detected_langs = Vec::new();
                                    for i in 1..=10 {
                                        let pattern = format!("{{language{}}}", i);
                                        if preset.prompt.contains(&pattern) {
                                            detected_langs.push(i);
                                        }
                                    }
                                    
                                    // Show language selectors for detected patterns
                                    for num in detected_langs {
                                        let key = format!("language{}", num);
                                        
                                        // Ensure the key exists in language_vars
                                        if !preset.language_vars.contains_key(&key) {
                                            preset.language_vars.insert(key.clone(), "Vietnamese".to_string());
                                        }
                                        
                                        // Always use numbered format: {language1}, {language2}, etc.
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

                                // 3. Model & Settings
                                ui.group(|ui| {
                                    ui.label(egui::RichText::new(text.model_section).strong());
                                    
                                    // Vision Model Selector
                                    let full_label = get_model_by_id(&preset.model)
                                        .map(|m| m.get_label(&self.config.ui_language))
                                        .unwrap_or_else(|| preset.model.clone());
                                    
                                    // Short label for outer display
                                    let short_label = full_label.split('(').next().unwrap_or(&full_label).trim().to_string();

                                    egui::ComboBox::from_id_source("vision_model_selector")
                                        .selected_text(short_label)
                                        .width(250.0)
                                        .show_ui(ui, |ui| {
                                            for model in get_all_models() {
                                                if model.enabled && model.model_type == ModelType::Vision {
                                                    if ui.selectable_value(&mut preset.model, model.id.clone(), model.get_label(&self.config.ui_language)).clicked() {
                                                        preset_changed = true;
                                                    }
                                                }
                                            }
                                        });

                                    ui.horizontal(|ui| {
                                        ui.label(text.streaming_label);
                                        egui::ComboBox::from_id_source("stream_combo")
                                            .selected_text(if preset.streaming_enabled { text.streaming_option_stream } else { text.streaming_option_wait })
                                            .show_ui(ui, |ui| {
                                                if ui.selectable_value(&mut preset.streaming_enabled, false, text.streaming_option_wait).clicked() { preset_changed = true; }
                                                if ui.selectable_value(&mut preset.streaming_enabled, true, text.streaming_option_stream).clicked() { preset_changed = true; }
                                            });
                                    });

                                    ui.horizontal(|ui| {
                                        if ui.checkbox(&mut preset.auto_copy, text.auto_copy_label).clicked() {
                                            preset_changed = true;
                                        }
                                        if preset.auto_copy {
                                            if ui.checkbox(&mut preset.hide_overlay, text.hide_overlay_label).clicked() {
                                                preset_changed = true;
                                            }
                                        }
                                    });
                                });

                                // 4. Retranslate
                                if !preset.hide_overlay {
                                    ui.group(|ui| {
                                        ui.label(egui::RichText::new(text.retranslate_section).strong());
                                        if ui.checkbox(&mut preset.retranslate, text.retranslate_checkbox).clicked() {
                                            preset_changed = true;
                                        }

                                        if preset.retranslate {
                                            // Target Language (Searchable - using menu_button)
                                            ui.horizontal(|ui| {
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
                                            });

                                            // Text Model Selector
                                            ui.horizontal(|ui| {
                                                ui.label(text.retranslate_model_label);
                                                let full_text_model = get_model_by_id(&preset.retranslate_model)
                                                    .map(|m| m.get_label(&self.config.ui_language))
                                                    .unwrap_or_else(|| preset.retranslate_model.clone());
                                                
                                                let short_text_model = full_text_model.split('(').next().unwrap_or(&full_text_model).trim().to_string();
                                                
                                                egui::ComboBox::from_id_source("text_model_selector")
                                                    .selected_text(short_text_model)
                                                    .width(180.0)
                                                    .show_ui(ui, |ui| {
                                                        for model in get_all_models() {
                                                            if model.enabled && model.model_type == ModelType::Text {
                                                                if ui.selectable_value(&mut preset.retranslate_model, model.id.clone(), model.get_label(&self.config.ui_language)).clicked() {
                                                                    preset_changed = true;
                                                                }
                                                            }
                                                        }
                                                    });
                                            });

                                            // Retranslate Streaming Toggle
                                            ui.horizontal(|ui| {
                                                ui.label(text.streaming_label);
                                                egui::ComboBox::from_id_source("retranslate_stream_combo")
                                                    .selected_text(if preset.retranslate_streaming_enabled { text.streaming_option_stream } else { text.streaming_option_wait })
                                                    .show_ui(ui, |ui| {
                                                        if ui.selectable_value(&mut preset.retranslate_streaming_enabled, false, text.streaming_option_wait).clicked() { preset_changed = true; }
                                                        if ui.selectable_value(&mut preset.retranslate_streaming_enabled, true, text.streaming_option_stream).clicked() { preset_changed = true; }
                                                    });
                                            });
                                        }
                                    });
                                }

                                // 5. Hotkeys
                                ui.group(|ui| {
                                    ui.label(egui::RichText::new(text.hotkey_bag_label).strong());
                                    
                                    let mut indices_to_remove = Vec::new();
                                    for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            ui.strong(&hotkey.name);
                                            if ui.small_button("✖").clicked() {
                                                indices_to_remove.push(h_idx);
                                            }
                                        });
                                    }
                                    for i in indices_to_remove.iter().rev() {
                                        preset.hotkeys.remove(*i);
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
