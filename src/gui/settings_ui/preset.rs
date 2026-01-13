use eframe::egui;
use crate::config::{Config, ProcessingBlock};
use crate::gui::locale::LocaleText;
use super::get_localized_preset_name;
use egui_snarl::Snarl;
use super::node_graph::{ChainNode, render_node_graph, blocks_to_snarl, request_node_graph_view_reset};

pub fn render_preset_editor(
    ui: &mut egui::Ui,
    config: &mut Config,
    preset_idx: usize,
    _search_query: &mut String,
    _cached_monitors: &mut Vec<String>,
    recording_hotkey_for_preset: &mut Option<usize>,
    hotkey_conflict_msg: &Option<String>,
    text: &LocaleText,
    snarl: &mut Snarl<ChainNode>,
) -> bool {
    if preset_idx >= config.presets.len() { return false; }

    let mut preset = config.presets[preset_idx].clone();
    let mut changed = false;

    // Constrain entire preset editor to a consistent width (matching history UI)
    ui.set_max_width(510.0);

    // Check if this is a default preset (ID starts with "preset_")
    let is_default_preset = preset.id.starts_with("preset_");
    
    // Get localized name for default presets
    let display_name = if is_default_preset {
        get_localized_preset_name(&preset.id, &config.ui_language)
    } else {
        preset.name.clone()
    };

    // --- HEADER CARD: Name, Type & Settings ---
    let is_dark = ui.visuals().dark_mode;
    let header_bg = if is_dark {
        egui::Color32::from_rgba_unmultiplied(28, 32, 42, 250)  // Darker for better text contrast
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 255)  // Pure white for light mode
    };
    let header_stroke = if is_dark {
        egui::Stroke::new(1.0, egui::Color32::from_gray(50))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(210))
    };
    
    ui.add_space(5.0);
    egui::Frame::new()
        .fill(header_bg)
        .stroke(header_stroke)
        .inner_margin(12.0)
        .corner_radius(10.0)
        .show(ui, |ui| {
            // Row 1: Preset Name + Controller + Restore
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(text.preset_name_label).strong());
                
                if is_default_preset {
                    ui.label(egui::RichText::new(&display_name).strong().size(15.0));
                } else {
                    if ui.add(egui::TextEdit::singleline(&mut preset.name).font(egui::TextStyle::Body)).changed() {
                        changed = true;
                    }
                }
                
                ui.add_space(10.0);
                
                // Controller checkbox with subtle styling
                // Hide for realtime audio presets (they always use the realtime overlay)
                let is_realtime_audio = preset.preset_type == "audio" && preset.audio_processing_mode == "realtime";
                if !is_realtime_audio {
                    if ui.checkbox(&mut preset.show_controller_ui, text.controller_checkbox_label).clicked() {
                        if !preset.show_controller_ui && preset.blocks.is_empty() {
                            preset.blocks.push(create_default_block_for_type(&preset.preset_type));
                            *snarl = blocks_to_snarl(&preset.blocks, &preset.block_connections, &preset.preset_type);
                        }
                        changed = true;
                    }
                }
                
                if is_default_preset {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Restore button with subtle styling
                        let restore_bg = if is_dark { 
                            egui::Color32::from_rgb(80, 70, 100) 
                        } else { 
                            egui::Color32::from_rgb(180, 170, 200) 
                        };
                        if ui.add(egui::Button::new(egui::RichText::new(text.restore_preset_btn).color(egui::Color32::WHITE).small())
                            .fill(restore_bg)
                            .corner_radius(8.0))
                            .on_hover_text(text.restore_preset_tooltip)
                            .clicked() {
                            let default_config = Config::default();
                            if let Some(default_p) = default_config.presets.iter().find(|p| p.id == preset.id) {
                                preset = default_p.clone();
                                *snarl = blocks_to_snarl(&preset.blocks, &preset.block_connections, &preset.preset_type);
                                request_node_graph_view_reset(ui.ctx());
                                changed = true;
                            }
                        }
                    });
                }
            });

            ui.add_space(6.0);
            
            // Row 2: Type + Mode selectors
            ui.horizontal(|ui| {
                ui.label(text.preset_type_label);
                let selected_text = match preset.preset_type.as_str() {
                    "audio" => text.preset_type_audio,
                    "video" => text.preset_type_video,
                    "text" => text.preset_type_text,
                    _ => text.preset_type_image,
                };
                
                egui::ComboBox::from_id_salt("preset_type_combo")
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

                ui.add_space(15.0);

                // Mode selectors based on type
                if preset.preset_type == "image" {
                    if !preset.show_controller_ui {
                        ui.label(text.command_mode_label);
                        egui::ComboBox::from_id_salt("prompt_mode_combo")
                            .selected_text(if preset.prompt_mode == "dynamic" { text.prompt_mode_dynamic } else { text.prompt_mode_fixed })
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut preset.prompt_mode, "fixed".to_string(), text.prompt_mode_fixed).clicked() { changed = true; }
                                if ui.selectable_value(&mut preset.prompt_mode, "dynamic".to_string(), text.prompt_mode_dynamic).clicked() { changed = true; }
                            });
                    }
                } else if preset.preset_type == "text" {
                    ui.label(text.text_input_mode_label);
                    egui::ComboBox::from_id_salt("text_input_mode_combo")
                        .selected_text(if preset.text_input_mode == "type" { text.text_mode_type } else { text.text_mode_select })
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut preset.text_input_mode, "select".to_string(), text.text_mode_select).clicked() { changed = true; }
                            if ui.selectable_value(&mut preset.text_input_mode, "type".to_string(), text.text_mode_type).clicked() { changed = true; }
                        });
                    
                    if preset.text_input_mode == "type" && !preset.show_controller_ui {
                        if ui.checkbox(&mut preset.continuous_input, text.continuous_input_label).clicked() { changed = true; }
                    }
                } else if preset.preset_type == "audio" {
                    if !preset.show_controller_ui {
                        let mode_label = match config.ui_language.as_str() {
                            "vi" => "Phương thức:",
                            "ko" => "작동 방식:",
                            _ => "Mode:",
                        };
                        ui.label(mode_label);
                        
                        let mode_record = match config.ui_language.as_str() {
                            "vi" => "Thu âm rồi xử lý",
                            "ko" => "녹음 후 처리",
                            _ => "Record then Process",
                        };
                        let mode_realtime = match config.ui_language.as_str() {
                            "vi" => "Xử lý thời gian thực",
                            "ko" => "실시간 처리",
                            _ => "Realtime Processing",
                        };
                        
                        let selected_mode_text = if preset.audio_processing_mode == "realtime" {
                            mode_realtime
                        } else {
                            mode_record
                        };
                        
                        egui::ComboBox::from_id_salt("audio_operation_mode_combo")
                            .selected_text(selected_mode_text)
                            .show_ui(ui, |ui| {
                                if ui.selectable_value(&mut preset.audio_processing_mode, "record_then_process".to_string(), mode_record).clicked() { changed = true; }
                                if ui.selectable_value(&mut preset.audio_processing_mode, "realtime".to_string(), mode_realtime).clicked() { changed = true; }
                            });


                    }
                }
            });

            // Row 2.5: Realtime Interface
            if preset.preset_type == "audio" && preset.audio_processing_mode == "realtime" && !preset.show_controller_ui {
                 ui.add_space(8.0);
                 ui.horizontal(|ui| {
                      let window_mode_label = match config.ui_language.as_str() {
                          "vi" => "Giao diện:",
                          "ko" => "인터페이스:",
                          _ => "Interface:",
                      };
                      ui.label(window_mode_label);

                      let mode_standard = match config.ui_language.as_str() {
                          "vi" => "Tiêu chuẩn",
                          "ko" => "표준",
                          _ => "Standard",
                      };
                      let mode_minimal = match config.ui_language.as_str() {
                          "vi" => "Tối giản",
                          "ko" => "최소",
                          _ => "Minimal",
                      };

                      let selected_window_mode = if preset.realtime_window_mode == "minimal" {
                          mode_minimal
                      } else {
                          mode_standard
                      };

                      egui::ComboBox::from_id_salt("realtime_window_mode_combo")
                          .selected_text(selected_window_mode)
                          .show_ui(ui, |ui| {
                              if ui.selectable_value(&mut preset.realtime_window_mode, "standard".to_string(), mode_standard).clicked() { changed = true; }
                              if ui.selectable_value(&mut preset.realtime_window_mode, "minimal".to_string(), mode_minimal).clicked() { changed = true; }
                          });
                 });
            }

            // Row 3: Audio source (if applicable) - Hide if Realtime mode
            if preset.preset_type == "audio" && preset.audio_processing_mode != "realtime" {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(text.audio_source_label);
                    let selected_text = if preset.audio_source == "mic" { text.audio_src_mic } else { text.audio_src_device };
                    egui::ComboBox::from_id_salt("audio_source_combo")
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut preset.audio_source, "mic".to_string(), text.audio_src_mic).clicked() { changed = true; }
                            if ui.selectable_value(&mut preset.audio_source, "device".to_string(), text.audio_src_device).clicked() { changed = true; }
                        });
                    if !preset.show_controller_ui {
                        ui.add_space(10.0);
                        if ui.checkbox(&mut preset.hide_recording_ui, text.hide_recording_ui_label).clicked() { changed = true; }
                        ui.add_space(6.0);
                        if ui.checkbox(&mut preset.auto_stop_recording, text.auto_stop_recording_label).clicked() { changed = true; }
                    }
                });
            }

            // Row 3b: Command mode for text select presets (new row)
            if preset.preset_type == "text" && preset.text_input_mode == "select" && !preset.show_controller_ui {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(text.command_mode_label);
                    egui::ComboBox::from_id_salt("text_prompt_mode_combo")
                        .selected_text(if preset.prompt_mode == "dynamic" { text.prompt_mode_dynamic } else { text.prompt_mode_fixed })
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut preset.prompt_mode, "fixed".to_string(), text.prompt_mode_fixed).clicked() { changed = true; }
                            if ui.selectable_value(&mut preset.prompt_mode, "dynamic".to_string(), text.prompt_mode_dynamic).clicked() { changed = true; }
                        });
                });
            }
        });

    ui.add_space(8.0);

    // Determine visibility conditions: Auto Paste is visible if any NON-input_adapter block has auto_copy enabled
    let has_any_auto_copy = preset.blocks.iter().any(|b| b.auto_copy && b.block_type != "input_adapter");
    
    // Show auto-paste control whenever any applicable block has auto_copy enabled AND controller UI is off
    if has_any_auto_copy && !preset.show_controller_ui {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut preset.auto_paste, text.auto_paste_label).clicked() { changed = true; }
            
            // Auto Newline: visible when any (non-input-adapter) block has auto_copy
            // Since has_any_auto_copy already excludes input_adapter, we can show it directly
            if ui.checkbox(&mut preset.auto_paste_newline, text.auto_paste_newline_label).clicked() { changed = true; }
        });
    } else if !has_any_auto_copy {
        // No auto_copy means auto_paste must be off
        if preset.auto_paste {
            preset.auto_paste = false;
            changed = true;
        }
    }

    ui.add_space(10.0);

    // Hotkeys - always visible, even when controller UI is enabled
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text.hotkeys_section).strong());
        
        let is_dark = ui.visuals().dark_mode;
        
        if *recording_hotkey_for_preset == Some(preset_idx) {
            let text_color = if is_dark { 
                egui::Color32::from_rgb(255, 200, 60)  // Warm orange-yellow for dark mode
            } else { 
                egui::Color32::from_rgb(200, 130, 0)   // Dark orange for light mode
            };
            ui.colored_label(text_color, text.press_keys);
            // Cancel button - subtle red pill
            let cancel_bg = if is_dark { 
                egui::Color32::from_rgb(120, 60, 60) 
            } else { 
                egui::Color32::from_rgb(220, 150, 150) 
            };
            if ui.add(egui::Button::new(egui::RichText::new(text.cancel_label).color(egui::Color32::WHITE))
                .fill(cancel_bg)
                .corner_radius(10.0))
                .clicked() { 
                *recording_hotkey_for_preset = None; 
            }
        } else {
            // Add hotkey button - teal pill
            let add_bg = if is_dark { 
                egui::Color32::from_rgb(50, 110, 120) 
            } else { 
                egui::Color32::from_rgb(100, 170, 180) 
            };
            if ui.add(egui::Button::new(egui::RichText::new(text.add_hotkey_button).color(egui::Color32::WHITE))
                .fill(add_bg)
                .corner_radius(10.0))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked() { 
                *recording_hotkey_for_preset = Some(preset_idx); 
            }
        }
        
        // Hotkey badges - purple/violet tint pills
        let hotkey_bg = if is_dark { 
            egui::Color32::from_rgb(90, 70, 130) 
        } else { 
            egui::Color32::from_rgb(170, 150, 200) 
        };
        
        let mut hotkey_to_remove = None;
        for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
            if ui.add(egui::Button::new(egui::RichText::new(format!("{} ×", hotkey.name)).color(egui::Color32::WHITE).small())
                .fill(hotkey_bg)
                .corner_radius(10.0))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked() { 
                hotkey_to_remove = Some(h_idx); 
            }
        }
        if let Some(h) = hotkey_to_remove { preset.hotkeys.remove(h); changed = true; }
    });
    if let Some(msg) = hotkey_conflict_msg {
        if *recording_hotkey_for_preset == Some(preset_idx) {
            ui.colored_label(egui::Color32::RED, msg);
        }
    }

    // --- PROCESSING CHAIN UI ---
    // Hide nodegraph when controller UI is enabled OR when in Realtime mode (no graph needed)
    if !preset.show_controller_ui && !(preset.preset_type == "audio" && preset.audio_processing_mode == "realtime") {
        // Use a subtle background for the node graph area
        let is_dark = ui.visuals().dark_mode;
        let graph_bg = if is_dark {
            egui::Color32::from_rgba_unmultiplied(35, 40, 50, 200)  // Subtle dark blue-gray
        } else {
            egui::Color32::from_rgba_unmultiplied(240, 242, 248, 255)  // Soft light gray
        };
        
        ui.push_id("node_graph_area", |ui| {
            egui::Frame::new()
                .fill(graph_bg)
                .inner_margin(6.0)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_min_height(325.0); // Allocate space for the graph
                    if render_node_graph(ui, snarl, &config.ui_language, &preset.prompt_mode, config.use_groq, config.use_gemini, config.use_openrouter, config.use_ollama, &preset.preset_type, text) {
                        changed = true;
                    }
                });
        });
    } else {
        // Controller UI mode - show elegant, minimal description
        ui.add_space(20.0);
        
        // Use a subtle background that works in both light and dark modes
        let is_dark = ui.visuals().dark_mode;
        let bg_color = if is_dark {
            egui::Color32::from_rgba_unmultiplied(60, 70, 85, 180)  // Subtle dark blue-gray
        } else {
            egui::Color32::from_rgba_unmultiplied(230, 235, 245, 255)  // Soft light gray-blue
        };
        
        let text_color = if is_dark {
            egui::Color32::from_gray(200)
        } else {
            egui::Color32::from_gray(60)
        };
        
        let accent_color = if is_dark {
            egui::Color32::from_rgb(130, 180, 230)  // Soft blue
        } else {
            egui::Color32::from_rgb(70, 120, 180)   // Deeper blue for light mode
        };
        
        egui::Frame::new()
            .fill(bg_color)
            .inner_margin(24.0)
            .corner_radius(12.0)
            .show(ui, |ui| {
                ui.set_min_height(260.0);
                
                // Title - clean, no emoji overload
                let is_realtime = preset.preset_type == "audio" && preset.audio_processing_mode == "realtime";
                
                let title = if is_realtime {
                    match config.ui_language.as_str() {
                        "vi" => "Xử lý âm thanh (Thời gian thực)",
                        "ko" => "오디오 처리 (실시간)",
                        _ => "Audio Processing (Realtime)",
                    }
                } else {
                    match config.ui_language.as_str() {
                        "vi" => "Chế độ Bộ điều khiển",
                        "ko" => "컨트롤러 모드",
                        _ => "Controller Mode",
                    }
                };
                ui.label(egui::RichText::new(title).heading().color(accent_color));
                
                ui.add_space(16.0);
                
                // Main Description - combined into one clear paragraph
                let desc = if is_realtime {
                    match config.ui_language.as_str() {
                        "vi" => "Chế độ này cung cấp phụ đề và dịch thuật trực tiếp theo thời gian thực.\nMã API của Gemini là bắt buộc, tính năng chỉ hoạt động tốt trên âm thanh có lời nói to rõ như podcast!\n\nBạn có thể điều chỉnh cỡ chữ, nguồn âm thanh và ngôn ngữ dịch ngay trong cửa sổ kết quả.",
                        "ko" => "이 모드는 실시간 자막 및 번역을 제공합니다.\nGemini API 키가 필수이며, 명확한 음성이 있는 팟캐스트 같은 오디오에서 잘 작동합니다!\n\n결과 창에서 글꼴 크기, 오디오 소스, 번역 언어를 직접 조정할 수 있습니다.",
                        _ => "This mode provides real-time transcription and translation.\nGemini API key is required, works best on audio with clear speech like podcasts!\n\nYou can adjust font size, audio source, and translation language directly in the result window.",
                    }
                } else {
                    match config.ui_language.as_str() {
                        "vi" => "Đây là cấu hình MASTER. Khi kích hoạt, một bánh xe chọn sẽ xuất hiện để bạn chọn cấu hình muốn sử dụng.\n\nChỉ cần gán một phím tắt để truy cập nhanh nhiều cấu hình khác nhau.",
                        "ko" => "이것은 MASTER 프리셋입니다. 활성화하면 프리셋 휠이 나타나 사용할 프리셋을 선택할 수 있습니다.\n\n하나의 단축키로 여러 프리셋에 빠르게 접근하세요.",
                        _ => "This is a MASTER preset. When activated, a selection wheel will appear letting you choose which preset to use.\n\nAssign a single hotkey for quick access to multiple presets.",
                    }
                };
                ui.label(egui::RichText::new(desc).color(text_color));
            });
    }


    // Apply Logic Updates (Radio Button Sync & Auto Paste)
    if changed {


        config.presets[preset_idx] = preset;
    }

    changed
}

/// Creates a default processing block based on preset type
fn create_default_block_for_type(preset_type: &str) -> ProcessingBlock {
    match preset_type {
        "audio" => ProcessingBlock {
            block_type: "audio".to_string(),
            model: "whisper-accurate".to_string(),
            prompt: "Transcribe this audio.".to_string(),
            selected_language: "Vietnamese".to_string(),
            auto_copy: true,
            ..Default::default()
        },
        "text" => ProcessingBlock {
            block_type: "text".to_string(),
            model: "text_accurate_kimi".to_string(),
            prompt: "Process this text.".to_string(),
            selected_language: "Vietnamese".to_string(),
            auto_copy: true,
            ..Default::default()
        },
        _ => ProcessingBlock {
            block_type: "image".to_string(),
            model: "maverick".to_string(),
            prompt: "Extract text from this image.".to_string(),
            selected_language: "Vietnamese".to_string(),
            show_overlay: true,
            auto_copy: true,
            ..Default::default()
        },
    }
}
