use eframe::egui;
use crate::config::Config;
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button};
use crate::model_config::{get_all_models, get_all_models_with_ollama};
use crate::updater::{Updater, UpdateStatus};
use std::collections::HashMap;
use auto_launch::AutoLaunch;
use super::node_graph::request_node_graph_view_reset;

const API_KEY_FIELD_WIDTH: f32 = 320.0;

pub fn render_global_settings(
    ui: &mut egui::Ui,
    config: &mut Config,
    show_api_key: &mut bool,
    show_gemini_api_key: &mut bool,
    show_openrouter_api_key: &mut bool,
    usage_stats: &HashMap<String, String>,
    updater: &Option<Updater>,
    update_status: &UpdateStatus,
    run_at_startup: &mut bool,
    auto_launcher: &Option<AutoLaunch>,
    current_admin_state: bool, 
    text: &LocaleText,
    show_usage_modal: &mut bool,
    show_tts_modal: &mut bool,
    _cached_audio_devices: &std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
) -> bool {
    let mut changed = false;
    
    let is_dark = ui.visuals().dark_mode;
    let card_bg = if is_dark {
        egui::Color32::from_rgba_unmultiplied(28, 32, 42, 250)  // Darker for better text contrast
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 255)
    };
    let card_stroke = if is_dark {
        egui::Stroke::new(1.0, egui::Color32::from_gray(50))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(210))
    };

    ui.add_space(5.0);
    
    // === API KEYS CARD ===
    egui::Frame::new()
        .fill(card_bg)
        .stroke(card_stroke)
        .inner_margin(12.0)
        .corner_radius(10.0)
        .show(ui, |ui| {
            // Header row with title and provider checkboxes
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(text.api_keys_header).strong().size(14.0));
                ui.add_space(16.0);
                
                
                if ui.checkbox(&mut config.use_groq, text.use_groq_checkbox).changed() {
                    changed = true;
                }
                if ui.checkbox(&mut config.use_gemini, text.use_gemini_checkbox).changed() {
                    changed = true;
                }
                if ui.checkbox(&mut config.use_openrouter, text.use_openrouter_checkbox).changed() {
                    changed = true;
                }
                if ui.checkbox(&mut config.use_ollama, "Ollama").changed() {
                    changed = true;
                }
            });
            ui.add_space(6.0);
            
            // Groq API Key (only show if enabled)
            if config.use_groq {
                ui.horizontal(|ui| {
                    ui.label(text.groq_label);
                    if ui.link(text.get_key_link).clicked() { let _ = open::that("https://console.groq.com/keys"); }
                });
                ui.horizontal(|ui| {
                    if ui.add(egui::TextEdit::singleline(&mut config.api_key).password(!*show_api_key).desired_width(API_KEY_FIELD_WIDTH)).changed() {
                        changed = true;
                    }
                    let eye_icon = if *show_api_key { Icon::EyeOpen } else { Icon::EyeClosed };
                    if icon_button(ui, eye_icon).clicked() { *show_api_key = !*show_api_key; }
                });
                ui.add_space(8.0);
            }
            
            // Gemini API Key (only show if enabled)
            if config.use_gemini {
                ui.horizontal(|ui| {
                    ui.label(text.gemini_api_key_label);
                    if ui.link(text.gemini_get_key_link).clicked() { let _ = open::that("https://aistudio.google.com/app/apikey"); }
                });
                ui.horizontal(|ui| {
                    if ui.add(egui::TextEdit::singleline(&mut config.gemini_api_key).password(!*show_gemini_api_key).desired_width(API_KEY_FIELD_WIDTH)).changed() {
                        changed = true;
                    }
                    let eye_icon = if *show_gemini_api_key { Icon::EyeOpen } else { Icon::EyeClosed };
                    if icon_button(ui, eye_icon).clicked() { *show_gemini_api_key = !*show_gemini_api_key; }
                });
                ui.add_space(8.0);
            }
            
            // OpenRouter API Key (only show if enabled)
            if config.use_openrouter {
                ui.horizontal(|ui| {
                    ui.label(text.openrouter_api_key_label);
                    if ui.link(text.openrouter_get_key_link).clicked() { let _ = open::that("https://openrouter.ai/settings/keys"); }
                });
                ui.horizontal(|ui| {
                    if ui.add(egui::TextEdit::singleline(&mut config.openrouter_api_key).password(!*show_openrouter_api_key).desired_width(API_KEY_FIELD_WIDTH)).changed() {
                        changed = true;
                    }
                    let eye_icon = if *show_openrouter_api_key { Icon::EyeOpen } else { Icon::EyeClosed };
                    if icon_button(ui, eye_icon).clicked() { *show_openrouter_api_key = !*show_openrouter_api_key; }
                });
                ui.add_space(8.0);
            }
            
            // Ollama (Local AI) - only show URL field if enabled
             if config.use_ollama {
                 ui.horizontal(|ui| {
                     ui.label("Ollama URL:");
                     if ui.link(text.ollama_url_guide).clicked() { let _ = open::that("https://docs.ollama.com/api/introduction#base-url"); }
                 });
                 ui.horizontal(|ui| {
                     if ui.add(egui::TextEdit::singleline(&mut config.ollama_base_url).desired_width(API_KEY_FIELD_WIDTH)).changed() {
                         changed = true;
                     }
                     // Show status if available
                     if let Some(status) = ui.ctx().memory(|mem| mem.data.get_temp::<String>(egui::Id::new("ollama_status"))) {
                         ui.label(egui::RichText::new(&status).size(11.0));
                     }
                 });
             }
        });

    ui.add_space(10.0);
    
    // === USAGE STATISTICS & TTS SETTINGS BUTTONS ===
    let is_dark = ui.visuals().dark_mode;
    let stats_bg = if is_dark { 
        egui::Color32::from_rgb(50, 100, 110)  // Teal for dark mode
    } else { 
        egui::Color32::from_rgb(90, 160, 170)  // Lighter teal for light mode
    };
    
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new(egui::RichText::new(format!("📊 {}", text.usage_statistics_title)).color(egui::Color32::WHITE).strong())
            .fill(stats_bg)
            .corner_radius(10.0))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(text.usage_statistics_tooltip)
            .clicked() 
        {
            *show_usage_modal = true;
        }

        ui.add_space(10.0);

        let tts_bg = if is_dark { 
            egui::Color32::from_rgb(100, 80, 120)  // Purple for dark mode
        } else { 
            egui::Color32::from_rgb(180, 140, 200)  // Lighter purple for light mode
        };
        
        if ui.add(egui::Button::new(egui::RichText::new(format!("🔊 {}", text.tts_settings_button)).color(egui::Color32::WHITE).strong())
            .fill(tts_bg)
            .corner_radius(10.0))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            *show_tts_modal = true;
        }
    });
    
    // === USAGE STATISTICS MODAL ===
    render_usage_modal(ui, usage_stats, text, show_usage_modal, config.use_groq, config.use_gemini, config.use_openrouter, config.use_ollama);

    // === TTS SETTINGS MODAL ===
    if render_tts_settings_modal(ui, config, text, show_tts_modal) {
        changed = true;
    }

    ui.add_space(10.0);

    // === SOFTWARE UPDATE CARD ===
    egui::Frame::new()
        .fill(card_bg)
        .stroke(card_stroke)
        .inner_margin(12.0)
        .corner_radius(10.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text.software_update_header).strong().size(14.0));
            ui.add_space(6.0);
            render_update_section_content(ui, updater, update_status, text);
        });

    ui.add_space(10.0);
    
    // === STARTUP OPTIONS CARD ===
    egui::Frame::new()
        .fill(card_bg)
        .stroke(card_stroke)
        .inner_margin(12.0)
        .corner_radius(10.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text.startup_display_header).strong().size(14.0));
            ui.add_space(6.0);
            
            // Main startup toggle
            ui.horizontal(|ui| {
                if let Some(launcher) = auto_launcher {
                    let mut startup_toggle = *run_at_startup;
                    if ui.checkbox(&mut startup_toggle, text.startup_label).clicked() {
                        if startup_toggle && !(*run_at_startup) {
                            if config.run_as_admin_on_startup && current_admin_state {
                                if crate::gui::utils::set_admin_startup(true) {
                                    let _ = launcher.disable();
                                    *run_at_startup = true;
                                    changed = true;
                                }
                            } else {
                                std::thread::spawn(|| {
                                    crate::gui::utils::set_admin_startup(false);
                                });
                                let _ = launcher.enable();
                                *run_at_startup = true;
                                changed = true;
                            }
                        } else if !startup_toggle && *run_at_startup {
                            std::thread::spawn(|| {
                                crate::gui::utils::set_admin_startup(false);
                            });
                            let _ = launcher.disable();
                            config.run_as_admin_on_startup = false;
                            config.start_in_tray = false;
                            *run_at_startup = false;
                            changed = true;
                        }
                    }
                }
            });

            // Admin Mode Sub-option
            if *run_at_startup {
                ui.indent("admin_indent", |ui| {
                    let mut is_admin_mode = config.run_as_admin_on_startup;
                    let checkbox_label = text.admin_startup_on;
                    
                    if current_admin_state {
                        if ui.checkbox(&mut is_admin_mode, checkbox_label).clicked() {
                            if is_admin_mode && !config.run_as_admin_on_startup {
                                if crate::gui::utils::set_admin_startup(true) {
                                    config.run_as_admin_on_startup = true;
                                    if let Some(launcher) = auto_launcher {
                                        let _ = launcher.disable();
                                    }
                                    changed = true;
                                }
                            } else if !is_admin_mode && config.run_as_admin_on_startup {
                                std::thread::spawn(|| {
                                    crate::gui::utils::set_admin_startup(false);
                                });
                                config.run_as_admin_on_startup = false;
                                if let Some(launcher) = auto_launcher {
                                    let _ = launcher.enable();
                                }
                                changed = true;
                            }
                        }
                    } else {
                        let mut _is_admin_mode_disabled = config.run_as_admin_on_startup;
                        ui.add_enabled_ui(false, |ui| {
                            ui.checkbox(&mut _is_admin_mode_disabled, checkbox_label);
                        });
                        ui.label(
                            egui::RichText::new(text.admin_startup_fail)
                                .size(11.0)
                                .color(egui::Color32::from_rgb(200, 100, 50))
                        );
                    }

                    if config.run_as_admin_on_startup && current_admin_state {
                        ui.label(
                            egui::RichText::new(text.admin_startup_success)
                                .size(11.0)
                                .color(egui::Color32::from_rgb(34, 139, 34))
                        );
                    }
                });

                if ui.checkbox(&mut config.start_in_tray, text.start_in_tray_label).clicked() {
                    changed = true;
                }
            }
            
            ui.add_space(8.0);
            
            // Graphics Mode + Reset button on same row
            ui.horizontal(|ui| {
                ui.label(text.graphics_mode_label);
                
                let current_label = match config.ui_language.as_str() {
                    "vi" => if config.graphics_mode == "minimal" { "Tối giản" } else { "Tiêu chuẩn" },
                    "ko" => if config.graphics_mode == "minimal" { "최소" } else { "표준" },
                    _ => if config.graphics_mode == "minimal" { "Minimal" } else { "Standard" },
                };
                
                egui::ComboBox::from_id_salt("graphics_mode_combo")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(config.graphics_mode == "standard", text.graphics_mode_standard).clicked() {
                            config.graphics_mode = "standard".to_string();
                            changed = true;
                        }
                        if ui.selectable_label(config.graphics_mode == "minimal", text.graphics_mode_minimal).clicked() {
                            config.graphics_mode = "minimal".to_string();
                            changed = true;
                        }
                    });
                
                // Big gap to simulate right alignment
                ui.add_space(80.0);
                
                // Reset button
                let reset_bg = if is_dark { 
                    egui::Color32::from_rgb(120, 60, 60) 
                } else { 
                    egui::Color32::from_rgb(220, 140, 140) 
                };
                if ui.add(egui::Button::new(egui::RichText::new(text.reset_defaults_btn).color(egui::Color32::WHITE))
                    .fill(reset_bg)
                    .corner_radius(8.0))
                    .clicked() {
                    let saved_groq_key = config.api_key.clone();
                    let saved_gemini_key = config.gemini_api_key.clone();
                    let saved_openrouter_key = config.openrouter_api_key.clone();
                    let saved_language = config.ui_language.clone();
                    let saved_use_groq = config.use_groq;
                    let saved_use_gemini = config.use_gemini;
                    let saved_use_openrouter = config.use_openrouter;
                    let saved_use_ollama = config.use_ollama;
                    let saved_ollama_base_url = config.ollama_base_url.clone();
                    // Realtime model reset to default (google-gemma)
                    
                    *config = Config::default();
                    
                    config.api_key = saved_groq_key;
                    config.gemini_api_key = saved_gemini_key;
                    config.openrouter_api_key = saved_openrouter_key;
                    config.ui_language = saved_language;
                    config.use_groq = saved_use_groq;
                    config.use_gemini = saved_use_gemini;
                    config.use_openrouter = saved_use_openrouter;
                    config.use_ollama = saved_use_ollama;
                    config.ollama_base_url = saved_ollama_base_url;
                    // config.realtime_translation_model = saved_realtime_model;
                    request_node_graph_view_reset(ui.ctx());
                    changed = true;
                }
            });
            

        });

    changed
}

fn render_usage_modal(
    ui: &mut egui::Ui, 
    usage_stats: &HashMap<String, String>, 
    text: &LocaleText,
    show_modal: &mut bool,
    use_groq: bool,
    use_gemini: bool,
    use_openrouter: bool,
    use_ollama: bool,
) {
    if !*show_modal {
        return;
    }
    
    egui::Window::new(format!("📊 {}", text.usage_statistics_title))
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .default_width(400.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            // Header with title and close button
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("📊 {}", text.usage_statistics_title)).strong().size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close).clicked() {
                        *show_modal = false;
                    }
                });
            });
            ui.separator();
            ui.add_space(4.0);
            
            // Get all models including Ollama models from cache
            let all_models = if use_ollama {
                get_all_models_with_ollama()
            } else {
                get_all_models().to_vec()
            };
            
            let mut shown_models = std::collections::HashSet::new();
            
            egui::ScrollArea::vertical()
                .max_height(450.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if use_groq {
                    egui::CollapsingHeader::new(egui::RichText::new("⚡ Groq").strong().size(13.0))
                        .default_open(true)
                        .show(ui, |ui| {
                        egui::Grid::new("groq_grid").striped(true).show(ui, |ui| {
                            ui.label(egui::RichText::new(text.usage_model_column).strong().size(11.0));
                            ui.label(egui::RichText::new(text.usage_remaining_column).strong().size(11.0));
                            ui.end_row();
                            
                            for model in &all_models {
                                if !model.enabled || model.provider != "groq" { continue; }
                                if shown_models.contains(&model.full_name) { continue; }
                                shown_models.insert(model.full_name.clone());
                                
                                ui.label(&model.full_name);
                                let status = usage_stats.get(&model.full_name).cloned().unwrap_or_else(|| "??? / ?".to_string());
                                ui.label(status);
                                ui.end_row();
                            }
                            
                            // Add llama-3.1-8b-instant (realtime translation model)
                            if !shown_models.contains("llama-3.1-8b-instant") {
                                shown_models.insert("llama-3.1-8b-instant".to_string());
                                ui.label("llama-3.1-8b-instant");
                                let status = usage_stats.get("llama-3.1-8b-instant").cloned().unwrap_or_else(|| "??? / ?".to_string());
                                ui.label(status);
                                ui.end_row();
                            }
                        });
                    });
                }
                
                if use_gemini {
                    egui::CollapsingHeader::new(egui::RichText::new("✨ Google Gemini").strong().size(13.0))
                        .default_open(true)
                        .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(text.usage_model_column).strong().size(11.0));
                            ui.add_space(120.0);
                            ui.hyperlink_to(text.usage_check_link, "https://aistudio.google.com/usage?timeRange=last-1-day&tab=rate-limit");
                        });
                        ui.add_space(4.0);
                        
                        for model in &all_models {
                            if !model.enabled || model.provider != "google" { continue; }
                            if shown_models.contains(&model.full_name) { continue; }
                            shown_models.insert(model.full_name.clone());
                            
                            ui.label(&model.full_name);
                        }
                    });
                }
                
                if use_openrouter {
                    egui::CollapsingHeader::new(egui::RichText::new("🌐 OpenRouter").strong().size(13.0))
                        .default_open(true)
                        .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(text.usage_model_column).strong().size(11.0));
                            ui.add_space(120.0);
                            ui.hyperlink_to(text.usage_check_link, "https://openrouter.ai/activity");
                        });
                        ui.add_space(4.0);
                        
                        for model in &all_models {
                            if !model.enabled || model.provider != "openrouter" { continue; }
                            if shown_models.contains(&model.full_name) { continue; }
                            shown_models.insert(model.full_name.clone());
                            
                            ui.label(&model.full_name);
                        }
                    });
                }
                
                if use_ollama {
                    egui::CollapsingHeader::new(egui::RichText::new("🏠 Ollama (Local)").strong().size(13.0))
                        .default_open(true)
                        .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(text.usage_model_column).strong().size(11.0));
                            ui.add_space(120.0);
                            ui.label("∞ Unlimited");
                        });
                        ui.add_space(4.0);
                        
                        for model in &all_models {
                            if !model.enabled || model.provider != "ollama" { continue; }
                            if shown_models.contains(&model.full_name) { continue; }
                            shown_models.insert(model.full_name.clone());
                            
                            ui.label(&model.full_name);
                        }
                    });
                }
            });
        });
}

fn render_tts_settings_modal(
    ui: &mut egui::Ui,
    config: &mut Config,
    text: &LocaleText,
    show_modal: &mut bool,
) -> bool {
    if !*show_modal {
        return false;
    }
    
    let mut changed = false;

    // List of voices (Name, Gender)
    const VOICES: &[(&str, &str)] = &[
        ("Achernar", "Female"), ("Achird", "Male"), ("Algenib", "Male"), ("Algieba", "Male"), 
        ("Alnilam", "Male"), ("Aoede", "Female"), ("Autonoe", "Female"), ("Callirrhoe", "Female"), 
        ("Charon", "Male"), ("Despina", "Female"), ("Enceladus", "Male"), ("Erinome", "Female"), 
        ("Fenrir", "Male"), ("Gacrux", "Female"), ("Iapetus", "Male"), ("Kore", "Female"), 
        ("Laomedeia", "Female"), ("Leda", "Female"), ("Orus", "Male"), ("Pulcherrima", "Female"), 
        ("Puck", "Male"), ("Rasalgethi", "Male"), ("Sadachbia", "Male"), ("Sadaltager", "Male"), 
        ("Schedar", "Male"), ("Sulafat", "Female"), ("Umbriel", "Male"), ("Vindemiatrix", "Female"), 
        ("Zephyr", "Female"), ("Zubenelgenubi", "Male"),
    ];

    let male_voices: Vec<_> = VOICES.iter().filter(|(_, g)| *g == "Male").collect();
    let female_voices: Vec<_> = VOICES.iter().filter(|(_, g)| *g == "Female").collect();

    egui::Window::new(format!("🔊 {}", text.tts_settings_title))
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .default_width(650.0)
        .default_height(600.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ui.ctx(), |ui| {
            ui.set_min_height(500.0); // Force minimum height for the content area

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("🔊 {}", text.tts_settings_title)).strong().size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close).clicked() {
                        *show_modal = false;
                    }
                });
            });
            ui.separator();
            ui.add_space(8.0);
            
            // Speed and Tone & Style side by side
            ui.columns(2, |columns| {
                // Left column: Speed
                columns[0].label(egui::RichText::new(text.tts_speed_label).strong());
                columns[0].horizontal(|ui| {
                    if ui.radio_value(&mut config.tts_speed, "Slow".to_string(), text.tts_speed_slow).clicked() { changed = true; }
                    if ui.radio_value(&mut config.tts_speed, "Normal".to_string(), text.tts_speed_normal).clicked() { changed = true; }
                    if ui.radio_value(&mut config.tts_speed, "Fast".to_string(), text.tts_speed_fast).clicked() { changed = true; }
                });
                
                // Right column: Language-Specific Instructions
                columns[1].label(egui::RichText::new(text.tts_instructions_label).strong());
                
                // Supported languages from whatlang (70 languages) with ISO 639-3 codes
                let supported_languages = [
                    ("afr", "Afrikaans"), ("ara", "Arabic"), ("aze", "Azerbaijani"),
                    ("bel", "Belarusian"), ("ben", "Bengali"), ("bul", "Bulgarian"),
                    ("cat", "Catalan"), ("ces", "Czech"), ("cmn", "Mandarin Chinese"),
                    ("dan", "Danish"), ("deu", "German"), ("ell", "Greek"),
                    ("eng", "English"), ("epo", "Esperanto"), ("est", "Estonian"),
                    ("eus", "Basque"), ("fin", "Finnish"), ("fra", "French"),
                    ("guj", "Gujarati"), ("heb", "Hebrew"), ("hin", "Hindi"),
                    ("hrv", "Croatian"), ("hun", "Hungarian"), ("ind", "Indonesian"),
                    ("ita", "Italian"), ("jpn", "Japanese"), ("kan", "Kannada"),
                    ("kat", "Georgian"), ("kor", "Korean"), ("lat", "Latin"),
                    ("lav", "Latvian"), ("lit", "Lithuanian"), ("mal", "Malayalam"),
                    ("mar", "Marathi"), ("mkd", "Macedonian"), ("mya", "Burmese"),
                    ("nep", "Nepali"), ("nld", "Dutch"), ("nno", "Norwegian Nynorsk"),
                    ("nob", "Norwegian Bokmål"), ("ori", "Oriya"), ("pan", "Punjabi"),
                    ("pes", "Persian"), ("pol", "Polish"), ("por", "Portuguese"),
                    ("ron", "Romanian"), ("rus", "Russian"), ("sin", "Sinhala"),
                    ("slk", "Slovak"), ("slv", "Slovenian"), ("som", "Somali"),
                    ("spa", "Spanish"), ("sqi", "Albanian"), ("srp", "Serbian"),
                    ("swe", "Swedish"), ("tam", "Tamil"), ("tel", "Telugu"),
                    ("tgl", "Tagalog"), ("tha", "Thai"), ("tur", "Turkish"),
                    ("ukr", "Ukrainian"), ("urd", "Urdu"), ("uzb", "Uzbek"),
                    ("vie", "Vietnamese"), ("yid", "Yiddish"), ("zho", "Chinese"),
                ];
                
                // Show existing conditions
                let mut to_remove: Option<usize> = None;
                for (idx, condition) in config.tts_language_conditions.iter_mut().enumerate() {
                    columns[1].horizontal(|ui| {
                        // Language dropdown (read-only display for now)
                        let display_name = supported_languages.iter()
                            .find(|(code, _)| code.eq_ignore_ascii_case(&condition.language_code))
                            .map(|(_, name)| *name)
                            .unwrap_or(&condition.language_name);
                        
                        ui.label(egui::RichText::new(display_name).strong().color(egui::Color32::from_rgb(100, 180, 100)));
                        ui.label("→");
                        
                        // Instruction input
                        if ui.add(
                            egui::TextEdit::singleline(&mut condition.instruction)
                                .desired_width(180.0)
                                .hint_text(text.tts_instructions_hint)
                        ).changed() {
                            changed = true;
                        }
                        
                        // Remove button - use Icon::Close for proper rendering
                        if icon_button(ui, Icon::Close).on_hover_text("Remove").clicked() {
                            to_remove = Some(idx);
                        }
                    });
                }
                
                // Remove condition if needed
                if let Some(idx) = to_remove {
                    config.tts_language_conditions.remove(idx);
                    changed = true;
                }
                
                // Add condition dropdown - selecting immediately adds the condition
                columns[1].horizontal(|ui| {
                    // Get languages that are not yet used
                    let used_codes: Vec<_> = config.tts_language_conditions.iter()
                        .map(|c| c.language_code.as_str())
                        .collect();
                    let available: Vec<_> = supported_languages.iter()
                        .filter(|(code, _)| !used_codes.contains(code))
                        .collect();
                    
                    if !available.is_empty() {
                        // Dropdown that immediately adds selected language
                        egui::ComboBox::from_id_salt("tts_add_condition")
                            .selected_text(text.tts_add_condition)
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (code, name) in &available {
                                    if ui.selectable_label(false, *name).clicked() {
                                        config.tts_language_conditions.push(crate::config::TtsLanguageCondition {
                                            language_code: code.to_string(),
                                            language_name: name.to_string(),
                                            instruction: String::new(),
                                        });
                                        changed = true;
                                    }
                                }
                            });
                    }
                });
            });
            
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
            
            // Voice selection - 4 columns layout to save vertical space
            ui.columns(4, |columns| {
                use std::sync::atomic::{AtomicUsize, Ordering};
                use std::time::{SystemTime, UNIX_EPOCH};
                use std::collections::hash_map::RandomState;
                use std::hash::{BuildHasher, Hasher};
                
                // Shared static to ensure randomness across all columns and no repeats globally
                static LAST_PREVIEW_IDX: AtomicUsize = AtomicUsize::new(9999);
                
                // Helper to render a voice item
                let render_voice = |ui: &mut egui::Ui, name: &str, config: &mut Config, text: &LocaleText, changed: &mut bool| {
                    ui.horizontal(|ui| {
                        let is_selected = config.tts_voice == name;
                        if ui.radio(is_selected, "").clicked() {
                            config.tts_voice = name.to_string();
                            *changed = true;
                        }
                        if ui.button("🔊").on_hover_text("Preview").clicked() {
                            config.tts_voice = name.to_string();
                            *changed = true;
                            
                            if !text.tts_preview_texts.is_empty() {
                                let s = RandomState::new();
                                let mut hasher = s.build_hasher();
                                hasher.write_usize(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as usize);
                                let rand_val = hasher.finish();
                                let len = text.tts_preview_texts.len();
                                let mut idx = (rand_val as usize) % len;
                                
                                let last = LAST_PREVIEW_IDX.load(Ordering::Relaxed);
                                if idx == last {
                                    idx = (idx + 1) % len;
                                }
                                LAST_PREVIEW_IDX.store(idx, Ordering::Relaxed);
                                
                                let preview_text = text.tts_preview_texts[idx].replace("{}", name);
                                crate::api::tts::TTS_MANAGER.speak_interrupt(&preview_text, 0);
                            } else {
                                let preview_text = format!("Hello, I am {}. This is a voice preview.", name);
                                crate::api::tts::TTS_MANAGER.speak_interrupt(&preview_text, 0);
                            }
                        }
                        ui.label(egui::RichText::new(name).strong());
                    });
                };

                // Split male voices into 2 columns
                let male_mid = (male_voices.len() + 1) / 2;
                let male_col1: Vec<_> = male_voices.iter().take(male_mid).collect();
                let male_col2: Vec<_> = male_voices.iter().skip(male_mid).collect();
                
                // Split female voices into 2 columns
                let female_mid = (female_voices.len() + 1) / 2;
                let female_col1: Vec<_> = female_voices.iter().take(female_mid).collect();
                let female_col2: Vec<_> = female_voices.iter().skip(female_mid).collect();

                // Column 0: Male (first half)
                columns[0].vertical(|ui| {
                    ui.label(egui::RichText::new(text.tts_male).strong().underline());
                    ui.add_space(4.0);
                    for (name, _) in male_col1 {
                        render_voice(ui, name, config, text, &mut changed);
                    }
                });
                
                // Column 1: Male (second half)
                columns[1].vertical(|ui| {
                    ui.label(egui::RichText::new("").strong()); // Empty header for alignment
                    ui.add_space(4.0);
                    for (name, _) in male_col2 {
                        render_voice(ui, name, config, text, &mut changed);
                    }
                });
                
                // Column 2: Female (first half)
                columns[2].vertical(|ui| {
                    ui.label(egui::RichText::new(text.tts_female).strong().underline());
                    ui.add_space(4.0);
                    for (name, _) in female_col1 {
                        render_voice(ui, name, config, text, &mut changed);
                    }
                });
                
                // Column 3: Female (second half)
                columns[3].vertical(|ui| {
                    ui.label(egui::RichText::new("").strong()); // Empty header for alignment
                    ui.add_space(4.0);
                    for (name, _) in female_col2 {
                        render_voice(ui, name, config, text, &mut changed);
                    }
                });
            });
        });
        
    changed
}

fn render_update_section_content(ui: &mut egui::Ui, updater: &Option<Updater>, status: &UpdateStatus, text: &LocaleText) {
    match status {
        UpdateStatus::Idle => {
            ui.horizontal(|ui| {
                ui.label(format!("{} v{}", text.current_version_label, env!("CARGO_PKG_VERSION")));
                if ui.button(text.check_for_updates_btn).clicked() {
                    if let Some(u) = updater { u.check_for_updates(); }
                }
            });
        },
        UpdateStatus::Checking => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text.checking_github);
            });
        },
        UpdateStatus::UpToDate(ver) => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{} (v{})", text.up_to_date, ver)).color(egui::Color32::from_rgb(34, 139, 34)));
                if ui.button(text.check_again_btn).clicked() {
                    if let Some(u) = updater { u.check_for_updates(); }
                }
            });
        },
        UpdateStatus::UpdateAvailable { version, body } => {
            ui.colored_label(egui::Color32::YELLOW, format!("{} {}", text.new_version_available, version));
            ui.collapsing(text.release_notes_label, |ui| {
                ui.label(body);
            });
            ui.add_space(5.0);
            if ui.button(egui::RichText::new(text.download_update_btn).strong()).clicked() {
                if let Some(u) = updater { u.perform_update(); }
            }
        },
        UpdateStatus::Downloading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text.downloading_update);
            });
        },
        UpdateStatus::Error(e) => {
            ui.colored_label(egui::Color32::RED, format!("{} {}", text.update_failed, e));
            ui.label(egui::RichText::new(text.app_folder_writable_hint).size(11.0));
            if ui.button(text.retry_btn).clicked() {
                if let Some(u) = updater { u.check_for_updates(); }
            }
        },
        UpdateStatus::UpdatedAndRestartRequired => {
            ui.label(egui::RichText::new(text.update_success).color(egui::Color32::GREEN).heading());
            ui.label(text.restart_to_use_new_version);
            if ui.button(text.restart_app_btn).clicked() {
                if let Ok(exe_path) = std::env::current_exe() {
                    if let Some(exe_dir) = exe_path.parent() {
                        if let Ok(entries) = std::fs::read_dir(exe_dir) {
                            if let Some(newest_exe) = entries.filter_map(|e| e.ok()).filter(|e| {
                                    let name = e.file_name();
                                    let name_str = name.to_string_lossy();
                                    name_str.starts_with("ScreenGoatedToolbox_v") && name_str.ends_with(".exe")
                                }).max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                            {
                                let _ = std::process::Command::new(newest_exe.path()).spawn();
                            }
                        }
                    }
                    std::process::exit(0);
                }
            }
        }
    }
}
