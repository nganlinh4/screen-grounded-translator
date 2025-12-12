use eframe::egui;
use crate::config::{Config, get_all_languages};
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button};
use crate::model_config::{get_all_models, ModelType, get_model_by_id};

pub fn render_preset_editor(
    ui: &mut egui::Ui,
    config: &mut Config,
    preset_idx: usize,
    search_query: &mut String,
    cached_monitors: &mut Vec<String>,
    recording_hotkey_for_preset: &mut Option<usize>,
    hotkey_conflict_msg: &Option<String>,
    text: &LocaleText,
) -> bool {
    // Safety check
    if preset_idx >= config.presets.len() { return false; }

    let mut preset = config.presets[preset_idx].clone();
    let mut changed = false;

    ui.add_space(5.0);

    // 1. Name
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text.preset_name_label).heading());
        if ui.add(egui::TextEdit::singleline(&mut preset.name).font(egui::TextStyle::Heading)).changed() {
            changed = true;
        }
    });
    
    let is_audio = preset.preset_type == "audio";
    let is_video = preset.preset_type == "video";
    let is_image = preset.preset_type == "image";
    let is_text = preset.preset_type == "text";

    // Type Dropdown + Prompt Mode Dropdown (on same line if image or text)
    ui.horizontal(|ui| {
         ui.label(text.preset_type_label);
         let image_label = text.preset_type_image;
         let audio_label = text.preset_type_audio;
         let video_label = text.preset_type_video;
         let text_label = text.preset_type_text;
         
         let selected_text = match preset.preset_type.as_str() {
             "audio" => audio_label,
             "video" => video_label,
             "text" => text_label,
             _ => image_label,
         };
         
         egui::ComboBox::from_id_source("preset_type_combo")
             .selected_text(selected_text)
             .show_ui(ui, |ui| {
                 if ui.selectable_value(&mut preset.preset_type, "text".to_string(), text_label).clicked() {
                     preset.model = "text_accurate_kimi".to_string(); 
                     changed = true;
                 }
                 if ui.selectable_value(&mut preset.preset_type, "image".to_string(), image_label).clicked() {
                     preset.model = "scout".to_string(); 
                     changed = true;
                 }
                 if ui.selectable_value(&mut preset.preset_type, "audio".to_string(), audio_label).clicked() {
                     preset.model = "whisper-fast".to_string(); 
                     changed = true;
                 }
                 ui.add_enabled_ui(false, |ui| {
                     let _ = ui.selectable_value(&mut preset.preset_type, "video".to_string(), video_label);
                 });
             });

         // Prompt Mode Dropdown (only for Image)
         if is_image {
             ui.label(text.prompt_mode_label);
             egui::ComboBox::from_id_source("prompt_mode_combo")
                 .selected_text(if preset.prompt_mode == "dynamic" { text.prompt_mode_dynamic } else { text.prompt_mode_fixed })
                 .show_ui(ui, |ui| {
                     if ui.selectable_value(&mut preset.prompt_mode, "fixed".to_string(), text.prompt_mode_fixed).clicked() {
                         changed = true;
                     }
                     if ui.selectable_value(&mut preset.prompt_mode, "dynamic".to_string(), text.prompt_mode_dynamic).clicked() {
                         changed = true;
                     }
                 });
         }

         // Text Input Mode Dropdown (only for Text)
         if is_text {
             ui.label(text.text_input_mode_label);
             egui::ComboBox::from_id_source("text_input_mode_combo")
                 .selected_text(if preset.text_input_mode == "type" { text.text_mode_type } else { text.text_mode_select })
                 .show_ui(ui, |ui| {
                     if ui.selectable_value(&mut preset.text_input_mode, "select".to_string(), text.text_mode_select).clicked() {
                         changed = true;
                     }
                     if ui.selectable_value(&mut preset.text_input_mode, "type".to_string(), text.text_mode_type).clicked() {
                         changed = true;
                     }
                 });
         }
     });

    if is_video {
        // Video Placeholder UI
        ui.group(|ui| {
             ui.label(egui::RichText::new(text.capture_method_label).strong());
             ui.horizontal(|ui| {
                 if icon_button(ui, Icon::Refresh).on_hover_text("Refresh Monitors").clicked() {
                     *cached_monitors = crate::gui::utils::get_monitor_names();
                 }

                 egui::ComboBox::from_id_source("video_cap_method")
                     .selected_text(if preset.video_capture_method == "region" {
                         text.region_capture.to_string()
                     } else {
                         preset.video_capture_method.strip_prefix("monitor:").unwrap_or("Unknown").to_string()
                     })
                     .show_ui(ui, |ui| {
                         if ui.selectable_value(&mut preset.video_capture_method, "region".to_string(), text.region_capture).clicked() {
                             changed = true;
                         }
                         for monitor in cached_monitors.iter() {
                             let val = format!("monitor:{}", monitor);
                             let label = format!("Full screen ({})", monitor);
                             if ui.selectable_value(&mut preset.video_capture_method, val, label).clicked() {
                                 changed = true;
                             }
                         }
                     });
             });
        });
    } else {
        // Standard UI

        // Logic to show prompt controls:
        // 1. If Audio: show unless using non-Gemini
        // 2. If Image: show ONLY if prompt_mode is "fixed"
        // 3. If Text: always show (since text is either selected or typed, prompt instructs model what to do with it)
        let show_prompt_controls = if is_audio {
            preset.model.contains("gemini")
        } else if is_image {
            preset.prompt_mode != "dynamic"
        } else {
            true // Text preset always uses prompt instructions
        };

        if show_prompt_controls {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(text.prompt_label).strong());
                    if ui.button(text.insert_lang_btn).clicked() {
                        let mut max_num = 0;
                        for i in 1..=10 {
                            if preset.prompt.contains(&format!("{{language{}}}", i)) {
                                max_num = i;
                            }
                        }
                        let next_num = max_num + 1;
                        preset.prompt.push_str(&format!(" {{language{}}} ", next_num));
                        let key = format!("language{}", next_num);
                        if !preset.language_vars.contains_key(&key) {
                            preset.language_vars.insert(key, "Vietnamese".to_string());
                        }
                        changed = true;
                    }
                });
                
                if ui.add(egui::TextEdit::multiline(&mut preset.prompt).desired_rows(3).desired_width(f32::INFINITY)).changed() {
                    changed = true;
                }
                
                if preset.prompt.trim().is_empty() {
                    ui.colored_label(egui::Color32::RED, text.empty_prompt_warning);
                }
                
                // Language tags
                let mut detected_langs = Vec::new();
                for i in 1..=10 {
                    let pattern = format!("{{language{}}}", i);
                    if preset.prompt.contains(&pattern) {
                        detected_langs.push(i);
                    }
                }
                
                for num in detected_langs {
                    let key = format!("language{}", num);
                    if !preset.language_vars.contains_key(&key) {
                        preset.language_vars.insert(key.clone(), "Vietnamese".to_string());
                    }
                    let label = match config.ui_language.as_str() {
                        "vi" => format!("Ngôn ngữ cho thẻ {{language{}}}:", num),
                        "ko" => format!("{{language{}}} 태그 언어:", num),
                        _ => format!("Language for {{language{}}} tag:", num),
                    };
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let current_lang = preset.language_vars.get(&key).cloned().unwrap_or_else(|| "Vietnamese".to_string());
                        ui.menu_button(current_lang.clone(), |ui| {
                            ui.style_mut().wrap = Some(false);
                            ui.set_min_width(150.0);
                            ui.add(egui::TextEdit::singleline(search_query).hint_text(text.search_placeholder));
                            let q = search_query.to_lowercase();
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                for lang in get_all_languages().iter() {
                                    if q.is_empty() || lang.to_lowercase().contains(&q) {
                                        if ui.button(lang).clicked() {
                                            preset.language_vars.insert(key.clone(), lang.clone());
                                            changed = true;
                                            ui.close_menu();
                                        }
                                    }
                                }
                            });
                        });
                    });
                }
            });
        }

        if is_audio {
            ui.group(|ui| {
                ui.label(egui::RichText::new(text.audio_source_label).strong());
                ui.horizontal(|ui| {
                    if ui.radio_value(&mut preset.audio_source, "mic".to_string(), text.audio_src_mic).clicked() {
                        changed = true;
                    }
                    if ui.radio_value(&mut preset.audio_source, "device".to_string(), text.audio_src_device).clicked() {
                        changed = true;
                    }
                    if ui.checkbox(&mut preset.hide_recording_ui, text.hide_recording_ui_label).clicked() {
                        changed = true;
                    }
                });
            });
        }

        ui.group(|ui| {
            ui.label(egui::RichText::new(text.model_section).strong());
            
            ui.horizontal(|ui| {
                let selected_model = get_model_by_id(&preset.model);
                let display_label = selected_model.as_ref()
                    .map(|m| match config.ui_language.as_str() {
                        "vi" => &m.name_vi,
                        "ko" => &m.name_ko,
                        _ => &m.name_en,
                    })
                    .map(|s| s.as_str())
                    .unwrap_or(&preset.model);

                egui::ComboBox::from_id_source("model_selector")
                    .selected_text(display_label)
                    .show_ui(ui, |ui| {
                        let target_type = if is_audio { ModelType::Audio } 
                                          else if is_text { ModelType::Text } // Show text models for Text preset
                                          else { ModelType::Vision };
                        for model in get_all_models() {
                            if model.enabled && model.model_type == target_type {
                                let dropdown_label = format!("{} ({}) - {}", 
                                    match config.ui_language.as_str() {
                                        "vi" => &model.name_vi,
                                        "ko" => &model.name_ko,
                                        _ => &model.name_en,
                                    },
                                    model.full_name,
                                    match config.ui_language.as_str() {
                                        "vi" => &model.quota_limit_vi,
                                        "ko" => &model.quota_limit_ko,
                                        _ => &model.quota_limit_en,
                                    }
                                );
                                if ui.selectable_value(&mut preset.model, model.id.clone(), dropdown_label).clicked() {
                                     changed = true;
                                     if is_audio && preset.model.contains("gemini") && preset.prompt.trim().is_empty() {
                                         preset.prompt = "Transcribe the audio accurately.".to_string();
                                     } else if is_audio && !preset.model.contains("gemini") && preset.prompt == "Transcribe the audio accurately." {
                                         preset.prompt = "".to_string();
                                     }
                                }
                            }
                        }
                    });

                 if !preset.hide_overlay {
                     ui.label(text.streaming_label);
                     egui::ComboBox::from_id_source("stream_combo")
                         .selected_text(if preset.streaming_enabled { text.streaming_option_stream } else { text.streaming_option_wait })
                         .show_ui(ui, |ui| {
                             if ui.selectable_value(&mut preset.streaming_enabled, false, text.streaming_option_wait).clicked() { changed = true; }
                             if ui.selectable_value(&mut preset.streaming_enabled, true, text.streaming_option_stream).clicked() { changed = true; }
                         });
                 }
            });

            ui.horizontal(|ui| {
                 // 1. Auto Copy Checkbox
                 if ui.checkbox(&mut preset.auto_copy, text.auto_copy_label).clicked() {
                     changed = true;
                     // Disable sub-features if copy is off
                     if !preset.auto_copy { 
                         preset.retranslate_auto_copy = false; 
                         preset.auto_paste = false; // Disable paste if copy is off
                     }
                 }

                 // Show sub-options only if Auto Copy is enabled
                 if preset.auto_copy {
                     // 2. Hide Overlay Checkbox
                     if ui.checkbox(&mut preset.hide_overlay, text.hide_overlay_label).clicked() {
                         changed = true;
                         // If overlay is shown, auto-paste generally shouldn't happen because focus is lost
                         if !preset.hide_overlay {
                             preset.auto_paste = false; 
                         }
                     }

                     // 3. Auto Paste Checkbox (Only if Hide Overlay is ON)
                     if preset.hide_overlay {
                          if ui.checkbox(&mut preset.auto_paste, text.auto_paste_label).clicked() {
                              changed = true;
                          }
                     }
                 }
             });

             // 4. Auto Paste Newline Checkbox
             // Only show if Auto Copy is enabled
             if preset.auto_copy {
                 if ui.checkbox(&mut preset.auto_paste_newline, text.auto_paste_newline_label).clicked() {
                     changed = true;
                 }
             }
        });

        if !preset.hide_overlay {
            ui.group(|ui| {
                ui.label(egui::RichText::new(text.retranslate_section).strong());
                
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut preset.retranslate, text.retranslate_checkbox).clicked() {
                        changed = true;
                    }
                    
                    if preset.retranslate {
                        ui.label(text.retranslate_to_label);
                        let retrans_label = preset.retranslate_to.clone();
                        ui.menu_button(retrans_label, |ui| {
                            ui.style_mut().wrap = Some(false);
                            ui.set_min_width(150.0);
                            ui.add(egui::TextEdit::singleline(search_query).hint_text(text.search_placeholder));
                            let q = search_query.to_lowercase();
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                for lang in get_all_languages().iter() {
                                    if q.is_empty() || lang.to_lowercase().contains(&q) {
                                        if ui.button(lang).clicked() {
                                            preset.retranslate_to = lang.clone();
                                            changed = true;
                                            ui.close_menu();
                                        }
                                    }
                                }
                            });
                        });
                    }
                });

                if preset.retranslate {
                    ui.horizontal(|ui| {
                        ui.label(text.retranslate_model_label);
                        let text_model = get_model_by_id(&preset.retranslate_model);
                        let text_display_label = text_model.as_ref()
                            .map(|m| match config.ui_language.as_str() {
                                "vi" => &m.name_vi,
                                "ko" => &m.name_ko,
                                _ => &m.name_en,
                            })
                            .map(|s| s.as_str())
                            .unwrap_or(&preset.retranslate_model);
                        
                        egui::ComboBox::from_id_source("text_model_selector")
                            .selected_text(text_display_label)
                            .show_ui(ui, |ui| {
                                for model in get_all_models() {
                                    if model.enabled && model.model_type == ModelType::Text {
                                        let dropdown_label = format!("{} ({}) - {}", 
                                            match config.ui_language.as_str() {
                                                "vi" => &model.name_vi,
                                                "ko" => &model.name_ko,
                                                _ => &model.name_en,
                                            },
                                            model.full_name,
                                            match config.ui_language.as_str() {
                                                "vi" => &model.quota_limit_vi,
                                                "ko" => &model.quota_limit_ko,
                                                _ => &model.quota_limit_en,
                                            }
                                        );
                                        if ui.selectable_value(&mut preset.retranslate_model, model.id.clone(), dropdown_label).clicked() {
                                            changed = true;
                                        }
                                    }
                                }
                            });
                        
                        if ui.checkbox(&mut preset.retranslate_auto_copy, text.auto_copy_label).clicked() {
                                changed = true;
                                if preset.retranslate_auto_copy { preset.auto_copy = false; }
                        }
                    });

                    if !preset.hide_overlay {
                        ui.horizontal(|ui| {
                            ui.label(text.streaming_label);
                            egui::ComboBox::from_id_source("retrans_stream_combo")
                                .selected_text(if preset.retranslate_streaming_enabled { text.streaming_option_stream } else { text.streaming_option_wait })
                                .show_ui(ui, |ui| {
                                    if ui.selectable_value(&mut preset.retranslate_streaming_enabled, false, text.streaming_option_wait).clicked() { changed = true; }
                                    if ui.selectable_value(&mut preset.retranslate_streaming_enabled, true, text.streaming_option_stream).clicked() { changed = true; }
                                });
                        });
                    }
                }
            });
        }

        ui.group(|ui| {
           ui.label(egui::RichText::new(text.hotkeys_section).strong());
           
           let mut hotkey_to_remove = None;
           for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
               ui.horizontal(|ui| {
                   ui.label(&hotkey.name);
                   if ui.small_button("x").clicked() {
                       hotkey_to_remove = Some(h_idx);
                   }
               });
           }
           if let Some(h_idx) = hotkey_to_remove {
               preset.hotkeys.remove(h_idx);
               changed = true;
           }

           if *recording_hotkey_for_preset == Some(preset_idx) {
               ui.horizontal(|ui| {
                   ui.colored_label(egui::Color32::YELLOW, text.press_keys);
                   if ui.button(text.cancel_label).clicked() {
                       *recording_hotkey_for_preset = None;
                   }
               });
               if let Some(msg) = hotkey_conflict_msg {
                   ui.colored_label(egui::Color32::RED, msg);
               }
           } else {
               if ui.button(text.add_hotkey_button).clicked() {
                   *recording_hotkey_for_preset = Some(preset_idx);
               }
           }
       });
    }

    if changed {
        config.presets[preset_idx] = preset;
    }
    
    changed
}
