use eframe::egui;
use crate::config::Config;
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button};
use crate::model_config::get_all_models;
use crate::updater::{Updater, UpdateStatus};
use std::collections::HashMap;
use auto_launch::AutoLaunch;
use super::node_graph::request_node_graph_view_reset;

const API_KEY_FIELD_WIDTH: f32 = 340.0;

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
            }
        });

    ui.add_space(10.0);
    
    // === USAGE STATISTICS CARD ===
    render_usage_statistics(ui, usage_stats, text, &config.ui_language, card_bg, card_stroke);

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
                    let saved_realtime_model = config.realtime_translation_model.clone();
                    
                    *config = Config::default();
                    
                    config.api_key = saved_groq_key;
                    config.gemini_api_key = saved_gemini_key;
                    config.openrouter_api_key = saved_openrouter_key;
                    config.ui_language = saved_language;
                    config.use_groq = saved_use_groq;
                    config.use_gemini = saved_use_gemini;
                    config.use_openrouter = saved_use_openrouter;
                    config.realtime_translation_model = saved_realtime_model;
                    request_node_graph_view_reset(ui.ctx());
                    changed = true;
                }
            });
            
            ui.add_space(4.0);
            
            // Realtime Translation Model selector
            ui.horizontal(|ui| {
                let label = match config.ui_language.as_str() {
                    "vi" => "Model dịch realtime:",
                    "ko" => "실시간 번역 모델:",
                    _ => "Realtime translation model:",
                };
                ui.label(label);
                
                let current_model_label = match config.realtime_translation_model.as_str() {
                    "google-gemma" => "Google Gemma 3 27B",
                    _ => "Groq Llama 3.1 8B",
                };
                
                egui::ComboBox::from_id_salt("realtime_translation_model_combo")
                    .selected_text(current_model_label)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(config.realtime_translation_model == "groq-llama", "Groq Llama 3.1 8B (Fast)").clicked() {
                            config.realtime_translation_model = "groq-llama".to_string();
                            changed = true;
                        }
                        if ui.selectable_label(config.realtime_translation_model == "google-gemma", "Google Gemma 3 27B (Accurate)").clicked() {
                            config.realtime_translation_model = "google-gemma".to_string();
                            changed = true;
                        }
                    });
            });
        });

    changed
}

fn render_usage_statistics(
    ui: &mut egui::Ui, 
    usage_stats: &HashMap<String, String>, 
    text: &LocaleText,
    _lang_code: &str,
    card_bg: egui::Color32,
    card_stroke: egui::Stroke,
) {
    egui::Frame::new()
        .fill(card_bg)
        .stroke(card_stroke)
        .inner_margin(12.0)
        .corner_radius(10.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("📊 {}", text.usage_statistics_title)).strong().size(14.0));
                icon_button(ui, Icon::Info).on_hover_text(text.usage_statistics_tooltip);
            });
            ui.add_space(6.0);
            
            egui::ScrollArea::vertical().max_height(70.0).show(ui, |ui| {
                egui::Grid::new("usage_grid").striped(true).show(ui, |ui| {
                    ui.label(egui::RichText::new(text.usage_model_column).strong());
                    ui.label(egui::RichText::new(text.usage_remaining_column).strong());
                    ui.end_row();

                    let mut shown_models = std::collections::HashSet::new();
                    
                    for model in get_all_models() {
                        if !model.enabled { continue; }
                        
                        if shown_models.contains(&model.full_name) { continue; }
                        shown_models.insert(model.full_name.clone());
                        
                        ui.label(model.full_name.clone());
                        
                        if model.provider == "groq" {
                            let status = usage_stats.get(&model.full_name).cloned().unwrap_or_else(|| "??? / ?".to_string());
                            ui.label(status);
                        } else if model.provider == "google" {
                            ui.hyperlink_to(text.usage_check_link, "https://aistudio.google.com/usage?timeRange=last-1-day&tab=rate-limit");
                        }
                        ui.end_row();
                    }
                });
            });
        });
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
