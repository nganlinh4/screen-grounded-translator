use eframe::egui;
use crate::config::Config;
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button, draw_icon_static};
use crate::model_config::get_all_models;
use crate::updater::{Updater, UpdateStatus};
use std::collections::HashMap;
use auto_launch::AutoLaunch;

pub fn render_global_settings(
    ui: &mut egui::Ui,
    config: &mut Config,
    show_api_key: &mut bool,
    show_gemini_api_key: &mut bool,
    usage_stats: &HashMap<String, String>,
    updater: &Option<Updater>,
    update_status: &UpdateStatus,
    run_at_startup: &mut bool,
    auto_launcher: &Option<AutoLaunch>,
    current_admin_state: bool, 
    text: &LocaleText,
) -> bool {
    let mut changed = false;

    ui.add_space(10.0);
    
    // API Keys
    ui.group(|ui| {
        ui.label(egui::RichText::new(text.api_section).strong());
        ui.horizontal(|ui| {
            ui.label(text.api_key_label);
            if ui.link(text.get_key_link).clicked() { let _ = open::that("https://console.groq.com/keys"); }
        });
        ui.horizontal(|ui| {
            if ui.add(egui::TextEdit::singleline(&mut config.api_key).password(!*show_api_key).desired_width(320.0)).changed() {
                changed = true;
            }
            let eye_icon = if *show_api_key { Icon::EyeOpen } else { Icon::EyeClosed };
            if icon_button(ui, eye_icon).clicked() { *show_api_key = !*show_api_key; }
        });
        
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label(text.gemini_api_key_label);
            if ui.link(text.gemini_get_key_link).clicked() { let _ = open::that("https://aistudio.google.com/app/apikey"); }
        });
        ui.horizontal(|ui| {
            if ui.add(egui::TextEdit::singleline(&mut config.gemini_api_key).password(!*show_gemini_api_key).desired_width(320.0)).changed() {
                changed = true;
            }
            let eye_icon = if *show_gemini_api_key { Icon::EyeOpen } else { Icon::EyeClosed };
            if icon_button(ui, eye_icon).clicked() { *show_gemini_api_key = !*show_gemini_api_key; }
        });
    });

    ui.add_space(10.0);
    
    // Usage Statistics
    render_usage_statistics(ui, usage_stats, text, &config.ui_language);

    ui.add_space(10.0);

    // Software Update
    render_update_section(ui, updater, update_status, text);

    ui.add_space(10.0);

    // --- NEW LOGIC: Startup and Start in Tray with Admin Support ---
    ui.vertical(|ui| {
        // Main startup toggle (Normal Registry-based or Admin Task Scheduler)
        ui.horizontal(|ui| {
            if let Some(launcher) = auto_launcher {
                let mut startup_toggle = *run_at_startup;
                if ui.checkbox(&mut startup_toggle, text.startup_label).clicked() {
                    if startup_toggle && !(*run_at_startup) {
                        // Enabling startup
                        if config.run_as_admin_on_startup && current_admin_state {
                            // Try to enable Admin startup via Task Scheduler
                            if crate::gui::utils::set_admin_startup(true) {
                                // Success: Task created, disable registry to avoid double run
                                let _ = launcher.disable();
                                *run_at_startup = true;
                                changed = true;
                            }
                        } else {
                            // Enable normal registry-based startup
                            // OPTIMIZATION: Run cleanup in background thread to prevent UI lag
                            std::thread::spawn(|| {
                                crate::gui::utils::set_admin_startup(false);
                            });
                            
                            let _ = launcher.enable();
                            *run_at_startup = true;
                            changed = true;
                        }
                    } else if !startup_toggle && *run_at_startup {
                        // Disabling startup
                        // OPTIMIZATION: Run cleanup in background thread to prevent UI lag
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

        // Admin Mode Sub-option (nested, only if startup is enabled)
        if *run_at_startup {
            ui.indent("admin_indent", |ui| {
                let mut is_admin_mode = config.run_as_admin_on_startup;
                let checkbox_label = text.admin_startup_on;
                
                if current_admin_state {
                    // User IS Admin: Allow toggle
                    if ui.checkbox(&mut is_admin_mode, checkbox_label).clicked() {
                        if is_admin_mode && !config.run_as_admin_on_startup {
                            // User trying to enable Admin Mode
                            if crate::gui::utils::set_admin_startup(true) {
                                config.run_as_admin_on_startup = true;
                                // Disable registry-based run to avoid double startup
                                if let Some(launcher) = auto_launcher {
                                    let _ = launcher.disable();
                                }
                                changed = true;
                            } else {
                                // Failed to create task, revert the checkbox
                                is_admin_mode = false;
                            }
                        } else if !is_admin_mode && config.run_as_admin_on_startup {
                            // User disabling Admin Mode (revert to normal startup)
                            // OPTIMIZATION: Run cleanup in background thread to prevent UI lag
                            std::thread::spawn(|| {
                                crate::gui::utils::set_admin_startup(false);
                            });
                            
                            config.run_as_admin_on_startup = false;
                            // Re-enable registry-based startup
                            if let Some(launcher) = auto_launcher {
                                let _ = launcher.enable();
                            }
                            changed = true;
                        }
                    }
                } else {
                    // User is NOT Admin: Disable checkbox and warn
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

                // Status message below the checkbox (Success only)
                // FIX: Only show success message if running as Admin to avoid conflict with the error message above
                if config.run_as_admin_on_startup && current_admin_state {
                    ui.label(
                        egui::RichText::new(text.admin_startup_success)
                            .size(11.0)
                            .color(egui::Color32::from_rgb(34, 139, 34))
                    );
                }
            });

            // Start in Tray checkbox (always shown if startup is enabled)
            if ui.checkbox(&mut config.start_in_tray, text.start_in_tray_label).clicked() {
                changed = true;
            }
        }
    });

    ui.add_space(5.0);

    if ui.button(text.reset_defaults_btn).clicked() {
        let saved_groq_key = config.api_key.clone();
        let saved_gemini_key = config.gemini_api_key.clone();
        let saved_language = config.ui_language.clone();
        
        *config = Config::default();
        
        config.api_key = saved_groq_key;
        config.gemini_api_key = saved_gemini_key;
        config.ui_language = saved_language;
        changed = true;
    }

    changed
}

fn render_usage_statistics(
    ui: &mut egui::Ui, 
    usage_stats: &HashMap<String, String>, 
    text: &LocaleText,
    _lang_code: &str
) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            draw_icon_static(ui, Icon::Statistics, None);
            ui.label(egui::RichText::new(text.usage_statistics_title).strong());
            icon_button(ui, Icon::Info).on_hover_text(text.usage_statistics_tooltip);
        });
        
        egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
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

fn render_update_section(ui: &mut egui::Ui, updater: &Option<Updater>, status: &UpdateStatus, text: &LocaleText) {
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
