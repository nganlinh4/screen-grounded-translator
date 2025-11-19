
use eframe::egui;
use crate::config::{Config, save_config, ISO_LANGUAGES, UiLanguage};
use std::sync::{Arc, Mutex};
use tray_icon::{TrayIcon, TrayIconEvent, MouseButton, menu::{Menu, MenuEvent}};
use auto_launch::AutoLaunch;
use std::sync::mpsc::{Receiver, channel};

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

// --- Font Configuration ---
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
    restart_note: &'static str,
    startup_label: &'static str,
    fullscreen_note: &'static str,
    footer_note: &'static str,
}

impl LocaleText {
    fn get(lang: &UiLanguage) -> Self {
        match lang {
            UiLanguage::English => Self {
                api_section: "API Configuration",
                api_key_label: "Groq API Key:",
                get_key_link: "Get API Key at console.groq.com",
                lang_section: "Translation Target",
                search_placeholder: "Search language...",
                current_language_label: "Current:",
                hotkey_section: "Controls",
                hotkey_label: "Activation Hotkey:",
                restart_note: "Note: Restart app to apply hotkey changes.",
                startup_label: "Run at Windows Startup",
                fullscreen_note: "⚠ To use hotkey in fullscreen apps/games, run this app as Administrator.",
                footer_note: "Press hotkey and select region to translate. Closing this window minimizes to System Tray.",
            },
            UiLanguage::Vietnamese => Self {
                api_section: "Cấu Hình API",
                api_key_label: "Mã API Groq:",
                get_key_link: "Lấy mã tại console.groq.com",
                lang_section: "Ngôn Ngữ Đích",
                search_placeholder: "Tìm kiếm ngôn ngữ...",
                current_language_label: "Hiện tại:",
                hotkey_section: "Điều Khiển",
                hotkey_label: "Phím Tắt Kích Hoạt:",
                restart_note: "Lưu ý: Khởi động lại để áp dụng phím tắt mới.",
                startup_label: "Khởi động cùng Windows",
                fullscreen_note: "⚠ Để sử dụng phím tắt trong các ứng dụng/trò chơi fullscreen, hãy chạy ứng dụng này dưới quyền Quản trị viên.",
                footer_note: "Bấm hotkey và chọn vùng trên màn hình để dịch, tắt cửa sổ này thì ứng dụng sẽ tiếp tục chạy trong System Tray",
            },
            UiLanguage::Korean => Self {
                api_section: "API 구성",
                api_key_label: "Groq API 키:",
                get_key_link: "console.groq.com에서 키 발급",
                lang_section: "번역 대상 언어",
                search_placeholder: "언어 검색...",
                current_language_label: "현재:",
                hotkey_section: "단축키 설정",
                hotkey_label: "활성화 키:",
                restart_note: "참고: 단축키 변경은 앱을 재시작해야 적용됩니다.",
                startup_label: "Windows 시작 시 실행",
                fullscreen_note: "⚠ 풀스크린 앱/게임에서 단축키를 사용하려면 관리자 권한으로 이 앱을 실행하세요.",
                footer_note: "단축키를 눌러 번역할 영역을 선택하세요. 창을 닫으면 트레이에서 실행됩니다.",
            },
        }
    }
}

pub struct SettingsApp {
    config: Config,
    app_state_ref: Arc<Mutex<crate::AppState>>,
    search_query: String,
    tray_icon: TrayIcon,
    _tray_menu: Menu,
    event_rx: Receiver<UserEvent>,
    // Logic fields
    is_quitting: bool,
    run_at_startup: bool,
    auto_launcher: Option<AutoLaunch>,
    show_api_key: bool,
}

impl SettingsApp {
    pub fn new(config: Config, app_state: Arc<Mutex<crate::AppState>>, tray_icon: TrayIcon, tray_menu: Menu, ctx: egui::Context) -> Self {
        // Initialize AutoLaunch
        let app_name = "ScreenGroundedTranslator";
        let app_path = std::env::current_exe().unwrap();
        let args: &[&str] = &[]; // No command line args
        
        let auto = AutoLaunch::new(
            app_name,
            app_path.to_str().unwrap(),
            args,
        );

        let run_at_startup = auto.is_enabled().unwrap_or(false);

        let (tx, rx) = channel();

        let tx_tray = tx.clone();
        let ctx_tray = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = TrayIconEvent::receiver().recv() {
                let _ = tx_tray.send(UserEvent::Tray(event));
                ctx_tray.request_repaint();
            }
        });

        let tx_menu = tx.clone();
        let ctx_menu = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(event) = MenuEvent::receiver().recv() {
                if event.id.0 == "1001" {
                    // Force exit immediately from this thread
                    std::process::exit(0);
                }
                let _ = tx_menu.send(UserEvent::Menu(event));
                ctx_menu.request_repaint();
            }
        });

        Self {
            config,
            app_state_ref: app_state,
            search_query: String::new(),
            tray_icon,
            _tray_menu: tray_menu,
            event_rx: rx,
            is_quitting: false,
            run_at_startup,
            auto_launcher: Some(auto),
            show_api_key: false,
        }
    }

    fn save_and_sync(&mut self) {
        save_config(&self.config);
        let mut state = self.app_state_ref.lock().unwrap();
        state.config = self.config.clone();
        if state.config.hotkey_code != self.config.hotkey_code {
             state.hotkey_updated = true;
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- Handle Pending Events ---
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                UserEvent::Tray(tray_event) => {
                    if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = tray_event {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    }
                }
                UserEvent::Menu(menu_event) => {
                    // Fallback if thread didn't catch it (unlikely)
                    if menu_event.id.0 == "1001" {
                        let _ = self.tray_icon.set_visible(false);
                        std::process::exit(0);
                    }
                }
            }
        }

        // --- Handle Window Close Request ---
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.is_quitting {
                // If NOT quitting via menu, just hide (minimize to tray)
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        // --- Apply Theme ---
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
                    egui::ComboBox::from_id_source("header_lang_switch")
                        .width(60.0)
                        .selected_text(match self.config.ui_language {
                            UiLanguage::English => "EN",
                            UiLanguage::Vietnamese => "VI",
                            UiLanguage::Korean => "KO",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.config.ui_language, UiLanguage::English, "English");
                            ui.selectable_value(&mut self.config.ui_language, UiLanguage::Vietnamese, "Vietnamese");
                            ui.selectable_value(&mut self.config.ui_language, UiLanguage::Korean, "Korean");
                        });

                    if original_lang != self.config.ui_language {
                        self.save_and_sync();
                    }
                });
            });

            ui.add_space(15.0);

            // --- API Key ---
            ui.group(|ui| {
                ui.heading(text.api_section);
                ui.label(text.api_key_label);
                ui.horizontal(|ui| {
                    let available = ui.available_width() - 32.0;
                    if ui.add(egui::TextEdit::singleline(&mut self.config.api_key)
                        .password(!self.show_api_key)
                        .desired_width(available)).changed() {
                        self.save_and_sync();
                    }
                    let eye_icon = if self.show_api_key { "👁" } else { "🔒" };
                    if ui.button(eye_icon).on_hover_text(if self.show_api_key { "Hide" } else { "Show" }).clicked() {
                        self.show_api_key = !self.show_api_key;
                    }
                });
                if ui.link(text.get_key_link).clicked() {
                    let _ = open::that("https://console.groq.com/keys");
                }
            });

            ui.add_space(10.0);

            // --- Language ---
            ui.group(|ui| {
                ui.heading(text.lang_section);
                ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text(text.search_placeholder));
                ui.add_space(5.0);
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    let q = self.search_query.to_lowercase();
                    let filtered = ISO_LANGUAGES.iter().filter(|l| l.to_lowercase().contains(&q));
                    for lang in filtered {
                        if ui.radio_value(&mut self.config.target_language, lang.to_string(), *lang).clicked() {
                            self.save_and_sync();
                        }
                    }
                });
                ui.label(format!("{} {}", text.current_language_label, self.config.target_language));
            });

            ui.add_space(10.0);

            // --- Controls (Hotkey + Startup) ---
            ui.group(|ui| {
                ui.heading(text.hotkey_section);
                
                // Startup Checkbox
                if let Some(launcher) = &self.auto_launcher {
                    if ui.checkbox(&mut self.run_at_startup, text.startup_label).clicked() {
                         if self.run_at_startup {
                             let _ = launcher.enable();
                         } else {
                             let _ = launcher.disable();
                         }
                    }
                }
                
                ui.add_space(5.0);
                ui.label(text.hotkey_label);
                
                egui::ComboBox::from_id_source("hotkey_selector")
                    .selected_text(&self.config.hotkey_name)
                    .show_ui(ui, |ui| {
                        let keys = [
                            ("Tilde (~)", 192, "` / ~"),
                            ("F2", 113, "F2"),
                            ("F4", 115, "F4"),
                            ("F6", 117, "F6"),
                            ("F7", 118, "F7"),
                            ("F8", 119, "F8"),
                            ("F9", 120, "F9"),
                            ("F10", 121, "F10"),
                        ];
                        for (label, code, short) in keys {
                            if ui.selectable_label(self.config.hotkey_code == code, label).clicked() {
                                self.config.hotkey_code = code;
                                self.config.hotkey_name = short.to_string();
                                self.save_and_sync();
                            }
                        }
                    });
                 
                 let warn_color = if self.config.dark_mode { egui::Color32::YELLOW } else { egui::Color32::from_rgb(200, 0, 0) };
                 ui.small(egui::RichText::new(text.restart_note).color(warn_color));
                 ui.add_space(8.0);
                 ui.small(egui::RichText::new(text.fullscreen_note).color(warn_color));
                 });

            ui.add_space(20.0);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(text.footer_note).italics().weak());
            });
        });
        
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    // Clean exit handler
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Explicitly hide/remove the tray icon on exit
        let _ = self.tray_icon.set_visible(false);
    }
}