use super::node_graph::request_node_graph_view_reset;
use crate::config::Config;
use crate::gui::icons::{icon_button, Icon};
use crate::gui::locale::LocaleText;
use crate::updater::{UpdateStatus, Updater};
use auto_launch::AutoLaunch;
use eframe::egui;
use std::collections::HashMap;

mod downloaded_tools;
mod tts_settings;
mod update_section;
mod usage_stats;

use crate::gui::settings_ui::download_manager::DownloadManager;
use downloaded_tools::render_downloaded_tools_modal;
use tts_settings::render_tts_settings_modal;
use update_section::render_update_section_content;
use usage_stats::render_usage_modal;

const API_KEY_FIELD_WIDTH: f32 = 400.0;

pub fn render_global_settings(
    ui: &mut egui::Ui,
    config: &mut Config,
    show_api_key: &mut bool,
    show_gemini_api_key: &mut bool,
    show_openrouter_api_key: &mut bool,
    show_cerebras_api_key: &mut bool,
    usage_stats: &HashMap<String, String>,
    updater: &Option<Updater>,
    update_status: &UpdateStatus,
    run_at_startup: &mut bool,
    auto_launcher: &Option<AutoLaunch>,
    current_admin_state: bool,
    text: &LocaleText,
    show_usage_modal: &mut bool,

    show_tts_modal: &mut bool,
    show_tools_modal: &mut bool,
    download_manager: &mut DownloadManager,
    _cached_audio_devices: &std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>,
    _recording_sr_hotkey: &mut bool,
) -> bool {
    let mut changed = false;

    let is_dark = ui.visuals().dark_mode;
    let card_bg = if is_dark {
        egui::Color32::from_rgba_unmultiplied(28, 32, 42, 250) // Darker for better text contrast
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
                ui.label(
                    egui::RichText::new(text.api_keys_header)
                        .strong()
                        .size(14.0),
                );
                ui.add_space(16.0);

                if ui
                    .checkbox(&mut config.use_groq, text.use_groq_checkbox)
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut config.use_cerebras, text.use_cerebras_checkbox)
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut config.use_gemini, text.use_gemini_checkbox)
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut config.use_openrouter, text.use_openrouter_checkbox)
                    .changed()
                {
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
                    if ui.link(text.get_key_link).clicked() {
                        let _ = open::that("https://console.groq.com/keys");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut config.api_key)
                                .id(egui::Id::new("settings_api_key_groq"))
                                .password(!*show_api_key)
                                .desired_width(API_KEY_FIELD_WIDTH),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    let eye_icon = if *show_api_key {
                        Icon::EyeOpen
                    } else {
                        Icon::EyeClosed
                    };
                    if icon_button(ui, eye_icon).clicked() {
                        *show_api_key = !*show_api_key;
                    }
                });
            }

            // Cerebras API Key (only show if enabled)
            if config.use_cerebras {
                ui.horizontal(|ui| {
                    ui.label(text.cerebras_api_key_label);
                    if ui.link(text.cerebras_get_key_link).clicked() {
                        let _ = open::that("https://cloud.cerebras.ai/");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut config.cerebras_api_key)
                                .id(egui::Id::new("settings_api_key_cerebras"))
                                .password(!*show_cerebras_api_key)
                                .desired_width(API_KEY_FIELD_WIDTH),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    let eye_icon = if *show_cerebras_api_key {
                        Icon::EyeOpen
                    } else {
                        Icon::EyeClosed
                    };
                    if icon_button(ui, eye_icon).clicked() {
                        *show_cerebras_api_key = !*show_cerebras_api_key;
                    }
                });
            }

            // Gemini API Key (only show if enabled)
            if config.use_gemini {
                ui.horizontal(|ui| {
                    ui.label(text.gemini_api_key_label);
                    if ui.link(text.gemini_get_key_link).clicked() {
                        let _ = open::that("https://aistudio.google.com/app/apikey");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut config.gemini_api_key)
                                .id(egui::Id::new("settings_api_key_gemini"))
                                .password(!*show_gemini_api_key)
                                .desired_width(API_KEY_FIELD_WIDTH),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    let eye_icon = if *show_gemini_api_key {
                        Icon::EyeOpen
                    } else {
                        Icon::EyeClosed
                    };
                    if icon_button(ui, eye_icon).clicked() {
                        *show_gemini_api_key = !*show_gemini_api_key;
                    }
                });
            }

            // OpenRouter API Key (only show if enabled)
            if config.use_openrouter {
                ui.horizontal(|ui| {
                    ui.label(text.openrouter_api_key_label);
                    if ui.link(text.openrouter_get_key_link).clicked() {
                        let _ = open::that("https://openrouter.ai/settings/keys");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut config.openrouter_api_key)
                                .id(egui::Id::new("settings_api_key_openrouter"))
                                .password(!*show_openrouter_api_key)
                                .desired_width(API_KEY_FIELD_WIDTH),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    let eye_icon = if *show_openrouter_api_key {
                        Icon::EyeOpen
                    } else {
                        Icon::EyeClosed
                    };
                    if icon_button(ui, eye_icon).clicked() {
                        *show_openrouter_api_key = !*show_openrouter_api_key;
                    }
                });
            }

            // Ollama (Local AI) - only show URL field if enabled
            if config.use_ollama {
                ui.horizontal(|ui| {
                    ui.label("Ollama URL:");
                    if ui.link(text.ollama_url_guide).clicked() {
                        let _ = open::that("https://docs.ollama.com/api/introduction#base-url");
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut config.ollama_base_url)
                                .id(egui::Id::new("settings_api_key_ollama_url"))
                                .desired_width(API_KEY_FIELD_WIDTH),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    // Show status if available
                    if let Some(status) = ui
                        .ctx()
                        .memory(|mem| mem.data.get_temp::<String>(egui::Id::new("ollama_status")))
                    {
                        ui.label(egui::RichText::new(&status).size(11.0));
                    }
                });
            }
        });

    ui.add_space(10.0);

    // === USAGE STATISTICS & TTS SETTINGS BUTTONS ===
    let is_dark = ui.visuals().dark_mode;
    let stats_bg = if is_dark {
        egui::Color32::from_rgb(50, 100, 110) // Teal for dark mode
    } else {
        egui::Color32::from_rgb(90, 160, 170) // Lighter teal for light mode
    };

    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("📊 {}", text.usage_statistics_title))
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(stats_bg)
                .corner_radius(10.0),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(text.usage_statistics_tooltip)
            .clicked()
        {
            *show_usage_modal = true;
        }

        ui.add_space(10.0);

        let tts_bg = if is_dark {
            egui::Color32::from_rgb(100, 80, 120) // Purple for dark mode
        } else {
            egui::Color32::from_rgb(180, 140, 200) // Lighter purple for light mode
        };

        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("🔊 {}", text.tts_settings_button))
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(tts_bg)
                .corner_radius(10.0),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            *show_tts_modal = true;
        }

        ui.add_space(10.0);

        let tools_bg = if is_dark {
            egui::Color32::from_rgb(60, 90, 140)
        } else {
            egui::Color32::from_rgb(100, 140, 200)
        };

        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("📦 {}", text.downloaded_tools_button))
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(tools_bg)
                .corner_radius(10.0),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            *show_tools_modal = true;
        }
    });

    // === USAGE STATISTICS MODAL ===
    render_usage_modal(
        ui,
        usage_stats,
        text,
        show_usage_modal,
        config.use_groq,
        config.use_gemini,
        config.use_openrouter,
        config.use_ollama,
        config.use_cerebras,
    );

    // === TOOLS MODAL ===
    let ctx = ui.ctx().clone();
    render_downloaded_tools_modal(&ctx, ui, show_tools_modal, download_manager, text);

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
            ui.label(
                egui::RichText::new(text.software_update_header)
                    .strong()
                    .size(14.0),
            );
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
            ui.label(
                egui::RichText::new(text.startup_display_header)
                    .strong()
                    .size(14.0),
            );
            ui.add_space(6.0);

            // Main startup toggle
            ui.horizontal(|ui| {
                if let Some(launcher) = auto_launcher {
                    let mut startup_toggle = *run_at_startup;
                    if ui
                        .checkbox(&mut startup_toggle, text.startup_label)
                        .clicked()
                    {
                        if startup_toggle && !(*run_at_startup) {
                            // User is turning it ON - authorize THIS exe as the one allowed to start
                            if let Ok(exe_path) = std::env::current_exe() {
                                if let Some(exe_str) = exe_path.to_str() {
                                    config.authorized_startup_path = exe_str.to_string();
                                }
                            }

                            if config.run_as_admin_on_startup && current_admin_state {
                                if crate::gui::utils::set_admin_startup(true) {
                                    let _ = launcher.disable();
                                    config.run_at_startup = false;
                                    *run_at_startup = true;
                                    changed = true;
                                }
                            } else {
                                std::thread::spawn(|| {
                                    crate::gui::utils::set_admin_startup(false);
                                });
                                let _ = launcher.enable();
                                config.run_at_startup = true;
                                *run_at_startup = true;
                                changed = true;
                            }
                        } else if !startup_toggle && *run_at_startup {
                            // User is turning it OFF
                            std::thread::spawn(|| {
                                crate::gui::utils::set_admin_startup(false);
                            });
                            let _ = launcher.disable();
                            config.run_as_admin_on_startup = false;
                            config.run_at_startup = false;
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
                                // Transitioning to admin mode requires updated authorization
                                if let Ok(exe_path) = std::env::current_exe() {
                                    if let Some(exe_str) = exe_path.to_str() {
                                        config.authorized_startup_path = exe_str.to_string();
                                    }
                                }

                                if crate::gui::utils::set_admin_startup(true) {
                                    config.run_as_admin_on_startup = true;
                                    config.run_at_startup = false;
                                    if let Some(launcher) = auto_launcher {
                                        let _ = launcher.disable();
                                    }
                                    changed = true;
                                }
                            } else if !is_admin_mode && config.run_as_admin_on_startup {
                                // Reverting to standard mode
                                if let Ok(exe_path) = std::env::current_exe() {
                                    if let Some(exe_str) = exe_path.to_str() {
                                        config.authorized_startup_path = exe_str.to_string();
                                    }
                                }

                                std::thread::spawn(|| {
                                    crate::gui::utils::set_admin_startup(false);
                                });
                                config.run_as_admin_on_startup = false;
                                config.run_at_startup = true;
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
                                .color(egui::Color32::from_rgb(200, 100, 50)),
                        );
                    }

                    if config.run_as_admin_on_startup && current_admin_state {
                        ui.label(
                            egui::RichText::new(text.admin_startup_success)
                                .size(11.0)
                                .color(egui::Color32::from_rgb(34, 139, 34)),
                        );
                    }
                });

                if ui
                    .checkbox(&mut config.start_in_tray, text.start_in_tray_label)
                    .clicked()
                {
                    changed = true;
                }
            }

            ui.add_space(8.0);

            // Graphics Mode + Reset button on same row
            ui.horizontal(|ui| {
                let current_label = match config.ui_language.as_str() {
                    "vi" => {
                        if config.graphics_mode == "minimal" {
                            "Tối giản"
                        } else {
                            "Tiêu chuẩn"
                        }
                    }
                    "ko" => {
                        if config.graphics_mode == "minimal" {
                            "최소"
                        } else {
                            "표준"
                        }
                    }
                    _ => {
                        if config.graphics_mode == "minimal" {
                            "Minimal"
                        } else {
                            "Standard"
                        }
                    }
                };

                egui::ComboBox::from_id_salt("graphics_mode_combo")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                config.graphics_mode == "standard",
                                text.graphics_mode_standard,
                            )
                            .clicked()
                        {
                            config.graphics_mode = "standard".to_string();
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                config.graphics_mode == "minimal",
                                text.graphics_mode_minimal,
                            )
                            .clicked()
                        {
                            config.graphics_mode = "minimal".to_string();
                            changed = true;
                        }
                    });

                // Big gap to simulate right alignment
                ui.add_space(40.0);

                // Force Quit button
                let quite_bg = if is_dark {
                    egui::Color32::from_rgb(120, 40, 40)
                } else {
                    egui::Color32::from_rgb(200, 100, 100)
                };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(text.force_quit).color(egui::Color32::WHITE),
                        )
                        .fill(quite_bg)
                        .corner_radius(8.0),
                    )
                    .clicked()
                {
                    std::process::exit(0);
                }

                ui.add_space(10.0);

                // Reset button
                let reset_bg = if is_dark {
                    egui::Color32::from_rgb(120, 60, 60)
                } else {
                    egui::Color32::from_rgb(220, 140, 140)
                };
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(text.reset_defaults_btn)
                                .color(egui::Color32::WHITE),
                        )
                        .fill(reset_bg)
                        .corner_radius(8.0),
                    )
                    .clicked()
                {
                    let saved_groq_key = config.api_key.clone();
                    let saved_gemini_key = config.gemini_api_key.clone();
                    let saved_openrouter_key = config.openrouter_api_key.clone();
                    let saved_cerebras_key = config.cerebras_api_key.clone();
                    let saved_language = config.ui_language.clone();
                    let saved_use_groq = config.use_groq;
                    let saved_use_gemini = config.use_gemini;
                    let saved_use_openrouter = config.use_openrouter;
                    let saved_use_ollama = config.use_ollama;
                    let saved_use_cerebras = config.use_cerebras;
                    let saved_ollama_base_url = config.ollama_base_url.clone();
                    // Realtime model reset to default (google-gemma)

                    *config = Config::default();

                    config.api_key = saved_groq_key;
                    config.gemini_api_key = saved_gemini_key;
                    config.openrouter_api_key = saved_openrouter_key;
                    config.cerebras_api_key = saved_cerebras_key;
                    config.ui_language = saved_language;
                    config.use_groq = saved_use_groq;
                    config.use_gemini = saved_use_gemini;
                    config.use_openrouter = saved_use_openrouter;
                    config.use_ollama = saved_use_ollama;
                    config.use_cerebras = saved_use_cerebras;
                    config.ollama_base_url = saved_ollama_base_url;
                    // config.realtime_translation_model = saved_realtime_model;
                    request_node_graph_view_reset(ui.ctx());

                    // Also clear WebView data (MIDI permissions, etc.)
                    // Instead of throwing an error if locked, we schedule for next startup and restart immediately.
                    config.clear_webview_on_startup = true;

                    // Save immediately and restart
                    crate::config::save_config(config);
                    crate::gui::app::restart_app();

                    changed = true;
                }
            });
        });

    ui.add_space(10.0);

    ui.add_space(10.0);

    changed
}
