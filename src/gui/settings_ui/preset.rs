use eframe::egui;
use crate::config::{Config, ProcessingBlock};
use crate::gui::locale::LocaleText;
use super::get_localized_preset_name;
use egui_snarl::Snarl;
use super::node_graph::{ChainNode, render_node_graph, blocks_to_snarl};

pub fn render_preset_editor(
    ui: &mut egui::Ui,
    config: &mut Config,
    preset_idx: usize,
    search_query: &mut String,
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
    ui.set_max_width(475.0);

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
            
            // Controller checkbox (Bộ điều khiển) - between title and restore button
            if ui.checkbox(&mut preset.show_controller_ui, text.controller_checkbox_label).clicked() {
                // When unticking controller UI, restore a default block if blocks are empty
                if !preset.show_controller_ui && preset.blocks.is_empty() {
                    preset.blocks.push(create_default_block_for_type(&preset.preset_type));
                    *snarl = blocks_to_snarl(&preset.blocks, &preset.block_connections);
                }
                changed = true;
            }
            
            // Restore Button (Right aligned)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(text.restore_preset_btn).on_hover_text(text.restore_preset_tooltip).clicked() {
                     let default_config = Config::default();
                     if let Some(default_p) = default_config.presets.iter().find(|p| p.id == preset.id) {
                         // Restore to default (reset content to factory state)
                         preset = default_p.clone();
                         *snarl = blocks_to_snarl(&preset.blocks, &preset.block_connections);
                         changed = true;
                     }
                }
            });
        } else {
            // Custom presets: editable name
            if ui.add(egui::TextEdit::singleline(&mut preset.name).font(egui::TextStyle::Heading)).changed() {
                changed = true;
            }
            
            // Controller checkbox (Bộ điều khiển) - also for custom presets
            if ui.checkbox(&mut preset.show_controller_ui, text.controller_checkbox_label).clicked() {
                // When unticking controller UI, restore a default block if blocks are empty
                if !preset.show_controller_ui && preset.blocks.is_empty() {
                    preset.blocks.push(create_default_block_for_type(&preset.preset_type));
                    *snarl = blocks_to_snarl(&preset.blocks, &preset.block_connections);
                }
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
        // When show_controller_ui is enabled:
        // - Image: Hide Phương thức selector
        // - Text: Keep Phương thức visible, but hide Nhập liên tục
        // - Audio: Hide Phương thức selector
        if preset.preset_type == "image" {
            // Hide prompt mode selector when controller UI is enabled
            if !preset.show_controller_ui {
                ui.label(text.prompt_mode_label);
                egui::ComboBox::from_id_source("prompt_mode_combo")
                    .selected_text(if preset.prompt_mode == "dynamic" { text.prompt_mode_dynamic } else { text.prompt_mode_fixed })
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut preset.prompt_mode, "fixed".to_string(), text.prompt_mode_fixed).clicked() { changed = true; }
                        if ui.selectable_value(&mut preset.prompt_mode, "dynamic".to_string(), text.prompt_mode_dynamic).clicked() { changed = true; }
                    });
            }
        } else if preset.preset_type == "text" {
            // Text: keep Phương thức visible always
            ui.label(text.text_input_mode_label);
            egui::ComboBox::from_id_source("text_input_mode_combo")
                .selected_text(if preset.text_input_mode == "type" { text.text_mode_type } else { text.text_mode_select })
                .show_ui(ui, |ui| {
                    if ui.selectable_value(&mut preset.text_input_mode, "select".to_string(), text.text_mode_select).clicked() { changed = true; }
                    if ui.selectable_value(&mut preset.text_input_mode, "type".to_string(), text.text_mode_type).clicked() { changed = true; }
                });
            
            // Show "Continuous Input" checkbox only when typing mode is selected AND controller UI is off
            if preset.text_input_mode == "type" && !preset.show_controller_ui {
                if ui.checkbox(&mut preset.continuous_input, text.continuous_input_label).clicked() { changed = true; }
            }
        } else if preset.preset_type == "audio" {
            // Hide audio mode selector when controller UI is enabled
            if !preset.show_controller_ui {
                // Audio: Cách hoạt động dropdown (same line as preset type)
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
                    "vi" => "Xử lý thời gian thực (upcoming)",
                    "ko" => "실시간 처리 (예정)",
                    _ => "Realtime Processing (upcoming)",
                };
                
                egui::ComboBox::from_id_source("audio_operation_mode_combo")
                    .selected_text(mode_record)
                    .show_ui(ui, |ui| {
                        // Active option
                        ui.selectable_label(true, mode_record);
                        // Grayed out upcoming option
                        ui.add_enabled(false, egui::SelectableLabel::new(false, mode_realtime));
                    });
            }
        }
    });

    // Audio-specific options on separate row (audio source etc)
    // Hide when controller UI is enabled
    if preset.preset_type == "audio" && !preset.show_controller_ui {
        ui.horizontal(|ui| {
            ui.label(text.audio_source_label);
            if ui.radio_value(&mut preset.audio_source, "mic".to_string(), text.audio_src_mic).clicked() { changed = true; }
            if ui.radio_value(&mut preset.audio_source, "device".to_string(), text.audio_src_device).clicked() { changed = true; }
            if ui.checkbox(&mut preset.hide_recording_ui, text.hide_recording_ui_label).clicked() { changed = true; }
        });
    }

    ui.separator();

    // Determine visibility conditions
    let has_any_auto_copy = preset.blocks.iter().any(|b| b.auto_copy);
    
    // Show auto-paste control whenever any block has auto_copy enabled AND controller UI is off
    if has_any_auto_copy && !preset.show_controller_ui {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut preset.auto_paste, text.auto_paste_label).clicked() { changed = true; }
            
            // Auto Newline: visible when any block has auto_copy
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

    // --- PROCESSING CHAIN UI ---
    // Hide nodegraph when controller UI is enabled
    if !preset.show_controller_ui {
        ui.push_id("node_graph_area", |ui| {
            egui::Frame::none().fill(ui.visuals().extreme_bg_color).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_height(325.0); // Allocate space for the graph
                if render_node_graph(ui, snarl, &config.ui_language, &preset.prompt_mode) {
                    changed = true;
                }
            });
        });
    } else {
        // Controller UI mode - show helpful description
        ui.add_space(20.0);
        egui::Frame::none()
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(20.0)
            .corner_radius(8.0)
            .show(ui, |ui| {
                ui.set_min_height(280.0);
                
                // Title
                let title = match config.ui_language.as_str() {
                    "vi" => "🎮 Chế độ Bộ điều khiển",
                    "ko" => "🎮 컨트롤러 모드",
                    _ => "🎮 Controller Mode",
                };
                ui.label(egui::RichText::new(title).heading().color(egui::Color32::from_rgb(100, 180, 255)));
                
                ui.add_space(12.0);
                
                // Main Description
                let desc = match config.ui_language.as_str() {
                    "vi" => "Đây là một cấu hình MASTER. Khi kích hoạt, một bánh xe chọn cấu hình sẽ xuất hiện để bạn chọn cấu hình thực tế muốn sử dụng.",
                    "ko" => "이것은 MASTER 프리셋입니다. 활성화하면 프리셋 휠이 나타나 실제로 사용할 프리셋을 선택할 수 있습니다.",
                    _ => "This is a MASTER preset. When activated, a preset selection wheel will appear letting you choose which preset to actually use.",
                };
                ui.label(egui::RichText::new(desc).color(egui::Color32::from_gray(180)));
                
                ui.add_space(12.0);
                
                // Benefit line with tip styling
                let benefit = match config.ui_language.as_str() {
                    "vi" => "💡 Điều này cho phép bạn gán một phím tắt duy nhất để truy cập nhanh nhiều cấu hình khác nhau.",
                    "ko" => "💡 이를 통해 하나의 단축키로 여러 프리셋에 빠르게 접근할 수 있습니다.",
                    _ => "💡 This allows you to assign a single hotkey for quick access to multiple different presets.",
                };
                ui.label(egui::RichText::new(benefit).italics().color(egui::Color32::from_rgb(180, 180, 120)));
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
