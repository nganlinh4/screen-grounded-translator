use crate::config::get_all_languages;
use crate::model_config::get_model_by_id;
use eframe::egui;
use std::collections::HashMap;

/// Check if a model supports search capabilities (grounding/web search)
pub fn model_supports_search(model_id: &str) -> bool {
    if let Some(model_config) = get_model_by_id(model_id) {
        // gemma-3-27b-it model doesn't support grounding
        if model_config.full_name.contains("gemma-3-27b-it") {
            return false;
        }
        // Gemini models support search
        if model_id.contains("gemini") || model_id.contains("gemma") {
            return true;
        }
        // Groq compound models support search
        if model_id.contains("compound") {
            return true;
        }
    }
    false
}

/// Request a node graph view reset (scale=1.0, centered)
/// This sets a flag that the patched egui-snarl library will check
pub fn request_node_graph_view_reset(ctx: &egui::Context) {
    let reset_id = egui::Id::new("snarl_reset_view");
    ctx.data_mut(|d| d.insert_temp(reset_id, true));
}

pub fn show_language_vars(
    ui: &mut egui::Ui,
    _ui_language: &str,
    prompt: &str,
    language_vars: &mut HashMap<String, String>,
    changed: &mut bool,
    _search_query: &mut String,
) {
    // Find {languageN} tags in prompt
    let mut detected_vars = Vec::new();
    for k in 1..=10 {
        let tag = format!("{{language{}}}", k);
        if prompt.contains(&tag) {
            detected_vars.push(k);
        }
    }

    for num in detected_vars {
        let key = format!("language{}", num);
        if !language_vars.contains_key(&key) {
            language_vars.insert(key.clone(), "Vietnamese".to_string());
        }

        let label = format!("{{language{}}}:", num);

        ui.horizontal(|ui| {
            ui.label(label);
            let current_val = language_vars.get(&key).cloned().unwrap_or_default();

            // Create unique IDs for this specific language selector

            let search_id = egui::Id::new(format!("lang_search_{}", num));

            // Styled button to open popup
            let is_dark = ui.visuals().dark_mode;
            let lang_var_bg = if is_dark {
                egui::Color32::from_rgb(70, 60, 100)
            } else {
                egui::Color32::from_rgb(150, 140, 180)
            };
            let button_response = ui.add(
                egui::Button::new(egui::RichText::new(&current_val).color(egui::Color32::WHITE))
                    .fill(lang_var_bg)
                    .corner_radius(8.0),
            );

            if button_response.clicked() {
                egui::Popup::toggle_id(ui.ctx(), button_response.id);
            }

            let popup_layer_id = button_response.id;
            egui::Popup::from_toggle_button_response(&button_response).show(|ui| {
                ui.set_min_width(120.0);

                // Get or create search state for this popup from temp data
                let mut search_text: String =
                    ui.data_mut(|d| d.get_temp(search_id).unwrap_or_default());

                // Search box
                let _search_response = ui.add(
                    egui::TextEdit::singleline(&mut search_text)
                        .hint_text("Search...")
                        .desired_width(110.0),
                );

                // Store search state back
                ui.data_mut(|d| d.insert_temp(search_id, search_text.clone()));

                ui.separator();

                // Language list in scroll area
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.set_width(120.0); // Ensure scrollbar stays on the right edge
                        for lang in get_all_languages() {
                            let matches_search = search_text.is_empty()
                                || lang.to_lowercase().contains(&search_text.to_lowercase());
                            if matches_search {
                                let is_selected = current_val == *lang;
                                if ui.selectable_label(is_selected, lang).clicked() {
                                    language_vars.insert(key.clone(), lang.clone());
                                    *changed = true;
                                    // Clear search and close popup
                                    ui.data_mut(|d| {
                                        d.insert_temp::<String>(search_id, String::new())
                                    });
                                    egui::Popup::toggle_id(ui.ctx(), popup_layer_id);
                                }
                            }
                        }
                    });
            });
        });
    }
}

pub fn insert_next_language_tag(prompt: &mut String, language_vars: &mut HashMap<String, String>) {
    let mut max_num = 0;
    for k in 1..=10 {
        if prompt.contains(&format!("{{language{}}}", k)) {
            max_num = k;
        }
    }
    let next_num = max_num + 1;
    let tag = format!(" {{language{}}} ", next_num);
    prompt.push_str(&tag);

    let key = format!("language{}", next_num);
    if !language_vars.contains_key(&key) {
        language_vars.insert(key, "Vietnamese".to_string());
    }
}
