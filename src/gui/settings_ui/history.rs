use eframe::egui;
use crate::config::Config;
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button, draw_icon_static};
use crate::history::{HistoryManager, HistoryItem, HistoryType};

pub fn render_history_panel(
    ui: &mut egui::Ui,
    config: &mut Config,
    history_manager: &HistoryManager,
    search_query: &mut String,
    text: &LocaleText,
) -> bool {
    let mut changed = false;
    
    // FIX: Limit width to 400px for the entire history header UI
    // This prevents the search, slider, and buttons from stretching too wide
    ui.vertical(|ui| {
        ui.set_max_width(400.0);
        
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(text.history_title).heading());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                 // Slider for limit
                 // UPDATED: Slider max 200, Label on Left (Order: Slider first, then Label)
                 if ui.add(egui::Slider::new(&mut config.max_history_items, 10..=200)).changed() {
                      history_manager.request_prune(config.max_history_items);
                      changed = true;
                 }
                 ui.label(text.max_items_label);
            });
        });
        
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(search_query).hint_text(text.search_placeholder).desired_width(250.0));
            
            // UPDATED: 'X' button appears only if search has text
            if !search_query.is_empty() {
                if icon_button(ui, Icon::Close).on_hover_text("Clear search").clicked() {
                    *search_query = "".to_string(); 
                }
            }
            
            if icon_button(ui, Icon::Folder).on_hover_text("Open Media Folder").clicked() {
                let config_dir = dirs::config_dir().unwrap_or_default().join("screen-goated-toolbox").join("history_media");
                let _ = std::fs::create_dir_all(&config_dir);
                let _ = open::that(config_dir);
            }

            // NEW: "Clear All" button (Text instead of Icon)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                 if ui.button(text.clear_all_history_btn).clicked() {
                      history_manager.clear_all();
                 }
            });
        });
        ui.separator();
    });
    
    let items = history_manager.items.lock().unwrap().clone();
    let q = search_query.to_lowercase();
    let filtered: Vec<&HistoryItem> = items.iter().filter(|i| {
        q.is_empty() || i.text.to_lowercase().contains(&q) || i.timestamp.contains(&q)
    }).collect();

    if filtered.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(text.history_empty);
        });
    } else {
        // FIX: Use Frame with fixed height to ensure scroll area stays at 700px
        egui::Frame::none().show(ui, |ui| {
            ui.set_height(370.0);
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                // FIX: Limit width to 400px to prevent items from stretching too wide
                // This improves readability on large monitors while maintaining the layout
                ui.set_max_width(400.0);
                
                let mut id_to_delete = None;
                
                for item in filtered {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let icon = match item.item_type {
                                HistoryType::Image => Icon::Image,
                                HistoryType::Audio => Icon::Microphone,
                            };
                            draw_icon_static(ui, icon, Some(14.0));
                            ui.label(egui::RichText::new(&item.timestamp).size(10.0).weak());
                            
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Delete Button - Uses larger, centered trash can for history
                                if icon_button(ui, Icon::DeleteLarge).on_hover_text("Delete").clicked() {
                                    id_to_delete = Some(item.id);
                                }
                                
                                // Copy Text Button (Small Icon) - Standard size
                                if icon_button(ui, Icon::Copy).on_hover_text("Copy Text").clicked() {
                                    crate::gui::utils::copy_to_clipboard_text(&item.text);
                                }

                                // View Media Button - Text Button
                                let btn_text = match item.item_type {
                                    HistoryType::Image => text.view_image_btn,
                                    HistoryType::Audio => text.listen_audio_btn,
                                };
                                if ui.button(btn_text).clicked() {
                                    let config_dir = dirs::config_dir().unwrap().join("screen-goated-toolbox").join("history_media");
                                    let path = config_dir.join(&item.media_path);
                                    let _ = open::that(path);
                                }
                            });
                        });
                        
                        ui.label(egui::RichText::new(&item.text).size(13.0));
                    });
                    ui.add_space(4.0);
                }
                
                if let Some(id) = id_to_delete {
                    history_manager.delete(id);
                }
            });
        });
    }
    
    changed
}
