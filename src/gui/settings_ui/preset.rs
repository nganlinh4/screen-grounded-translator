use eframe::egui;
use crate::config::{Config, get_all_languages, ProcessingBlock};
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button};
use crate::model_config::{get_all_models, ModelType, get_model_by_id};

/// Get localized preset name for default presets
fn get_localized_preset_name(preset_id: &str, lang: &str) -> String {
    match (preset_id, lang) {
        // Vietnamese
        ("preset_translate", "vi") => "Dịch ảnh".to_string(),
        ("preset_translate_auto_paste", "vi") => "Dịch+Dán".to_string(),
        ("preset_translate_select", "vi") => "Dịch bôi đen".to_string(),
        ("preset_translate_retranslate", "vi") => "Dịch+Dịch lại".to_string(),
        ("preset_trans_retrans_typing", "vi") => "Dịch+Dịch lại (gõ)".to_string(),
        ("preset_ocr", "vi") => "Trích văn bản".to_string(),
        ("preset_extract_retranslate", "vi") => "Trích+Dịch".to_string(),
        ("preset_summarize", "vi") => "Tóm tắt ảnh".to_string(),
        ("preset_desc", "vi") => "Mô tả ảnh".to_string(),
        ("preset_ask_image", "vi") => "Hỏi về ảnh".to_string(),
        ("preset_transcribe", "vi") => "Chép lời nói".to_string(),
        ("preset_study_language", "vi") => "Học ngoại ngữ".to_string(),
        ("preset_transcribe_retranslate", "vi") => "Chép+Dịch".to_string(),
        ("preset_quicker_foreigner_reply", "vi") => "Dịch audio nhanh".to_string(),
        ("preset_video_summary_placeholder", "vi") => "Tóm tắt video (sắp có)".to_string(),
        
        // Korean
        ("preset_translate", "ko") => "이미지 번역".to_string(),
        ("preset_translate_auto_paste", "ko") => "번역+붙여넣기".to_string(),
        ("preset_translate_select", "ko") => "선택 번역".to_string(),
        ("preset_translate_retranslate", "ko") => "번역+재번역".to_string(),
        ("preset_trans_retrans_typing", "ko") => "번역+재번역 (입력)".to_string(),
        ("preset_ocr", "ko") => "텍스트 추출".to_string(),
        ("preset_extract_retranslate", "ko") => "추출+번역".to_string(),
        ("preset_summarize", "ko") => "이미지 요약".to_string(),
        ("preset_desc", "ko") => "이미지 설명".to_string(),
        ("preset_ask_image", "ko") => "이미지 질문".to_string(),
        ("preset_transcribe", "ko") => "음성 받아쓰기".to_string(),
        ("preset_study_language", "ko") => "언어 학습".to_string(),
        ("preset_transcribe_retranslate", "ko") => "받아쓰기+번역".to_string(),
        ("preset_quicker_foreigner_reply", "ko") => "빠른 오디오 번역".to_string(),
        ("preset_video_summary_placeholder", "ko") => "비디오 요약 (예정)".to_string(),
        
        // English (default)
        ("preset_translate", _) => "Translate image".to_string(),
        ("preset_translate_auto_paste", _) => "Translate+Paste".to_string(),
        ("preset_translate_select", _) => "Translate selection".to_string(),
        ("preset_translate_retranslate", _) => "Translate+Retranslate".to_string(),
        ("preset_trans_retrans_typing", _) => "Trans+Retrans (type)".to_string(),
        ("preset_ocr", _) => "Extract text (OCR)".to_string(),
        ("preset_extract_retranslate", _) => "Extract+Translate".to_string(),
        ("preset_summarize", _) => "Summarize image".to_string(),
        ("preset_desc", _) => "Describe image".to_string(),
        ("preset_ask_image", _) => "Ask about image".to_string(),
        ("preset_transcribe", _) => "Transcribe speech".to_string(),
        ("preset_study_language", _) => "Study language".to_string(),
        ("preset_transcribe_retranslate", _) => "Transcribe+Translate".to_string(),
        ("preset_quicker_foreigner_reply", _) => "Quick audio translate".to_string(),
        ("preset_video_summary_placeholder", _) => "Summarize video (soon)".to_string(),
        
        // Fallback: return original ID without "preset_" prefix
        _ => preset_id.strip_prefix("preset_").unwrap_or(preset_id).replace('_', " "),
    }
}


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

    // Constrain entire preset editor to a consistent width (matching history UI)
    ui.set_max_width(400.0);

    // Check if this is a default preset (ID starts with "preset_")
    let is_default_preset = preset.id.starts_with("preset_");
    
    // Get localized name for default presets
    let display_name = if is_default_preset {
        get_localized_preset_name(&preset.id, &config.ui_language)
    } else {
        preset.name.clone()
    };

    // --- HEADER: Name & Main Type ---
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text.preset_name_label).heading());
        
        if is_default_preset {
            // Default presets: show localized name as read-only label
            ui.label(egui::RichText::new(&display_name).heading());
        } else {
            // Custom presets: editable name
            if ui.add(egui::TextEdit::singleline(&mut preset.name).font(egui::TextStyle::Heading)).changed() {
                changed = true;
            }
        }
    });

    // Preset Type Selector + Operation Mode on same line
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

        ui.add_space(10.0);

        // Operation Mode on same line (if applicable)
        if preset.preset_type == "image" {
            ui.label(text.prompt_mode_label);
            egui::ComboBox::from_id_source("prompt_mode_combo")
                .selected_text(if preset.prompt_mode == "dynamic" { text.prompt_mode_dynamic } else { text.prompt_mode_fixed })
                .show_ui(ui, |ui| {
                    if ui.selectable_value(&mut preset.prompt_mode, "fixed".to_string(), text.prompt_mode_fixed).clicked() { changed = true; }
                    if ui.selectable_value(&mut preset.prompt_mode, "dynamic".to_string(), text.prompt_mode_dynamic).clicked() { changed = true; }
                });
        } else if preset.preset_type == "text" {
            ui.label(text.text_input_mode_label);
            egui::ComboBox::from_id_source("text_input_mode_combo")
                .selected_text(if preset.text_input_mode == "type" { text.text_mode_type } else { text.text_mode_select })
                .show_ui(ui, |ui| {
                    if ui.selectable_value(&mut preset.text_input_mode, "select".to_string(), text.text_mode_select).clicked() { changed = true; }
                    if ui.selectable_value(&mut preset.text_input_mode, "type".to_string(), text.text_mode_type).clicked() { changed = true; }
                });
        }
    });

    // Audio-specific options on separate row (takes more space)
    if preset.preset_type == "audio" {
        ui.horizontal(|ui| {
            ui.label(text.audio_source_label);
            if ui.radio_value(&mut preset.audio_source, "mic".to_string(), text.audio_src_mic).clicked() { changed = true; }
            if ui.radio_value(&mut preset.audio_source, "device".to_string(), text.audio_src_device).clicked() { changed = true; }
            if ui.checkbox(&mut preset.hide_recording_ui, text.hide_recording_ui_label).clicked() { changed = true; }
        });
    }

    ui.separator();

    // Determine visibility conditions
    let all_overlays_hidden = preset.blocks.iter().all(|b| !b.show_overlay);
    let has_any_auto_copy = preset.blocks.iter().any(|b| b.auto_copy);
    
    // Only show controls if any block has auto_copy
    if has_any_auto_copy {
        ui.horizontal(|ui| {
            // Auto Paste: only visible when ALL overlays are hidden AND has auto_copy
            if all_overlays_hidden {
                if ui.checkbox(&mut preset.auto_paste, text.auto_paste_label).clicked() { changed = true; }
            }
            
            // Auto Newline: visible when any block has auto_copy
            if ui.checkbox(&mut preset.auto_paste_newline, text.auto_paste_newline_label).clicked() { changed = true; }
        });
    } else {
        // No auto_copy means auto_paste must be off
        if preset.auto_paste {
            preset.auto_paste = false;
            changed = true;
        }
    }

    ui.add_space(10.0);

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
    // Header with title, chain visualization, and Add button
    let mut add_new_block = false;
    
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text.processing_chain_title).heading());
        ui.add_space(10.0);
        
        // Draw chain visualization: icon -> icon -> icon ...
        for (i, block) in preset.blocks.iter().enumerate() {
            let type_icon = match block.block_type.as_str() {
                "image" => Icon::Image,
                "audio" => Icon::Microphone,
                _ => Icon::Text,
            };
            crate::gui::icons::draw_icon_static(ui, type_icon, Some(14.0));
            
            if i < preset.blocks.len() - 1 {
                crate::gui::icons::draw_icon_static(ui, Icon::ChainArrow, Some(14.0));
            }
        }
        
        ui.add_space(5.0);
        
        // "+ Action" button inline with header
        if ui.small_button(text.add_step_btn).clicked() {
            add_new_block = true;
        }
    });
    
    let mut block_to_remove = None;
    let mut block_auto_copy_idx = None;

    // Find which block has auto_copy enabled (for radio button logic)
    for (i, block) in preset.blocks.iter().enumerate() {
        if block.auto_copy { block_auto_copy_idx = Some(i); }
    }

    let block_count = preset.blocks.len();
    
    // Use Frame pattern from history UI for proper fixed height
    egui::Frame::none().show(ui, |ui| {
        ui.set_height(280.0);
        
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, block) in preset.blocks.iter_mut().enumerate() {
                let is_first = i == 0;
            
            ui.push_id(format!("block_{}", i), |ui| {
                ui.group(|ui| {
                    // BLOCK HEADER: Type | Model | Visibility | Delete
                    ui.horizontal(|ui| {
                        // 1. Type icon and Label
                        let type_icon = match block.block_type.as_str() {
                            "image" => Icon::Image,
                            "audio" => Icon::Microphone,
                            _ => Icon::Text,
                        };
                        crate::gui::icons::draw_icon_static(ui, type_icon, None);
                        let title = if is_first {
                            format!("{} ({})", text.step_input_label, block.block_type)
                        } else {
                            format!("{} {}", text.step_process_label, i + 1)
                        };
                        ui.label(egui::RichText::new(title).strong());
                        
                        ui.add_space(5.0);
                        
                        // 2. Model Selector (compact, adapt to content)
                        ui.label(text.model_label);
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

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !is_first {
                                if icon_button(ui, Icon::Close).on_hover_text(text.remove_step_tooltip).clicked() {
                                    block_to_remove = Some(i);
                                }
                            }
                            
                            // 3. Visibility Toggle
                            let vis_icon = if block.show_overlay { Icon::EyeOpen } else { Icon::EyeClosed };
                            let hover_text = if block.show_overlay { text.overlay_visible_tooltip } else { text.overlay_hidden_tooltip };
                            if icon_button(ui, vis_icon).on_hover_text(hover_text).clicked() {
                                block.show_overlay = !block.show_overlay;
                                changed = true;
                            }
                        });
                    });
                    
                    ui.add_space(4.0);

                    // Prompt Editor - hidden for whisper audio models (they just transcribe, no prompt needed)
                    let is_whisper_audio = block.block_type == "audio" && block.model.starts_with("whisper");
                    
                    if !is_whisper_audio {
                        ui.horizontal(|ui| {
                            ui.label(text.prompt_label);
                            // Helper for indexed language tags
                            if ui.button(text.insert_lang_tag_btn).on_hover_text(text.insert_lang_tag_tooltip).clicked() {
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

                        // Dynamic Dropdowns for {languageN} tags
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
                    }

                    // Bottom Row: Stream | Auto Copy (removed redundant Target Lang - use {languageN} tags instead)
                    ui.horizontal(|ui| {
                        // Streaming Toggle - only visible if overlay is shown
                        if block.show_overlay {
                            if ui.checkbox(&mut block.streaming_enabled, text.stream_checkbox).on_hover_text(text.stream_tooltip).clicked() {
                                changed = true;
                            }
                            ui.separator();
                        }

                        // Auto Copy (Radio behavior managed manually)
                        let mut is_copy = Some(i) == block_auto_copy_idx;
                        if ui.checkbox(&mut is_copy, text.auto_copy_label).on_hover_text(text.auto_copy_tooltip).clicked() {
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
            
            // Visual Arrow to next step (using hand-drawn icon)
            if i < block_count - 1 {
                ui.vertical_centered(|ui| {
                    crate::gui::icons::draw_icon_static(ui, Icon::ChainArrowDown, Some(16.0));
                });
            }
        }
        });
    });

    // Handle add new block
    if add_new_block {
        preset.blocks.push(ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Translate to {language1}.".to_string(),
            selected_language: "Vietnamese".to_string(),
            streaming_enabled: true,
            show_overlay: true,
            auto_copy: false,
            ..Default::default()
        });
        changed = true;
    }

    // Handle block deletion (must be outside 'if changed' to work)
    if let Some(idx) = block_to_remove {
        preset.blocks.remove(idx);
        changed = true;
    }

    // Apply Logic Updates (Radio Button Sync & Auto Paste)
    if changed {
        // Enforce Auto Copy exclusivity
        for (i, block) in preset.blocks.iter_mut().enumerate() {
            block.auto_copy = Some(i) == block_auto_copy_idx;
        }

        config.presets[preset_idx] = preset;
    }

    changed
}
