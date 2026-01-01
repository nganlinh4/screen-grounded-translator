use super::ViewMode;
use crate::config::{Config, Preset, ThemeMode};
use crate::gui::icons::{draw_icon_static, icon_button_sized, Icon};
use crate::gui::locale::LocaleText;
use eframe::egui;

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
        ("preset_quick_screenshot", "vi") => "Chụp MH nhanh".to_string(),
        ("preset_quick_screenshot", "ko") => "빠른 스크린샷".to_string(),
        ("preset_quick_screenshot", _) => "Quick screenshot".to_string(),
        ("preset_ocr_read", "vi") => "Đọc vùng này".to_string(),
        ("preset_summarize", "vi") => "Tóm tắt vùng".to_string(),
        ("preset_desc", "vi") => "Mô tả ảnh".to_string(),
        ("preset_ask_image", "vi") => "Hỏi về ảnh".to_string(),
        ("preset_translate_select", "vi") => "Dịch".to_string(),
        ("preset_translate_arena", "vi") => "Dịch (Arena)".to_string(),
        ("preset_read_aloud", "vi") => "Đọc to".to_string(),
        ("preset_trans_retrans_select", "vi") => "Dịch+ Dịch lại".to_string(),
        ("preset_select_translate_replace", "vi") => "Dịch và Thay".to_string(),
        ("preset_fix_grammar", "vi") => "Sửa ngữ pháp".to_string(),
        ("preset_rephrase", "vi") => "Viết lại".to_string(),
        ("preset_make_formal", "vi") => "Chuyên nghiệp hóa".to_string(),
        ("preset_explain", "vi") => "Giải thích".to_string(),
        ("preset_ask_text", "vi") => "Hỏi về text...".to_string(),
        ("preset_edit_as_follows", "vi") => "Sửa như sau:".to_string(),
        ("preset_extract_table", "vi") => "Trích bảng".to_string(),
        ("preset_qr_scanner", "vi") => "Quét mã QR".to_string(),
        ("preset_trans_retrans_typing", "vi") => "Dịch+Dịch lại (Tự gõ)".to_string(),
        ("preset_ask_ai", "vi") => "Hỏi AI".to_string(),
        ("preset_internet_search", "vi") => "Tìm kiếm internet".to_string(),
        ("preset_make_game", "vi") => "Tạo con game".to_string(),
        ("preset_transcribe", "vi") => "Lời nói thành văn".to_string(),
        ("preset_fix_pronunciation", "vi") => "Chỉnh phát âm".to_string(),
        ("preset_study_language", "vi") => "Học ngoại ngữ".to_string(),
        ("preset_transcribe_retranslate", "vi") => "Trả lời ng.nc.ngoài 1".to_string(),
        ("preset_quicker_foreigner_reply", "vi") => "Trả lời ng.nc.ngoài 2".to_string(),
        ("preset_fact_check", "vi") => "Kiểm chứng thông tin".to_string(),
        ("preset_omniscient_god", "vi") => "Thần Trí tuệ".to_string(),
        ("preset_realtime_audio_translate", "vi") => "Dịch cabin".to_string(),
        ("preset_quick_ai_question", "vi") => "Hỏi nhanh AI".to_string(),
        ("preset_voice_search", "vi") => "Nói để search".to_string(),
        ("preset_hang_image", "vi") => "Treo ảnh".to_string(),
        ("preset_hang_text", "vi") => "Treo text".to_string(),
        ("preset_quick_note", "vi") => "Note nhanh".to_string(),
        ("preset_quick_record", "vi") => "Thu âm nhanh".to_string(),
        ("preset_record_device", "vi") => "Thu âm máy".to_string(),
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
        ("preset_ocr_read", "ko") => "영역 읽기".to_string(),
        ("preset_summarize", "ko") => "영역 요약".to_string(),
        ("preset_desc", "ko") => "이미지 설명".to_string(),
        ("preset_ask_image", "ko") => "이미지 질문".to_string(),
        ("preset_translate_select", "ko") => "번역 (선택 텍스트)".to_string(),
        ("preset_translate_arena", "ko") => "번역 (아레나)".to_string(),
        ("preset_read_aloud", "ko") => "크게 읽기".to_string(),
        ("preset_trans_retrans_select", "ko") => "번역+재번역 (선택)".to_string(),
        ("preset_select_translate_replace", "ko") => "선택-번역-교체".to_string(),
        ("preset_fix_grammar", "ko") => "문법 수정".to_string(),
        ("preset_rephrase", "ko") => "다시 쓰기".to_string(),
        ("preset_make_formal", "ko") => "공식적으로".to_string(),
        ("preset_explain", "ko") => "설명".to_string(),
        ("preset_ask_text", "ko") => "텍스트 질문...".to_string(),
        ("preset_edit_as_follows", "ko") => "다음과 같이 수정:".to_string(),
        ("preset_extract_table", "ko") => "표 추출".to_string(),
        ("preset_qr_scanner", "ko") => "QR 스캔".to_string(),
        ("preset_trans_retrans_typing", "ko") => "번역+재번역 (입력)".to_string(),
        ("preset_ask_ai", "ko") => "AI 질문".to_string(),
        ("preset_internet_search", "ko") => "인터넷 검색".to_string(),
        ("preset_make_game", "ko") => "게임 만들기".to_string(),
        ("preset_transcribe", "ko") => "음성 받아쓰기".to_string(),
        ("preset_fix_pronunciation", "ko") => "발음 교정".to_string(),
        ("preset_study_language", "ko") => "언어 학습".to_string(),
        ("preset_transcribe_retranslate", "ko") => "빠른 외국인 답변 1".to_string(),
        ("preset_quicker_foreigner_reply", "ko") => "빠른 외국인 답변 2".to_string(),
        ("preset_fact_check", "ko") => "정보 확인".to_string(),
        ("preset_omniscient_god", "ko") => "전지전능한 신".to_string(),
        ("preset_realtime_audio_translate", "ko") => "실시간 음성 번역".to_string(),
        ("preset_quick_ai_question", "ko") => "빠른 AI 질문".to_string(),
        ("preset_voice_search", "ko") => "음성 검색".to_string(),
        ("preset_hang_image", "ko") => "이미지 오버레이".to_string(),
        ("preset_hang_text", "ko") => "텍스트 오버레이".to_string(),
        ("preset_quick_note", "ko") => "빠른 메모".to_string(),
        ("preset_quick_record", "ko") => "빠른 녹음".to_string(),
        ("preset_record_device", "ko") => "시스템 녹음".to_string(),
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
        ("preset_ocr_read", _) => "Read this region".to_string(),
        ("preset_summarize", _) => "Summarize region".to_string(),
        ("preset_desc", _) => "Describe image".to_string(),
        ("preset_ask_image", _) => "Ask about image".to_string(),
        ("preset_translate_select", _) => "Trans (Select text)".to_string(),
        ("preset_translate_arena", _) => "Trans (Arena)".to_string(),
        ("preset_read_aloud", _) => "Read aloud".to_string(),
        ("preset_trans_retrans_select", _) => "Trans+Retrans (Select)".to_string(),
        ("preset_select_translate_replace", _) => "Select-Trans-Replace".to_string(),
        ("preset_fix_grammar", _) => "Fix Grammar".to_string(),
        ("preset_rephrase", _) => "Rephrase".to_string(),
        ("preset_make_formal", _) => "Make Formal".to_string(),
        ("preset_explain", _) => "Explain".to_string(),
        ("preset_ask_text", _) => "Ask about text...".to_string(),
        ("preset_edit_as_follows", _) => "Edit as follows:".to_string(),
        ("preset_extract_table", _) => "Extract Table".to_string(),
        ("preset_qr_scanner", _) => "QR Scanner".to_string(),
        ("preset_trans_retrans_typing", _) => "Trans+Retrans (Type)".to_string(),
        ("preset_ask_ai", _) => "Ask AI".to_string(),
        ("preset_internet_search", _) => "Internet Search".to_string(),
        ("preset_make_game", _) => "Make a Game".to_string(),
        ("preset_transcribe", _) => "Transcribe speech".to_string(),
        ("preset_fix_pronunciation", _) => "Fix pronunciation".to_string(),
        ("preset_study_language", _) => "Study language".to_string(),
        ("preset_transcribe_retranslate", _) => "Quick 4NR reply 1".to_string(),
        ("preset_quicker_foreigner_reply", _) => "Quick 4NR reply 2".to_string(),
        ("preset_fact_check", _) => "Fact Check".to_string(),
        ("preset_omniscient_god", _) => "Omniscient God".to_string(),
        ("preset_realtime_audio_translate", _) => "Live Translate".to_string(),
        ("preset_quick_ai_question", _) => "Quick AI Question".to_string(),
        ("preset_voice_search", _) => "Voice Search".to_string(),
        ("preset_hang_image", _) => "Image Overlay".to_string(),
        ("preset_hang_text", _) => "Text Overlay".to_string(),
        ("preset_quick_note", _) => "Quick Note".to_string(),
        ("preset_quick_record", _) => "Quick Record".to_string(),
        ("preset_record_device", _) => "Device Audio Record".to_string(),
        // MASTER presets - English (default)
        ("preset_image_master", _) => "Image MASTER".to_string(),
        ("preset_text_select_master", _) => "Select MASTER".to_string(),
        ("preset_text_type_master", _) => "Type MASTER".to_string(),
        ("preset_audio_mic_master", _) => "Mic MASTER".to_string(),
        ("preset_audio_device_master", _) => "Sound MASTER".to_string(),

        // Fallback: return original ID without "preset_" prefix
        _ => preset_id
            .strip_prefix("preset_")
            .unwrap_or(preset_id)
            .replace('_', " "),
    }
}

pub fn render_sidebar(
    ui: &mut egui::Ui,
    config: &mut Config,
    view_mode: &mut ViewMode,
    text: &LocaleText,
) -> bool {
    let mut changed = false;
    let mut preset_to_add_type = None;
    let mut preset_idx_to_select: Option<usize> = None;
    let mut preset_idx_to_delete = None;
    let mut preset_idx_to_clone = None;
    let mut preset_idx_to_toggle_favorite = None;
    let mut preset_swap_request = None;

    // Get currently dragging item index from memory (if any)
    let dragging_idx_id = egui::Id::new("sidebar_drag_source");
    let dragging_source_idx: Option<usize> = ui.memory(|mem| mem.data.get_temp(dragging_idx_id));

    let mut image_indices = Vec::new();
    let mut text_indices = Vec::new();
    let mut audio_video_indices = Vec::new();

    for (i, p) in config.presets.iter().enumerate() {
        match p.preset_type.as_str() {
            "image" => image_indices.push(i),
            "text" => text_indices.push(i),
            "audio" | "video" => audio_video_indices.push(i),
            _ => image_indices.push(i),
        }
    }

    // Audio/Video indices are not sorted by type to allow user reordering.
    // They will appear in the order they are defined in config.presets.

    let current_view_mode = view_mode.clone();
    let mut should_set_global = false;
    let mut should_set_history = false;

    // Use actual grid width from previous frame for Global Settings position
    thread_local! {
        static GRID_WIDTH: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    }
    let cached_grid_width = GRID_WIDTH.with(|w| w.get());

    // --- Header Navigation ---
    // Use horizontal layout that doesn't claim fixed space (avoids influencing grid)
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let is_dark = ui.visuals().dark_mode;

        // Theme Switcher
        let (theme_icon, tooltip) = match config.theme_mode {
            ThemeMode::Dark => (Icon::Moon, "Theme: Dark"),
            ThemeMode::Light => (Icon::Sun, "Theme: Light"),
            ThemeMode::System => (Icon::Device, "Theme: System (Auto)"),
        };

        if icon_button_sized(ui, theme_icon, 20.0)
            .on_hover_text(tooltip)
            .clicked()
        {
            config.theme_mode = match config.theme_mode {
                ThemeMode::System => ThemeMode::Dark,
                ThemeMode::Dark => ThemeMode::Light,
                ThemeMode::Light => ThemeMode::System,
            };
            changed = true;
        }

        // Language Switcher
        let original_lang = config.ui_language.clone();
        let lang_flag = match config.ui_language.as_str() {
            "vi" => "🇻🇳",
            "ko" => "🇰🇷",
            _ => "🇺🇸",
        };
        egui::ComboBox::from_id_salt("header_lang_switch")
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

        // History Button
        let history_bg = if is_dark {
            egui::Color32::from_rgb(40, 90, 90)
        } else {
            egui::Color32::from_rgb(100, 180, 180)
        };
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("📜 {}", text.history_btn))
                        .color(egui::Color32::WHITE),
                )
                .fill(history_bg)
                .corner_radius(8.0),
            )
            .clicked()
        {
            should_set_history = true;
        }

        // Push remaining items to the right side
        let used_width = ui.min_rect().width();
        let remaining = (ui.available_width()).max(0.0);
        ui.add_space(remaining);

        // Help Assistant Button
        let help_bg = if is_dark {
            egui::Color32::from_rgb(80, 60, 120)
        } else {
            egui::Color32::from_rgb(180, 160, 220)
        };
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("❓ {}", text.help_assistant_btn))
                        .color(egui::Color32::WHITE),
                )
                .fill(help_bg)
                .corner_radius(8.0),
            )
            .on_hover_text(text.help_assistant_title)
            .clicked()
        {
            // Trigger the help assistant using TextInput overlay
            std::thread::spawn(|| {
                crate::gui::settings_ui::help_assistant::show_help_input();
            });
        }

        ui.add_space(4.0);

        // Global Settings
        ui.spacing_mut().item_spacing.x = 4.0;
        draw_icon_static(ui, Icon::Settings, None);
        let is_global = matches!(current_view_mode, ViewMode::Global);
        if ui
            .selectable_label(is_global, text.global_settings)
            .clicked()
        {
            should_set_global = true;
        }
    });

    ui.add_space(8.0);

    // --- Presets Grid ---
    // Use stable ID based on preset count and IDs (not names - those change during typing)
    let preset_hash: u64 = config
        .presets
        .iter()
        .fold(config.presets.len() as u64, |acc, p| {
            acc.wrapping_mul(31).wrapping_add(
                p.id.bytes()
                    .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)),
            )
        });
    let grid_id = egui::Id::new("presets_grid").with(preset_hash);

    let grid_response = egui::Grid::new(grid_id)
        .num_columns(6)
        .spacing([8.0, 4.0])
        .min_col_width(67.0)
        .show(ui, |ui| {
            // ROW 1: Add Buttons
            let is_dark = ui.visuals().dark_mode;
            let img_bg = if is_dark {
                egui::Color32::from_rgb(45, 85, 140)
            } else {
                egui::Color32::from_rgb(100, 150, 220)
            };
            let txt_bg = if is_dark {
                egui::Color32::from_rgb(45, 120, 80)
            } else {
                egui::Color32::from_rgb(90, 180, 120)
            };
            let aud_bg = if is_dark {
                egui::Color32::from_rgb(150, 95, 40)
            } else {
                egui::Color32::from_rgb(220, 160, 80)
            };

            // Image
            ui.add(
                egui::Button::new(
                    egui::RichText::new(text.add_image_preset_btn)
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(img_bg)
                .corner_radius(12.0),
            )
            .clicked()
            .then(|| preset_to_add_type = Some("image"));
            ui.label("");

            // Text
            ui.add(
                egui::Button::new(
                    egui::RichText::new(text.add_text_preset_btn)
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(txt_bg)
                .corner_radius(12.0),
            )
            .clicked()
            .then(|| preset_to_add_type = Some("text"));
            ui.label("");

            // Audio
            ui.add(
                egui::Button::new(
                    egui::RichText::new(text.add_audio_preset_btn)
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(aud_bg)
                .corner_radius(12.0),
            )
            .clicked()
            .then(|| preset_to_add_type = Some("audio"));
            ui.label("");
            ui.end_row();

            // ROW 2+: Preset Items
            let max_len = image_indices
                .len()
                .max(text_indices.len())
                .max(audio_video_indices.len());
            for i in 0..max_len {
                // Column 1&2: Image
                if let Some(&idx) = image_indices.get(i) {
                    render_preset_item_parts(
                        ui,
                        &config.presets,
                        idx,
                        dragging_source_idx,
                        &current_view_mode,
                        &mut preset_idx_to_select,
                        &mut preset_idx_to_delete,
                        &mut preset_idx_to_clone,
                        &mut preset_idx_to_toggle_favorite,
                        &mut preset_swap_request,
                        &config.ui_language,
                    );
                } else {
                    ui.label("");
                    ui.label("");
                }

                // Column 3&4: Text
                if let Some(&idx) = text_indices.get(i) {
                    render_preset_item_parts(
                        ui,
                        &config.presets,
                        idx,
                        dragging_source_idx,
                        &current_view_mode,
                        &mut preset_idx_to_select,
                        &mut preset_idx_to_delete,
                        &mut preset_idx_to_clone,
                        &mut preset_idx_to_toggle_favorite,
                        &mut preset_swap_request,
                        &config.ui_language,
                    );
                } else {
                    ui.label("");
                    ui.label("");
                }

                // Column 5&6: Audio
                if let Some(&idx) = audio_video_indices.get(i) {
                    render_preset_item_parts(
                        ui,
                        &config.presets,
                        idx,
                        dragging_source_idx,
                        &current_view_mode,
                        &mut preset_idx_to_select,
                        &mut preset_idx_to_delete,
                        &mut preset_idx_to_clone,
                        &mut preset_idx_to_toggle_favorite,
                        &mut preset_swap_request,
                        &config.ui_language,
                    );
                } else {
                    ui.label("");
                    ui.label("");
                }

                ui.end_row();
            }
        });

    // Update cached grid width for next frame
    GRID_WIDTH.with(|w| w.set(grid_response.response.rect.width()));

    if should_set_global {
        *view_mode = ViewMode::Global;
    }
    if should_set_history {
        *view_mode = ViewMode::History;
    }
    if let Some(idx) = preset_idx_to_select {
        *view_mode = ViewMode::Preset(idx);
    }

    if let Some(idx) = preset_idx_to_toggle_favorite {
        if let Some(preset) = config.presets.get_mut(idx) {
            preset.is_favorite = !preset.is_favorite;
            changed = true;
            crate::overlay::favorite_bubble::update_favorites_panel();
            crate::overlay::favorite_bubble::trigger_blink_animation();
        }
    }

    if let Some(idx) = preset_idx_to_clone {
        let mut new_preset = config.presets[idx].clone();
        new_preset.id = format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
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
        new_preset.hotkeys.clear();
        config.presets.push(new_preset);
        *view_mode = ViewMode::Preset(config.presets.len() - 1);
        changed = true;
    }

    if let Some((idx_a, idx_b)) = preset_swap_request {
        // Swap presets
        config.presets.swap(idx_a, idx_b);
        // If currently selecting one of them, update view_mode
        if let ViewMode::Preset(current) = view_mode {
            if *current == idx_a {
                *view_mode = ViewMode::Preset(idx_b);
            } else if *current == idx_b {
                *view_mode = ViewMode::Preset(idx_a);
            }
        }
        changed = true;
    }

    if let Some(type_str) = preset_to_add_type {
        let mut new_preset = Preset::default();
        if type_str == "text" {
            new_preset.preset_type = "text".to_string();
            new_preset.name = format!("Text {}", config.presets.len() + 1);
            new_preset.text_input_mode = "select".to_string();
            if let Some(block) = new_preset.blocks.first_mut() {
                block.block_type = "text".to_string();
                block.model = "text_accurate_kimi".to_string();
                block.prompt = "Translate this text.".to_string();
            }
        } else if type_str == "audio" {
            new_preset.preset_type = "audio".to_string();
            new_preset.name = format!("Audio {}", config.presets.len() + 1);
            new_preset.audio_source = "mic".to_string();
            if let Some(block) = new_preset.blocks.first_mut() {
                block.block_type = "audio".to_string();
                block.model = "whisper-fast".to_string();
            }
        } else {
            new_preset.name = format!("Image {}", config.presets.len() + 1);
        }
        config.presets.push(new_preset);
        *view_mode = ViewMode::Preset(config.presets.len() - 1);
        changed = true;
    }

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

fn render_preset_item_parts(
    ui: &mut egui::Ui,
    presets: &[Preset],
    idx: usize,
    dragging_source_idx: Option<usize>,
    current_view_mode: &ViewMode,
    preset_idx_to_select: &mut Option<usize>,
    preset_idx_to_delete: &mut Option<usize>,
    preset_idx_to_clone: &mut Option<usize>,
    preset_idx_to_toggle_favorite: &mut Option<usize>,
    preset_swap_request: &mut Option<(usize, usize)>,
    lang: &str,
) {
    let preset = &presets[idx];
    let display_name = if preset.id.starts_with("preset_") {
        get_localized_preset_name(&preset.id, lang)
    } else {
        preset.name.clone()
    };
    let is_selected = matches!(current_view_mode, ViewMode::Preset(i) if *i == idx);
    let has_hotkey = !preset.hotkeys.is_empty();

    let icon_type = match preset.preset_type.as_str() {
        "audio" => {
            if preset.audio_processing_mode == "realtime" {
                Icon::Realtime
            } else if preset.audio_source == "device" {
                Icon::Speaker
            } else {
                Icon::Microphone
            }
        }
        "video" => Icon::Image,
        "text" => {
            if preset.text_input_mode == "select" {
                Icon::TextSelect
            } else {
                Icon::Text
            }
        }
        _ => Icon::Image,
    };

    // --- Column X: Content ---
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if has_hotkey && !preset.is_upcoming {
            let rect = ui.available_rect_before_wrap();
            let is_dark = ui.visuals().dark_mode;
            let bg_color = if is_dark {
                egui::Color32::from_rgba_unmultiplied(40, 150, 130, 70)
            } else {
                egui::Color32::from_rgb(200, 235, 220)
            };
            ui.painter().rect_filled(rect, 4.0, bg_color);
        }
        if preset.is_upcoming {
            ui.add_enabled_ui(false, |ui| {
                draw_icon_static(ui, icon_type, Some(14.0));
                let _ = ui.selectable_label(is_selected, &display_name);
            });
        } else {
            draw_icon_static(ui, icon_type, Some(14.0));
            // Make the label draggable.
            // SelectableLabel by default captures clicks. We want to also capture drags.
            let response = ui.add(
                egui::SelectableLabel::new(is_selected, &display_name)
                    .sense(egui::Sense::click_and_drag()),
            );

            if response.clicked() {
                *preset_idx_to_select = Some(idx);
            }

            // Drag Source Logic
            let dragging_id = egui::Id::new("sidebar_drag_source");
            if response.drag_started() {
                ui.memory_mut(|mem| mem.data.insert_temp(dragging_id, idx));
            }
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if response.drag_stopped() {
                // Clear state when drag stops
                ui.memory_mut(|mem| mem.data.remove::<usize>(dragging_id));
            }

            // Drop Target Logic
            // If dragging, and we are not the source, and hovered, and released
            if let Some(source_idx) = dragging_source_idx {
                if source_idx != idx && response.hovered() && ui.input(|i| i.pointer.any_released())
                {
                    // Check if they are in the same column group
                    let source_preset = &presets[source_idx];
                    // Target is `preset`

                    let get_group = |p: &Preset| -> u8 {
                        match p.preset_type.as_str() {
                            "text" => 1,
                            "audio" | "video" => 2,
                            _ => 0, // Image or default
                        }
                    };

                    if get_group(source_preset) == get_group(preset) {
                        *preset_swap_request = Some((source_idx, idx));
                    }
                }
            }
        }
    });

    // --- Column X+1: Actions ---
    // Use horizontal layout (not right_to_left) to prevent column expansion
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if !preset.is_upcoming {
            // Drag handle removed - label is now draggable

            if icon_button_sized(ui, Icon::CopySmall, 22.0).clicked() {
                *preset_idx_to_clone = Some(idx);
            }
            let star_icon = if preset.is_favorite {
                Icon::StarFilled
            } else {
                Icon::Star
            };
            if icon_button_sized(ui, star_icon, 22.0).clicked() {
                *preset_idx_to_toggle_favorite = Some(idx);
            }
            if presets.len() > 1 {
                if icon_button_sized(ui, Icon::Delete, 22.0).clicked() {
                    *preset_idx_to_delete = Some(idx);
                }
            }
        }
    });
}
