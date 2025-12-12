use eframe::egui;
use crate::config::{Config, Preset, ThemeMode};
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button, draw_icon_static, icon_button_sized};
use super::ViewMode;

pub fn render_sidebar(
    ui: &mut egui::Ui,
    config: &mut Config,
    view_mode: &mut ViewMode,
    text: &LocaleText,
) -> bool {
    let mut changed = false;

    // Header Grid Layout (matching presets section)
    egui::Grid::new("header_grid")
        .striped(false)
        .spacing(egui::vec2(10.0, 6.0))
        .show(ui, |ui| {
            // Column 1: Theme + Language + History (all in one horizontal row)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                
                // --- THEME SWITCHER LOGIC ---
                let (theme_icon, tooltip) = match config.theme_mode {
                    ThemeMode::Dark => (Icon::Moon, "Theme: Dark"),
                    ThemeMode::Light => (Icon::Sun, "Theme: Light"),
                    ThemeMode::System => (Icon::SystemTheme, "Theme: System (Auto)"),
                };

                if icon_button(ui, theme_icon).on_hover_text(tooltip).clicked() {
                    // Cycle: System -> Dark -> Light -> System
                    config.theme_mode = match config.theme_mode {
                        ThemeMode::System => ThemeMode::Dark,
                        ThemeMode::Dark => ThemeMode::Light,
                        ThemeMode::Light => ThemeMode::System,
                    };
                    changed = true;
                }
                
                let original_lang = config.ui_language.clone();
                let lang_display = match config.ui_language.as_str() {
                    "vi" => "VI",
                    "ko" => "KO",
                    _ => "EN",
                };
                egui::ComboBox::from_id_source("header_lang_switch")
                    .width(60.0)
                    .selected_text(lang_display)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.ui_language, "en".to_string(), "English");
                        ui.selectable_value(&mut config.ui_language, "vi".to_string(), "Vietnamese");
                        ui.selectable_value(&mut config.ui_language, "ko".to_string(), "Korean");
                    });
                if original_lang != config.ui_language {
                    changed = true;
                }
                
                // History Button
                if ui.button(text.history_btn).clicked() {
                    *view_mode = ViewMode::History;
                }
            });

            // Column 2: Global Settings (Right Aligned in Column)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.style_mut().spacing.item_spacing.x = 4.0;
                let is_global = matches!(view_mode, ViewMode::Global);
                if ui.selectable_label(is_global, text.global_settings).clicked() {
                    *view_mode = ViewMode::Global;
                }
                draw_icon_static(ui, Icon::Settings, None);
            });
            ui.end_row();
        });
    
    ui.add_space(10.0);
    
    let mut preset_idx_to_delete = None;
    let mut preset_to_add_type = None;

    // Split indices
    let mut img_indices = Vec::new();
    let mut other_indices = Vec::new();

    for (i, p) in config.presets.iter().enumerate() {
        if p.preset_type == "image" {
            img_indices.push(i);
        } else {
            other_indices.push(i);
        }
    }
    
    // Sort other indices: Text -> Audio -> Video -> Other
    other_indices.sort_by_key(|&i| {
        match config.presets[i].preset_type.as_str() {
            "text" => 0,
            "audio" => 1,
            "video" => 2,
            _ => 3,
        }
    });

    // Helper closure to render a preset row content
    let mut render_item = |ui: &mut egui::Ui, idx: usize| {
        let preset = &config.presets[idx];
        
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            
            let is_selected = matches!(view_mode, ViewMode::Preset(i) if *i == idx);
            
            let icon_type = match preset.preset_type.as_str() {
                "audio" => Icon::Microphone,
                "video" => Icon::Video,
                "text" => Icon::Text,
                _ => Icon::Image,
            };
            
            if preset.is_upcoming {
                ui.add_enabled_ui(false, |ui| {
                    draw_icon_static(ui, icon_type, Some(14.0));
                    let _ = ui.selectable_label(is_selected, &preset.name);
                });
            } else {
                draw_icon_static(ui, icon_type, Some(14.0));
                // Use the full name directly without truncation
                if ui.selectable_label(is_selected, &preset.name).clicked() {
                    *view_mode = ViewMode::Preset(idx);
                }
                // Delete button (Small X icon)
                if config.presets.len() > 1 {
                    if icon_button_sized(ui, Icon::Delete, 14.0).clicked() {
                        preset_idx_to_delete = Some(idx);
                    }
                }
            }
        });
    };

    // Dynamic Grid Layout
    egui::Grid::new("presets_grid")
        .striped(false)
        .spacing(egui::vec2(10.0, 6.0)) // Horizontal spacing allows visual separation
        .show(ui, |ui| {
            // --- HEADER ROW (Integrated into Grid) ---
            // Column 1: Title
            ui.label(egui::RichText::new(text.presets_section).strong());

            // Column 2: Buttons (Right Aligned in Column)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.style_mut().spacing.item_spacing.x = 4.0;
                // Add buttons in reverse order because of right_to_left layout
                if ui.button(text.add_audio_preset_btn).clicked() {
                    preset_to_add_type = Some("audio");
                }
                if ui.button(text.add_image_preset_btn).clicked() {
                    preset_to_add_type = Some("image");
                }
                if ui.button(text.add_text_preset_btn).clicked() {
                    preset_to_add_type = Some("text");
                }
            });
            ui.end_row();

            // --- DATA ROWS ---
            let max_len = std::cmp::max(img_indices.len(), other_indices.len());

            for i in 0..max_len {
                // Column 1: Image Presets
                if let Some(&idx) = img_indices.get(i) {
                    render_item(ui, idx);
                } else {
                    ui.label(""); // Empty placeholder to maintain grid structure
                }

                // Column 2: Text/Audio/Other Presets
                if let Some(&idx) = other_indices.get(i) {
                    render_item(ui, idx);
                } else {
                    ui.label(""); 
                }
                
                ui.end_row();
            }
        });

    // Handle Add
    if let Some(type_str) = preset_to_add_type {
        let mut new_preset = Preset::default();
        if type_str == "text" {
            new_preset.preset_type = "text".to_string();
            new_preset.name = format!("Text {}", config.presets.len() + 1);
            new_preset.model = "text_accurate_kimi".to_string();
            new_preset.text_input_mode = "select".to_string();
            new_preset.prompt = "Translate this text.".to_string();
            new_preset.audio_source = "".to_string();
        } else if type_str == "audio" {
            new_preset.preset_type = "audio".to_string();
            new_preset.name = format!("Audio {}", config.presets.len() + 1);
            new_preset.model = "whisper-fast".to_string();
            new_preset.audio_source = "mic".to_string();
        } else {
            new_preset.name = format!("Image {}", config.presets.len() + 1);
            // Default preset is already Image type
        }
        
        config.presets.push(new_preset);
        *view_mode = ViewMode::Preset(config.presets.len() - 1);
        changed = true;
    }

    // Handle Delete
    if let Some(idx) = preset_idx_to_delete {
        config.presets.remove(idx);
        if let ViewMode::Preset(curr) = *view_mode {
            if curr >= idx && curr > 0 {
                *view_mode = ViewMode::Preset(curr - 1);
            } else if config.presets.is_empty() {
                *view_mode = ViewMode::Global;
            } else {
                *view_mode = ViewMode::Preset(0);
            }
        }
        changed = true;
    }

    changed
}
