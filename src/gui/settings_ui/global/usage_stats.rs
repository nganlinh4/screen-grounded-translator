use eframe::egui;
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button};
use crate::model_config::{get_all_models, get_all_models_with_ollama};
use std::collections::HashMap;

pub fn render_usage_modal(
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
