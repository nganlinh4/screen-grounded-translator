use eframe::egui;
use crate::config::{Config, get_all_languages, ProcessingBlock};
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button};
use crate::model_config::{get_all_models, ModelType, get_model_by_id};

pub fn render_preset_editor(
    ui: &mut egui::Ui,
    config: &mut Config,
    preset_idx: usize,
    search_query: &mut String,
    _cached_monitors: &mut Vec<String>,
    recording_hotkey_for_preset: &mut Option<usize>,
    hotkey_conflict_msg: &Option<String>,
    text: &LocaleText,
) -> bool {
    if preset_idx >= config.presets.len() { return false; }

    let mut preset = config.presets[preset_idx].clone();
    let mut changed = false;

    // --- HEADER: Name & Main Type ---
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text.preset_name_label).heading());
        if ui.add(egui::TextEdit::singleline(&mut preset.name).font(egui::TextStyle::Heading)).changed() {
            changed = true;
        }
    });

    // Preset Type Selector (Changes the type of the FIRST block)
    ui.horizontal(|ui| {
        ui.label(text.preset_type_label);
        let selected_text = match preset.preset_type.as_str() {
            "audio" => text.preset_type_audio,
            "video" => text.preset_type_video,
            "text" => text.preset_type_text,
            _ => text.preset_type_image,
        };
        
        egui::ComboBox::from_id_source("preset_type_combo")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if ui.selectable_value(&mut preset.preset_type, "image".to_string(), text.preset_type_image).clicked() {
                    // Update first block type
                    if let Some(first) = preset.blocks.first_mut() {
                        first.block_type = "image".to_string();
                        first.model = "maverick".to_string();
                    }
                    changed = true;
                }
                if ui.selectable_value(&mut preset.preset_type, "text".to_string(), text.preset_type_text).clicked() {
                    if let Some(first) = preset.blocks.first_mut() {
                        first.block_type = "text".to_string();
                        first.model = "text_accurate_kimi".to_string();
                    }
                    changed = true;
                }
                if ui.selectable_value(&mut preset.preset_type, "audio".to_string(), text.preset_type_audio).clicked() {
                    if let Some(first) = preset.blocks.first_mut() {
                        first.block_type = "audio".to_string();
                        first.model = "whisper-accurate".to_string();
                    }
                    changed = true;
                }
                ui.add_enabled_ui(false, |ui| {
                    let _ = ui.selectable_value(&mut preset.preset_type, "video".to_string(), text.preset_type_video);
                });
            });
    });

    ui.separator();

    // --- GLOBAL SETTINGS (Hotkeys & Input Methods) ---
    // These apply to the preset activation, not specific blocks
    
    // Input Method Config (Dynamic based on type)
    if preset.preset_type == "image" {
        ui.horizontal(|ui| {
            ui.label(text.prompt_mode_label); // "Operation Mode"
            egui::ComboBox::from_id_source("prompt_mode_combo")
                .selected_text(if preset.prompt_mode == "dynamic" { text.prompt_mode_dynamic } else { text.prompt_mode_fixed })
                .show_ui(ui, |ui| {
                    if ui.selectable_value(&mut preset.prompt_mode, "fixed".to_string(), text.prompt_mode_fixed).clicked() { changed = true; }
                    if ui.selectable_value(&mut preset.prompt_mode, "dynamic".to_string(), text.prompt_mode_dynamic).clicked() { changed = true; }
                });
        });
    } else if preset.preset_type == "audio" {
        ui.horizontal(|ui| {
            ui.label(text.audio_source_label);
            if ui.radio_value(&mut preset.audio_source, "mic".to_string(), text.audio_src_mic).clicked() { changed = true; }
            if ui.radio_value(&mut preset.audio_source, "device".to_string(), text.audio_src_device).clicked() { changed = true; }
            if ui.checkbox(&mut preset.hide_recording_ui, text.hide_recording_ui_label).clicked() { changed = true; }
        });
    } else if preset.preset_type == "text" {
        ui.horizontal(|ui| {
            ui.label(text.text_input_mode_label);
            egui::ComboBox::from_id_source("text_input_mode_combo")
                .selected_text(if preset.text_input_mode == "type" { text.text_mode_type } else { text.text_mode_select })
                .show_ui(ui, |ui| {
                    if ui.selectable_value(&mut preset.text_input_mode, "select".to_string(), text.text_mode_select).clicked() { changed = true; }
                    if ui.selectable_value(&mut preset.text_input_mode, "type".to_string(), text.text_mode_type).clicked() { changed = true; }
                });
        });
    }

    // Auto Paste Option (Global)
    if ui.checkbox(&mut preset.auto_paste, text.auto_paste_label).clicked() { changed = true; }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(5.0);

    // Hotkeys
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text.hotkeys_section).strong());
        if *recording_hotkey_for_preset == Some(preset_idx) {
            ui.colored_label(egui::Color32::YELLOW, text.press_keys);
            if ui.button(text.cancel_label).clicked() { *recording_hotkey_for_preset = None; }
        } else {
            if ui.button(text.add_hotkey_button).clicked() { *recording_hotkey_for_preset = Some(preset_idx); }
        }
        
        let mut hotkey_to_remove = None;
        for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
            if ui.small_button(format!("{} ⓧ", hotkey.name)).clicked() { hotkey_to_remove = Some(h_idx); }
        }
        if let Some(h) = hotkey_to_remove { preset.hotkeys.remove(h); changed = true; }
    });
    if let Some(msg) = hotkey_conflict_msg {
        if *recording_hotkey_for_preset == Some(preset_idx) {
            ui.colored_label(egui::Color32::RED, msg);
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(5.0);

    // --- PROCESSING CHAIN UI ---
    ui.label(egui::RichText::new("Processing Chain").heading());
    
    let mut block_to_remove = None;
    let mut block_auto_copy_idx = None;

    // Find which block has auto_copy enabled (for radio button logic)
    for (i, block) in preset.blocks.iter().enumerate() {
        if block.auto_copy { block_auto_copy_idx = Some(i); }
    }

    let block_count = preset.blocks.len();
    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
        for (i, block) in preset.blocks.iter_mut().enumerate() {
            let is_first = i == 0;
            
            ui.push_id(format!("block_{}", i), |ui| {
                ui.group(|ui| {
                    // BLOCK HEADER: Type | Visibility | Delete
                    ui.horizontal(|ui| {
                        // 1. Label
                        let type_icon = match block.block_type.as_str() {
                            "image" => Icon::Image,
                            "audio" => Icon::Microphone,
                            _ => Icon::Text,
                        };
                        crate::gui::icons::draw_icon_static(ui, type_icon, None);
                        let title = if is_first {
                            format!("Step 1: Input ({})", block.block_type)
                        } else {
                            format!("Step {}: Process/Refine", i + 1)
                        };
                        ui.label(egui::RichText::new(title).strong());

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // 3. Delete (Only if not first)
                            if !is_first {
                                if icon_button(ui, Icon::Close).on_hover_text("Remove Step").clicked() {
                                    block_to_remove = Some(i);
                                }
                            }
                            
                            // 2. Visibility Toggle
                            let vis_icon = if block.show_overlay { Icon::EyeOpen } else { Icon::EyeClosed };
                            let hover_text = if block.show_overlay { "Overlay Visible" } else { "Background Processing (Hidden)" };
                            if icon_button(ui, vis_icon).on_hover_text(hover_text).clicked() {
                                block.show_overlay = !block.show_overlay;
                                changed = true;
                            }
                        });
                    });
                    
                    ui.add_space(4.0);

                    // BLOCK BODY: Model | Prompt | Settings
                    
                    // Model Selector
                    ui.horizontal(|ui| {
                        ui.label("Model:");
                        let model_def = get_model_by_id(&block.model);
                        let name = model_def.as_ref()
                            .map(|m| match config.ui_language.as_str() {
                                "vi" => &m.name_vi,
                                "ko" => &m.name_ko,
                                _ => &m.name_en,
                            })
                            .map(|s| s.as_str())
                            .unwrap_or(&block.model);
                        
                        egui::ComboBox::from_id_source(format!("model_{}", i))
                            .selected_text(name)
                            .width(250.0)
                            .show_ui(ui, |ui| {
                                let filter_type = match block.block_type.as_str() {
                                    "image" => ModelType::Vision,
                                    "audio" => ModelType::Audio,
                                    _ => ModelType::Text,
                                };
                                
                                for m in get_all_models() {
                                    if m.enabled && m.model_type == filter_type {
                                        let dropdown_label = match config.ui_language.as_str() {
                                            "vi" => format!("{} ({})", &m.name_vi, &m.full_name),
                                            "ko" => format!("{} ({})", &m.name_ko, &m.full_name),
                                            _ => format!("{} ({})", &m.name_en, &m.full_name),
                                        };
                                        if ui.selectable_value(&mut block.model, m.id.clone(), dropdown_label).clicked() {
                                            changed = true;
                                        }
                                    }
                                }
                            });
                    });

                    // Prompt Editor
                    ui.horizontal(|ui| {
                        ui.label("Prompt:");
                        // Helper for indexed language tags
                        if ui.button("+ {lang}").on_hover_text("Insert {languageN} Tag").clicked() {
                            let mut max_num = 0;
                            for k in 1..=10 {
                                if block.prompt.contains(&format!("{{language{}}}", k)) {
                                    max_num = k;
                                }
                            }
                            let next_num = max_num + 1;
                            block.prompt.push_str(&format!(" {{language{}}} ", next_num));
                            let key = format!("language{}", next_num);
                            if !block.language_vars.contains_key(&key) {
                                block.language_vars.insert(key, "Vietnamese".to_string());
                            }
                            changed = true;
                        }
                    });
                    if ui.add(egui::TextEdit::multiline(&mut block.prompt).desired_rows(2).desired_width(f32::INFINITY)).changed() {
                        changed = true;
                    }

                    // NEW: Dynamic Dropdowns for {languageN} tags
                    let mut detected_vars = Vec::new();
                    for k in 1..=10 {
                        let tag = format!("{{language{}}}", k);
                        if block.prompt.contains(&tag) {
                            detected_vars.push(k);
                        }
                    }

                    for num in detected_vars {
                        let key = format!("language{}", num);
                        if !block.language_vars.contains_key(&key) {
                            block.language_vars.insert(key.clone(), "Vietnamese".to_string());
                        }
                        
                        let label_text = match config.ui_language.as_str() {
                            "vi" => format!("Ngôn ngữ {{language{}}}:", num),
                            "ko" => format!("{{language{}}} 언어:", num),
                            _ => format!("Language for {{language{}}}:", num),
                        };

                        ui.horizontal(|ui| {
                             ui.label(label_text);
                             let current_val = block.language_vars.get(&key).cloned().unwrap_or_default();
                             ui.menu_button(current_val, |ui| {
                                 ui.style_mut().wrap = Some(false);
                                 ui.set_min_width(150.0);
                                 ui.add(egui::TextEdit::singleline(search_query).hint_text("Search..."));
                                 let q = search_query.to_lowercase();
                                 egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                     for lang in get_all_languages().iter() {
                                         if q.is_empty() || lang.to_lowercase().contains(&q) {
                                             if ui.button(lang).clicked() {
                                                 block.language_vars.insert(key.clone(), lang.clone());
                                                 changed = true;
                                                 ui.close_menu();
                                             }
                                         }
                                     }
                                 });
                             });
                        });
                    }

                    // Bottom Row: Language | Stream | Copy
                    ui.horizontal(|ui| {
                        // Language Selector
                        ui.label("Target Lang:");
                        let lang_label = block.selected_language.clone();
                        ui.menu_button(lang_label, |ui| {
                            ui.style_mut().wrap = Some(false);
                            ui.set_min_width(150.0);
                            ui.add(egui::TextEdit::singleline(search_query).hint_text("Search..."));
                            let q = search_query.to_lowercase();
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                for lang in get_all_languages().iter() {
                                    if q.is_empty() || lang.to_lowercase().contains(&q) {
                                        if ui.button(lang).clicked() {
                                            block.selected_language = lang.clone();
                                            changed = true;
                                            ui.close_menu();
                                        }
                                    }
                                }
                            });
                        });

                        ui.separator();

                        // Streaming Toggle
                        if ui.checkbox(&mut block.streaming_enabled, "Stream").on_hover_text("Stream text output").clicked() {
                            changed = true;
                        }

                        ui.separator();

                        // Auto Copy (Radio behavior managed manually)
                        let mut is_copy = Some(i) == block_auto_copy_idx;
                        if ui.checkbox(&mut is_copy, "Auto Copy").on_hover_text("Copy this result to clipboard").clicked() {
                            if is_copy {
                                block_auto_copy_idx = Some(i);
                            } else if block_auto_copy_idx == Some(i) {
                                block_auto_copy_idx = None;
                            }
                            changed = true;
                        }
                    });
                });
            });
            
            // Visual Arrow to next step
            if i < block_count - 1 {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("⬇").size(16.0).strong());
                });
            }
        }
    });

    ui.add_space(10.0);
    
    // "Add Step" Button
    if ui.button(egui::RichText::new("+ Add Processing Step").strong()).clicked() {
        preset.blocks.push(ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate to {language}.".to_string(),
            selected_language: "Vietnamese".to_string(),
            streaming_enabled: true,
            show_overlay: true,
            auto_copy: false,
            ..Default::default()
        });
        changed = true;
    }

    // Apply Logic Updates (Radio Button Sync & Auto Paste)
    if changed {
        // Enforce Auto Copy exclusivity
        for (i, block) in preset.blocks.iter_mut().enumerate() {
            block.auto_copy = Some(i) == block_auto_copy_idx;
        }
        
        // Handle deletions
        if let Some(idx) = block_to_remove {
            preset.blocks.remove(idx);
        }

        config.presets[preset_idx] = preset;
    }

    changed
}
