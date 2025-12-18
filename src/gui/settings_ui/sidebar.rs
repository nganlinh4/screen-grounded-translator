use eframe::egui;
use crate::config::{Config, Preset, ThemeMode};
use crate::gui::locale::LocaleText;
use crate::gui::icons::{Icon, icon_button, draw_icon_static, icon_button_sized};
use super::ViewMode;

/// Get localized preset name for default presets (public for reuse in other modules)
pub fn get_localized_preset_name(preset_id: &str, lang_code: &str) -> String {
    match (preset_id, lang_code) {
        // Vietnamese
        ("preset_translate", "vi") => "Dịch vùng".to_string(),
        ("preset_extract_retranslate", "vi") => "Dịch vùng (CHUẨN)".to_string(),
        ("preset_translate_auto_paste", "vi") => "Dịch vùng (Tự dán)".to_string(),
        ("preset_translate_retranslate", "vi") => "Dịch vùng+Dịch lại".to_string(),
        ("preset_extract_retrans_retrans", "vi") => "D.vùng (CHUẨN)+D.lại".to_string(),
        ("preset_ocr", "vi") => "Lấy text từ ảnh".to_string(),
        ("preset_summarize", "vi") => "Tóm tắt vùng".to_string(),
        ("preset_desc", "vi") => "Mô tả ảnh".to_string(),
        ("preset_ask_image", "vi") => "Hỏi về ảnh".to_string(),
        ("preset_translate_select", "vi") => "Dịch".to_string(),
        ("preset_trans_retrans_select", "vi") => "Dịch+ Dịch lại".to_string(),
        ("preset_select_translate_replace", "vi") => "Dịch và Thay".to_string(),
        ("preset_fix_grammar", "vi") => "Sửa ngữ pháp".to_string(),
        ("preset_rephrase", "vi") => "Viết lại".to_string(),
        ("preset_make_formal", "vi") => "Chuyên nghiệp hóa".to_string(),
        ("preset_explain", "vi") => "Giải thích".to_string(),
        ("preset_ask_text", "vi") => "Hỏi về text".to_string(),
        ("preset_extract_table", "vi") => "Trích bảng".to_string(),
        ("preset_trans_retrans_typing", "vi") => "Dịch+Dịch lại (Tự gõ)".to_string(),
        ("preset_internet_search", "vi") => "Tìm kiếm internet".to_string(),
        ("preset_make_game", "vi") => "Tạo con game".to_string(),
        ("preset_transcribe", "vi") => "Lời nói thành văn".to_string(),
        ("preset_study_language", "vi") => "Học ngoại ngữ".to_string(),
        ("preset_transcribe_retranslate", "vi") => "Trả lời ng.nc.ngoài 1".to_string(),
        ("preset_quicker_foreigner_reply", "vi") => "Trả lời ng.nc.ngoài 2".to_string(),
        ("preset_fact_check", "vi") => "Kiểm chứng thông tin".to_string(),
        ("preset_omniscient_god", "vi") => "Thần Trí tuệ".to_string(),
        ("preset_realtime_audio_translate", "vi") => "Dịch cabin (sắp có)".to_string(),
        // MASTER presets - Vietnamese
        ("preset_image_master", "vi") => "Ảnh MASTER".to_string(),
        ("preset_text_select_master", "vi") => "Bôi MASTER".to_string(),
        ("preset_text_type_master", "vi") => "Gõ MASTER".to_string(),
        ("preset_audio_mic_master", "vi") => "Mic MASTER".to_string(),
        ("preset_audio_device_master", "vi") => "Tiếng MASTER".to_string(),
        
        // Korean
        ("preset_translate", "ko") => "영역 번역".to_string(),
        ("preset_extract_retranslate", "ko") => "영역 번역 (정확)".to_string(),
        ("preset_translate_auto_paste", "ko") => "영역 번역 (자동 붙.)".to_string(),
        ("preset_translate_retranslate", "ko") => "영역 번역+재번역".to_string(),
        ("preset_extract_retrans_retrans", "ko") => "영.번역 (정확)+재번역".to_string(),
        ("preset_ocr", "ko") => "텍스트 추출".to_string(),
        ("preset_summarize", "ko") => "영역 요약".to_string(),
        ("preset_desc", "ko") => "이미지 설명".to_string(),
        ("preset_ask_image", "ko") => "이미지 질문".to_string(),
        ("preset_translate_select", "ko") => "번역 (선택 텍스트)".to_string(),
        ("preset_trans_retrans_select", "ko") => "번역+재번역 (선택)".to_string(),
        ("preset_select_translate_replace", "ko") => "선택-번역-교체".to_string(),
        ("preset_fix_grammar", "ko") => "문법 수정".to_string(),
        ("preset_rephrase", "ko") => "다시 쓰기".to_string(),
        ("preset_make_formal", "ko") => "공식적으로".to_string(),
        ("preset_explain", "ko") => "설명".to_string(),
        ("preset_ask_text", "ko") => "텍스트 질문".to_string(),
        ("preset_extract_table", "ko") => "표 추출".to_string(),
        ("preset_trans_retrans_typing", "ko") => "번역+재번역 (입력)".to_string(),
        ("preset_internet_search", "ko") => "인터넷 검색".to_string(),
        ("preset_make_game", "ko") => "게임 만들기".to_string(),
        ("preset_transcribe", "ko") => "음성 받아쓰기".to_string(),
        ("preset_study_language", "ko") => "언어 학습".to_string(),
        ("preset_transcribe_retranslate", "ko") => "빠른 외국인 답변 1".to_string(),
        ("preset_quicker_foreigner_reply", "ko") => "빠른 외국인 답변 2".to_string(),
        ("preset_fact_check", "ko") => "정보 확인".to_string(),
        ("preset_omniscient_god", "ko") => "전지전능한 신".to_string(),
        ("preset_realtime_audio_translate", "ko") => "실시간 음성 번역 (예정)".to_string(),
        // MASTER presets - Korean
        ("preset_image_master", "ko") => "이미지 마스터".to_string(),
        ("preset_text_select_master", "ko") => "선택 마스터".to_string(),
        ("preset_text_type_master", "ko") => "입력 마스터".to_string(),
        ("preset_audio_mic_master", "ko") => "마이크 마스터".to_string(),
        ("preset_audio_device_master", "ko") => "사운드 마스터".to_string(),
        
        // English (default)
        ("preset_translate", _) => "Translate region".to_string(),
        ("preset_extract_retranslate", _) => "Trans reg (ACCURATE)".to_string(),
        ("preset_translate_auto_paste", _) => "Trans reg (Auto paste)".to_string(),
        ("preset_translate_retranslate", _) => "Trans reg+Retrans".to_string(),
        ("preset_extract_retrans_retrans", _) => "Trans (ACC)+Retrans".to_string(),
        ("preset_ocr", _) => "Extract text".to_string(),
        ("preset_summarize", _) => "Summarize region".to_string(),
        ("preset_desc", _) => "Describe image".to_string(),
        ("preset_ask_image", _) => "Ask about image".to_string(),
        ("preset_translate_select", _) => "Trans (Select text)".to_string(),
        ("preset_trans_retrans_select", _) => "Trans+Retrans (Select)".to_string(),
        ("preset_select_translate_replace", _) => "Select-Trans-Replace".to_string(),
        ("preset_fix_grammar", _) => "Fix Grammar".to_string(),
        ("preset_rephrase", _) => "Rephrase".to_string(),
        ("preset_make_formal", _) => "Make Formal".to_string(),
        ("preset_explain", _) => "Explain".to_string(),
        ("preset_ask_text", _) => "Ask about text".to_string(),
        ("preset_extract_table", _) => "Extract Table".to_string(),
        ("preset_trans_retrans_typing", _) => "Trans+Retrans (Type)".to_string(),
        ("preset_internet_search", _) => "Internet Search".to_string(),
        ("preset_make_game", _) => "Make a Game".to_string(),
        ("preset_transcribe", _) => "Transcribe speech".to_string(),
        ("preset_study_language", _) => "Study language".to_string(),
        ("preset_transcribe_retranslate", _) => "Quick 4NR reply 1".to_string(),
        ("preset_quicker_foreigner_reply", _) => "Quick 4NR reply 2".to_string(),
        ("preset_fact_check", _) => "Fact Check".to_string(),
        ("preset_omniscient_god", _) => "Omniscient God".to_string(),
        ("preset_realtime_audio_translate", _) => "Realtime Audio Trans (soon)".to_string(),
        // MASTER presets - English (default)
        ("preset_image_master", _) => "Image MASTER".to_string(),
        ("preset_text_select_master", _) => "Select MASTER".to_string(),
        ("preset_text_type_master", _) => "Type MASTER".to_string(),
        ("preset_audio_mic_master", _) => "Mic MASTER".to_string(),
        ("preset_audio_device_master", _) => "Sound MASTER".to_string(),
        
        // Fallback: return original ID without "preset_" prefix
        _ => preset_id.strip_prefix("preset_").unwrap_or(preset_id).replace('_', " "),
    }
}


pub fn render_sidebar(
    ui: &mut egui::Ui,
    config: &mut Config,
    view_mode: &mut ViewMode,
    text: &LocaleText,
) -> bool {
    let mut changed = false;
    let mut preset_idx_to_delete = None;
    let mut preset_idx_to_clone = None;
    let mut preset_to_add_type = None;
    let mut preset_idx_to_select: Option<usize> = None;

    // Split indices for presets into 3 columns
    // Column 1: Image presets
    // Column 2: Text presets  
    // Column 3: Audio + Video presets
    let mut image_indices = Vec::new();
    let mut text_indices = Vec::new();
    let mut audio_video_indices = Vec::new();

    for (i, p) in config.presets.iter().enumerate() {
        match p.preset_type.as_str() {
            "image" => image_indices.push(i),
            "text" => text_indices.push(i),
            "audio" | "video" => audio_video_indices.push(i),
            _ => image_indices.push(i), // fallback to image
        }
    }
    
    // Sort audio_video column: Audio first, then Video
    audio_video_indices.sort_by_key(|&i| {
        match config.presets[i].preset_type.as_str() {
            "audio" => 0,
            "video" => 1,
            _ => 2,
        }
    });

    // Capture current view_mode for comparison (avoids borrow issues)
    let current_view_mode = view_mode.clone();
    let mut should_set_global = false;
    let mut should_set_history = false;

    // === UNIFIED GRID: Header + Presets in ONE grid for perfect alignment ===
    egui::Grid::new("sidebar_unified_grid")
        .striped(false)
        .spacing(egui::vec2(8.0, 6.0))
        .min_row_height(24.0) // Ensure consistent row heights
        .show(ui, |ui| {
            // === ROW 1: Theme/Lang/History | (empty) | Global Settings ===
            // Column 1: Use explicit center alignment
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                
                let is_dark = ui.visuals().dark_mode;
                
                // Theme Switcher - styled button with background
                let theme_bg = if is_dark {
                    egui::Color32::from_rgb(50, 55, 70)
                } else {
                    egui::Color32::from_rgb(230, 235, 245)
                };
                let (theme_text, tooltip) = match config.theme_mode {
                    ThemeMode::Dark => ("🌙", "Theme: Dark"),
                    ThemeMode::Light => ("☀", "Theme: Light"),
                    ThemeMode::System => ("💻", "Theme: System (Auto)"),
                };
                if ui.add(egui::Button::new(egui::RichText::new(theme_text).size(14.0))
                    .fill(theme_bg)
                    .corner_radius(6.0))
                    .on_hover_text(tooltip)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked() {
                    config.theme_mode = match config.theme_mode {
                        ThemeMode::System => ThemeMode::Dark,
                        ThemeMode::Dark => ThemeMode::Light,
                        ThemeMode::Light => ThemeMode::System,
                    };
                    changed = true;
                }
                
                // Language Switcher - styled combobox with flag
                let original_lang = config.ui_language.clone();
                let lang_flag = match config.ui_language.as_str() {
                    "vi" => "🇻🇳",
                    "ko" => "🇰🇷",
                    _ => "🇺🇸",
                };
                egui::ComboBox::from_id_source("header_lang_switch")
                    .width(32.0)
                    .selected_text(lang_flag)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.ui_language, "en".to_string(), "🇺🇸 English");
                        ui.selectable_value(&mut config.ui_language, "vi".to_string(), "🇻🇳 Tiếng Việt");
                        ui.selectable_value(&mut config.ui_language, "ko".to_string(), "🇰🇷 한국어");
                    });
                if original_lang != config.ui_language {
                    changed = true;
                }
                
                // History Button - styled teal pill
                let history_bg = if is_dark {
                    egui::Color32::from_rgb(40, 90, 90)
                } else {
                    egui::Color32::from_rgb(100, 180, 180)
                };
                if ui.add(egui::Button::new(egui::RichText::new(format!("📜 {}", text.history_btn)).color(egui::Color32::WHITE))
                    .fill(history_bg)
                    .corner_radius(8.0))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked() {
                    should_set_history = true;
                }
            });

            // Column 2: empty spacer
            ui.label("");

            // Column 3: Global Settings (Right Aligned with Center vertical alignment)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let is_global = matches!(current_view_mode, ViewMode::Global);
                if ui.selectable_label(is_global, text.global_settings).clicked() {
                    should_set_global = true;
                }
                draw_icon_static(ui, Icon::Settings, None);
            });
            ui.end_row();

            // === ROW 2: Add Buttons as Column Headers ===
            let is_dark = ui.visuals().dark_mode;
            
            // Image button - Blue tint
            let img_bg = if is_dark { 
                egui::Color32::from_rgb(45, 85, 140) 
            } else { 
                egui::Color32::from_rgb(100, 150, 220) 
            };
            
            // Text button - Green tint
            let txt_bg = if is_dark { 
                egui::Color32::from_rgb(45, 120, 80) 
            } else { 
                egui::Color32::from_rgb(90, 180, 120) 
            };
            
            // Audio button - Orange/Amber tint
            let aud_bg = if is_dark { 
                egui::Color32::from_rgb(150, 95, 40) 
            } else { 
                egui::Color32::from_rgb(220, 160, 80) 
            };
            
            // Column 1: +Ảnh button
            if ui.add(egui::Button::new(egui::RichText::new(text.add_image_preset_btn).color(egui::Color32::WHITE).strong())
                .fill(img_bg)
                .corner_radius(12.0))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked() {
                preset_to_add_type = Some("image");
            }
            
            // Column 2: +Text button  
            if ui.add(egui::Button::new(egui::RichText::new(text.add_text_preset_btn).color(egui::Color32::WHITE).strong())
                .fill(txt_bg)
                .corner_radius(12.0))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked() {
                preset_to_add_type = Some("text");
            }
            
            // Column 3: +Âm thanh button
            if ui.add(egui::Button::new(egui::RichText::new(text.add_audio_preset_btn).color(egui::Color32::WHITE).strong())
                .fill(aud_bg)
                .corner_radius(12.0))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked() {
                preset_to_add_type = Some("audio");
            }
            ui.end_row();

            // === ROW 3+: Preset Items in 3 columns ===
            let max_len = *[image_indices.len(), text_indices.len(), audio_video_indices.len()].iter().max().unwrap_or(&0);

            for i in 0..max_len {
                // Column 1: Image Presets
                if let Some(&idx) = image_indices.get(i) {
                    render_preset_item(ui, &config.presets, idx, &current_view_mode, &mut preset_idx_to_select, &mut preset_idx_to_delete, &mut preset_idx_to_clone, &config.ui_language);
                } else {
                    ui.label("");
                }

                // Column 2: Text Presets
                if let Some(&idx) = text_indices.get(i) {
                    render_preset_item(ui, &config.presets, idx, &current_view_mode, &mut preset_idx_to_select, &mut preset_idx_to_delete, &mut preset_idx_to_clone, &config.ui_language);
                } else {
                    ui.label("");
                }

                // Column 3: Audio + Video Presets
                if let Some(&idx) = audio_video_indices.get(i) {
                    render_preset_item(ui, &config.presets, idx, &current_view_mode, &mut preset_idx_to_select, &mut preset_idx_to_delete, &mut preset_idx_to_clone, &config.ui_language);
                } else {
                    ui.label("");
                }
                
                ui.end_row();
            }
        });

    // Apply view_mode changes after the grid
    if should_set_global {
        *view_mode = ViewMode::Global;
    }
    if should_set_history {
        *view_mode = ViewMode::History;
    }
    if let Some(idx) = preset_idx_to_select {
        *view_mode = ViewMode::Preset(idx);
    }

    // Handle Clone
    if let Some(idx) = preset_idx_to_clone {
        let mut new_preset = config.presets[idx].clone();
        
        // Generate new ID
        new_preset.id = format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        
        // Generate new Name
        let base_name = if config.presets[idx].id.starts_with("preset_") {
            get_localized_preset_name(&config.presets[idx].id, &config.ui_language)
        } else {
            new_preset.name.clone()
        };
        let mut new_name = format!("{} Copy", base_name);
        
        let mut counter = 1;
        while config.presets.iter().any(|p| p.name == new_name) {
            new_name = format!("{} Copy {}", base_name, counter);
            counter += 1;
        }
        new_preset.name = new_name;
        
        // Clear hotkeys to avoid conflicts
        new_preset.hotkeys.clear();
        
        config.presets.push(new_preset);
        *view_mode = ViewMode::Preset(config.presets.len() - 1);
        changed = true;
    }

    // Handle Add
    if let Some(type_str) = preset_to_add_type {
        let mut new_preset = Preset::default();
        if type_str == "text" {
            new_preset.preset_type = "text".to_string();
            new_preset.name = format!("Text {}", config.presets.len() + 1);
            new_preset.text_input_mode = "select".to_string();
            // Update block 0 instead of legacy fields
            if let Some(block) = new_preset.blocks.first_mut() {
                block.block_type = "text".to_string();
                block.model = "text_accurate_kimi".to_string();
                block.prompt = "Translate this text.".to_string();
            }
        } else if type_str == "audio" {
            new_preset.preset_type = "audio".to_string();
            new_preset.name = format!("Audio {}", config.presets.len() + 1);
            new_preset.audio_source = "mic".to_string();
            // Update block 0 instead of legacy fields
            if let Some(block) = new_preset.blocks.first_mut() {
                block.block_type = "audio".to_string();
                block.model = "whisper-fast".to_string();
                block.prompt = "".to_string();
            }
        } else {
            new_preset.name = format!("Image {}", config.presets.len() + 1);
            // Default preset already has image block, no changes needed
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

// Helper function to render a single preset item (avoids closure borrow issues)
fn render_preset_item(
    ui: &mut egui::Ui,
    presets: &[Preset],
    idx: usize,
    current_view_mode: &ViewMode,
    preset_idx_to_select: &mut Option<usize>,
    preset_idx_to_delete: &mut Option<usize>,
    preset_idx_to_clone: &mut Option<usize>,
    lang: &str,
) {
    let preset = &presets[idx];
    
    // Get display name: localized for default presets, original for custom
    let display_name = if preset.id.starts_with("preset_") {
        get_localized_preset_name(&preset.id, lang)
    } else {
        preset.name.clone()
    };
    
    let is_selected = matches!(current_view_mode, ViewMode::Preset(i) if *i == idx);
    let has_hotkey = !preset.hotkeys.is_empty();
    
    let icon_type = match preset.preset_type.as_str() {
        "audio" => {
            // Use Speaker for device audio, Microphone for mic audio
            if preset.audio_source == "device" {
                Icon::Speaker
            } else {
                Icon::Microphone
            }
        },
        "video" => Icon::Video,
        "text" => {
            // Use TextSelect for select mode, Text (T) for type mode
            if preset.text_input_mode == "select" {
                Icon::TextSelect
            } else {
                Icon::Text
            }
        },
        _ => Icon::Image,
    };
    
    // Use horizontal_centered for proper vertical alignment
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        
        // Draw background if has hotkey
        if has_hotkey && !preset.is_upcoming {
            let rect = ui.available_rect_before_wrap();
            let is_dark = ui.visuals().dark_mode;
            
            if is_dark {
                // Dark Mode: Softer, muted teal/green (tint)
                let bg_color = egui::Color32::from_rgba_unmultiplied(40, 150, 130, 70);
                ui.painter().rect_filled(rect, 4.0, bg_color);
            } else {
                // Light Mode: Stronger pastel green (Mint) - needs to be visible against white
                // RGB(210, 245, 230) is a visible mint green
                let bg_color = egui::Color32::from_rgb(200, 235, 220); 
                ui.painter().rect_filled(rect, 4.0, bg_color);
            }
        }
        
        if preset.is_upcoming {
            ui.add_enabled_ui(false, |ui| {
                draw_icon_static(ui, icon_type, Some(14.0));
                let _ = ui.selectable_label(is_selected, &display_name);
            });
        } else {
            draw_icon_static(ui, icon_type, Some(14.0));
            if ui.selectable_label(is_selected, &display_name).clicked() {
                *preset_idx_to_select = Some(idx);
            }
            
            ui.spacing_mut().item_spacing.x = 0.0;
            
            // Copy Button
            let copy_tooltip = match lang {
                "vi" => "Nhân bản",
                "ko" => "복제",
                _ => "Duplicate",
            };
            if icon_button_sized(ui, Icon::CopySmall, 22.0).on_hover_text(copy_tooltip).clicked() {
                *preset_idx_to_clone = Some(idx);
            }

            // Delete button (Small X icon)
            if presets.len() > 1 {
                if icon_button_sized(ui, Icon::Delete, 22.0).clicked() {
                    *preset_idx_to_delete = Some(idx);
                }
            }
        }
    });
}
