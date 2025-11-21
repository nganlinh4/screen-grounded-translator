use eframe::egui;
use crate::config::{Config, save_config, get_all_languages};
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

// Windows Modifier Constants
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

// --- Font Configuration (Unchanged) ---
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

// --- Localization ---
struct LocaleText {
    api_section: &'static str,
    api_key_label: &'static str,
    get_key_link: &'static str,
    lang_section: &'static str,
    search_placeholder: &'static str,
    current_language_label: &'static str,
    hotkey_section: &'static str,
    hotkey_label: &'static str,
    startup_label: &'static str,
    fullscreen_note: &'static str,
    footer_note: &'static str,
    auto_copy_label: &'static str,
    press_keys: &'static str,
    active_hotkeys_label: &'static str,
    add_hotkey_button: &'static str,
}

impl LocaleText {
    fn get(lang_code: &str) -> Self {
        match lang_code {
            "vi" => Self {
                api_section: "Cấu Hình API",
                api_key_label: "Mã API Groq:",
                get_key_link: "Lấy mã tại console.groq.com",
                lang_section: "Ngôn Ngữ Mục Tiêu",
                search_placeholder: "Tìm kiếm ngôn ngữ...",
                current_language_label: "Hiện tại:",
                hotkey_section: "Điều Khiển",
                hotkey_label: "Phím Tắt Kích Hoạt:",
                startup_label: "Khởi động cùng Windows",
                fullscreen_note: "⚠ Để sử dụng phím tắt trong các ứng dụng/trò chơi fullscreen, hãy chạy ứng dụng này dưới quyền Quản trị viên.",
                footer_note: "Bấm hotkey và chọn vùng trên màn hình để dịch, tắt cửa sổ này thì ứng dụng sẽ tiếp tục chạy trong System Tray",
                auto_copy_label: "Tự động copy bản dịch",
                press_keys: "Ấn phím/tổ hợp phím (vd: F1, Ctrl+Q)...",
                active_hotkeys_label: "Phím Tắt Hiện Tại:",
                add_hotkey_button: "+ Thêm Phím Tắt",
            },
            "ko" => Self {
                api_section: "API 구성",
                api_key_label: "Groq API 키:",
                get_key_link: "console.groq.com에서 키 발급",
                lang_section: "번역 대상 언어",
                search_placeholder: "언어 검색...",
                current_language_label: "현재:",
                hotkey_section: "단축키 설정",
                hotkey_label: "활성화 키:",
                startup_label: "Windows 시작 시 실행",
                fullscreen_note: "⚠ 풀스크린 앱/게임에서 단축키를 사용하려면 관리자 권한으로 이 앱을 실행하세요.",
                footer_note: "단축키를 눌러 번역할 영역을 선택하세요. 창을 닫으면 트레이에서 실행됩니다.",
                auto_copy_label: "번역 자동 복사",
                press_keys: "키/단축키를 입력하세요 (예: F1, Ctrl+Q)...",
                active_hotkeys_label: "활성화된 단축키:",
                add_hotkey_button: "+ 단축키 추가",
            },
            _ => Self {
                // English (default)
                api_section: "API Configuration",
                api_key_label: "Groq API Key:",
                get_key_link: "Get API Key at console.groq.com",
                lang_section: "Translation Target",
                search_placeholder: "Search language...",
                current_language_label: "Current:",
                hotkey_section: "Controls",
                hotkey_label: "Activation Hotkey:",
                startup_label: "Run at Windows Startup",
                fullscreen_note: "⚠ To use hotkey in fullscreen apps/games, run this app as Administrator.",
                footer_note: "Press hotkey and select region to translate. Closing this window minimizes to System Tray.",
                auto_copy_label: "Auto copy translation",
                press_keys: "Press key/combination (e.g. F1, Ctrl+Q)...",
                active_hotkeys_label: "Active Hotkeys:",
                add_hotkey_button: "+ Add Hotkey",
            },
        }
    }
}

// Global signal for window restoration
lazy_static::lazy_static! {
    static ref RESTORE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub struct SettingsApp {
    config: Config,
    app_state_ref: Arc<Mutex<crate::AppState>>,
    search_query: String,
    tray_icon: Option<TrayIcon>,
    _tray_menu: Menu,
    event_rx: Receiver<UserEvent>,
    is_quitting: bool,
    run_at_startup: bool,
    auto_launcher: Option<AutoLaunch>,
    show_api_key: bool,
    recording_hotkey: bool,
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
                                    let title = w!("Screen Grounded Translator");
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
                            let mut hwnd = FindWindowW(PCWSTR(class_name.as_ptr()), None);
                            if hwnd.0 == 0 {
                                let title = w!("Screen Grounded Translator");
                                hwnd = FindWindowW(None, PCWSTR(title.as_ptr()));
                            }
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
            recording_hotkey: false,
        }
    }

    fn save_and_sync(&mut self) {
        let mut state = self.app_state_ref.lock().unwrap();
        if state.config.hotkeys != self.config.hotkeys {
            state.hotkeys_updated = true;
        }
        state.config = self.config.clone();
        drop(state);
        save_config(&self.config);
    }
    
    fn restore_window(&self, ctx: &egui::Context) {
         ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
         ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
         ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
         ctx.request_repaint();
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if RESTORE_SIGNAL.swap(false, Ordering::SeqCst) {
            self.restore_window(ctx);
        }

        // --- Handle Hotkey Recording (Support Combinations) ---
        if self.recording_hotkey {
            let mut key_to_record: Option<(u32, String)> = None;
            let mut modifiers_bitmap = 0;
            
            // Check modifiers and keys using egui state
            ctx.input(|i| {
                if i.modifiers.ctrl { modifiers_bitmap |= MOD_CONTROL; }
                if i.modifiers.alt { modifiers_bitmap |= MOD_ALT; }
                if i.modifiers.shift { modifiers_bitmap |= MOD_SHIFT; }
                // mac command key usually maps to Win on Windows/Linux for egui
                if i.modifiers.command { modifiers_bitmap |= MOD_WIN; } 

                // Check for pressed keys
                for event in &i.events {
                    if let egui::Event::Key { key, pressed: true, .. } = event {
                        if let Some(vk) = egui_key_to_vk(key) {
                            // Filter out keys that are just modifier triggers themselves
                            // (16=Shift, 17=Ctrl, 18=Alt, 91=Win, 92=RWin)
                            if !matches!(vk, 16 | 17 | 18 | 91 | 92) {
                                let key_name = format!("{:?}", key).trim_start_matches("Key").to_string();
                                key_to_record = Some((vk, key_name));
                            }
                        }
                    }
                }
            });

            // If a non-modifier key is pressed, record the combo
            if let Some((vk, key_name)) = key_to_record {
                // Build name string
                let mut name_parts = Vec::new();
                if (modifiers_bitmap & MOD_CONTROL) != 0 { name_parts.push("Ctrl".to_string()); }
                if (modifiers_bitmap & MOD_ALT) != 0 { name_parts.push("Alt".to_string()); }
                if (modifiers_bitmap & MOD_SHIFT) != 0 { name_parts.push("Shift".to_string()); }
                if (modifiers_bitmap & MOD_WIN) != 0 { name_parts.push("Win".to_string()); }
                name_parts.push(key_name);
                
                let new_hotkey = crate::config::Hotkey {
                    code: vk,
                    modifiers: modifiers_bitmap,
                    name: name_parts.join(" + "),
                };

                // Avoid duplicates
                if !self.config.hotkeys.iter().any(|h| h.code == vk && h.modifiers == modifiers_bitmap) {
                    self.config.hotkeys.push(new_hotkey);
                    self.recording_hotkey = false;
                    self.save_and_sync();
                }
            }
        }

        // --- Handle Pending Events ---
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

        egui::CentralPanel::default().show(ctx, |ui| {
            // --- HEADER ---
            ui.horizontal(|ui| {
                ui.heading("Made by ");
                ui.add(egui::Hyperlink::from_label_and_url(
                    egui::RichText::new("nganlinh4").heading(),
                    "https://github.com/nganlinh4/screen-grounded-translator"
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let theme_icon = if self.config.dark_mode { "🌙" } else { "☀" };
                    if ui.button(theme_icon).on_hover_text("Toggle Theme").clicked() {
                        self.config.dark_mode = !self.config.dark_mode;
                        self.save_and_sync();
                    }
                    ui.add_space(5.0);
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
            });

            ui.add_space(15.0);

            // --- TWO COLUMN LAYOUT ---
            ui.columns(2, |cols| {
                // LEFT COLUMN: API Key and Language
                cols[0].group(|ui| {
                    ui.heading(text.api_section);
                    ui.label(text.api_key_label);
                    ui.horizontal(|ui| {
                        let available = ui.available_width() - 32.0;
                        if ui.add(egui::TextEdit::singleline(&mut self.config.api_key).password(!self.show_api_key).desired_width(available)).changed() {
                            self.save_and_sync();
                        }
                        let eye_icon = if self.show_api_key { "👁" } else { "🔒" };
                        if ui.button(eye_icon).clicked() { self.show_api_key = !self.show_api_key; }
                    });
                    if ui.link(text.get_key_link).clicked() { let _ = open::that("https://console.groq.com/keys"); }
                });

                cols[0].add_space(10.0);

                cols[0].group(|ui| {
                    ui.heading(text.lang_section);
                    ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text(text.search_placeholder));
                    ui.add_space(5.0);
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        let q = self.search_query.to_lowercase();
                        let all_languages = get_all_languages();
                        let filtered = all_languages.iter().filter(|l| l.to_lowercase().contains(&q));
                        for lang in filtered {
                            if ui.radio_value(&mut self.config.target_language, lang.clone(), lang).clicked() {
                                self.save_and_sync();
                            }
                        }
                    });
                    ui.label(format!("{} {}", text.current_language_label, self.config.target_language));
                });

                // RIGHT COLUMN: Controls
                cols[1].group(|ui| {
                    ui.heading(text.hotkey_section);
                if let Some(launcher) = &self.auto_launcher {
                    if ui.checkbox(&mut self.run_at_startup, text.startup_label).clicked() {
                         if self.run_at_startup { let _ = launcher.enable(); } else { let _ = launcher.disable(); }
                    }
                }
                ui.add_space(8.0);
                if ui.checkbox(&mut self.config.auto_copy, text.auto_copy_label).clicked() { self.save_and_sync(); }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(text.hotkey_label).strong());
                
                    // List Hotkeys in a grid layout
                    let hotkey_list: Vec<_> = self.config.hotkeys.iter().cloned().collect();
                    if !hotkey_list.is_empty() {
                        ui.label(text.active_hotkeys_label);
                        let mut grid_indices_to_remove = Vec::new();
                        egui::Grid::new("hotkey_grid")
                            .num_columns(2)
                            .spacing([8.0, 5.0])
                            .show(ui, |ui| {
                                for (idx, hotkey) in hotkey_list.iter().enumerate() {
                                    ui.strong(&hotkey.name);
                                    if ui.small_button("✖").on_hover_text("Remove").clicked() {
                                        grid_indices_to_remove.push(idx);
                                    }
                                    ui.end_row();
                                }
                            });
                        
                        // Remove hotkeys in reverse order to maintain correct indices
                        for idx in grid_indices_to_remove.iter().rev() {
                            self.config.hotkeys.remove(*idx);
                        }
                        if !grid_indices_to_remove.is_empty() {
                            self.save_and_sync();
                        }
                    }
                    
                    // Recorder
                    if self.recording_hotkey {
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::YELLOW, text.press_keys);
                            if ui.button("Cancel").clicked() {
                                self.recording_hotkey = false;
                            }
                        });
                    } else {
                        if ui.button(text.add_hotkey_button).clicked() {
                            self.recording_hotkey = true;
                        }
                    }
                      
                    let warn_color = if self.config.dark_mode { egui::Color32::YELLOW } else { egui::Color32::from_rgb(200, 0, 0) };
                    ui.small(egui::RichText::new(text.fullscreen_note).color(warn_color));
                });
            });

            ui.add_space(20.0);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(text.footer_note).italics().weak());
            });
        });
        
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray_icon = None;
    }
}

// Expanded Mapping Function: egui Key -> Windows Virtual Key (VK)
// This covers Function keys, arrows, delete/insert, home/end, and standard alphanumerics
fn egui_key_to_vk(key: &egui::Key) -> Option<u32> {
    match key {
        // Numbers
        egui::Key::Num0 => Some(0x30), egui::Key::Num1 => Some(0x31), egui::Key::Num2 => Some(0x32),
        egui::Key::Num3 => Some(0x33), egui::Key::Num4 => Some(0x34), egui::Key::Num5 => Some(0x35),
        egui::Key::Num6 => Some(0x36), egui::Key::Num7 => Some(0x37), egui::Key::Num8 => Some(0x38),
        egui::Key::Num9 => Some(0x39),
        // Letters
        egui::Key::A => Some(0x41), egui::Key::B => Some(0x42), egui::Key::C => Some(0x43),
        egui::Key::D => Some(0x44), egui::Key::E => Some(0x45), egui::Key::F => Some(0x46),
        egui::Key::G => Some(0x47), egui::Key::H => Some(0x48), egui::Key::I => Some(0x49),
        egui::Key::J => Some(0x4A), egui::Key::K => Some(0x4B), egui::Key::L => Some(0x4C),
        egui::Key::M => Some(0x4D), egui::Key::N => Some(0x4E), egui::Key::O => Some(0x4F),
        egui::Key::P => Some(0x50), egui::Key::Q => Some(0x51), egui::Key::R => Some(0x52),
        egui::Key::S => Some(0x53), egui::Key::T => Some(0x54), egui::Key::U => Some(0x55),
        egui::Key::V => Some(0x56), egui::Key::W => Some(0x57), egui::Key::X => Some(0x58),
        egui::Key::Y => Some(0x59), egui::Key::Z => Some(0x5A),
        // Function Keys
        egui::Key::F1 => Some(0x70), egui::Key::F2 => Some(0x71), egui::Key::F3 => Some(0x72),
        egui::Key::F4 => Some(0x73), egui::Key::F5 => Some(0x74), egui::Key::F6 => Some(0x75),
        egui::Key::F7 => Some(0x76), egui::Key::F8 => Some(0x77), egui::Key::F9 => Some(0x78),
        egui::Key::F10 => Some(0x79), egui::Key::F11 => Some(0x7A), egui::Key::F12 => Some(0x7B),
        egui::Key::F13 => Some(0x7C), egui::Key::F14 => Some(0x7D), egui::Key::F15 => Some(0x7E),
        egui::Key::F16 => Some(0x7F), egui::Key::F17 => Some(0x80), egui::Key::F18 => Some(0x81),
        egui::Key::F19 => Some(0x82), egui::Key::F20 => Some(0x83),
        // Navigation / Editing
        egui::Key::Escape => Some(0x1B),
        egui::Key::Insert => Some(0x2D),
        egui::Key::Delete => Some(0x2E),
        egui::Key::Home => Some(0x24),
        egui::Key::End => Some(0x23),
        egui::Key::PageUp => Some(0x21),
        egui::Key::PageDown => Some(0x22),
        egui::Key::ArrowLeft => Some(0x25),
        egui::Key::ArrowUp => Some(0x26),
        egui::Key::ArrowRight => Some(0x27),
        egui::Key::ArrowDown => Some(0x28),
        egui::Key::Backspace => Some(0x08),
        egui::Key::Enter => Some(0x0D),
        egui::Key::Space => Some(0x20),
        egui::Key::Tab => Some(0x09),
        // Symbols
        egui::Key::Backtick => Some(0xC0), // `
        egui::Key::Minus => Some(0xBD),    // -
        egui::Key::Plus => Some(0xBB),     // = (Plus is usually shift+=)
        egui::Key::OpenBracket => Some(0xDB), // [
        egui::Key::CloseBracket => Some(0xDD), // ]
        egui::Key::Backslash => Some(0xDC), // \
        egui::Key::Semicolon => Some(0xBA), // ;
        egui::Key::Comma => Some(0xBC),     // ,
        egui::Key::Period => Some(0xBE),    // .
        egui::Key::Slash => Some(0xBF),     // /
        _ => None,
    }
}