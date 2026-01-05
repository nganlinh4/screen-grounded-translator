use super::node::ChainNode;
use super::utils::{insert_next_language_tag, model_supports_search, show_language_vars};
use super::viewer::ChainViewer;
use crate::gui::icons::{icon_button, Icon};
use crate::model_config::{
    get_all_models_with_ollama, get_model_by_id, is_ollama_scan_in_progress, model_is_non_llm,
    trigger_ollama_model_scan, ModelType,
};
use eframe::egui;
use egui_snarl::{NodeId, Snarl};

pub fn show_body(
    viewer: &mut ChainViewer,
    node_id: NodeId,
    ui: &mut egui::Ui,
    snarl: &mut Snarl<ChainNode>,
) {
    #[allow(deprecated)]
    {
        let mut auto_copy_triggered = false;
        let current_node_uuid = snarl
            .get_node(node_id)
            .map(|n| n.id().to_string())
            .unwrap_or_default();

        // Render Node UI
        {
            let node = snarl.get_node_mut(node_id).unwrap();

            ui.vertical(|ui| {
                ui.set_max_width(320.0);

                match node {
                    ChainNode::Input {
                        block_type,
                        auto_copy,
                        auto_speak,
                        show_overlay,
                        render_mode,
                        ..
                    } => {
                        ui.set_min_width(173.0);
                        // Input settings (Simplified) - Label removed to avoid duplication with header
                        // ui.separator() removed for compact look

                        // Determine actual input type
                        let actual_type = if block_type == "input_adapter" {
                            viewer.preset_type.as_str()
                        } else {
                            block_type.as_str()
                        };

                        // Eye button + Display mode row for Input nodes
                        ui.horizontal(|ui| {
                            // Eye icon toggle
                            let icon = if *show_overlay {
                                Icon::EyeOpen
                            } else {
                                Icon::EyeClosed
                            };
                            if icon_button(ui, icon).clicked() {
                                *show_overlay = !*show_overlay;
                                viewer.changed = true;

                                // When turning ON, auto-set render_mode based on input type
                                // Image/Audio: "markdown" (đẹp), Text: "plain" (thường)
                                if *show_overlay {
                                    *render_mode = if actual_type == "text" {
                                        "plain".to_string()
                                    } else {
                                        "markdown".to_string()
                                    };
                                }
                            }

                            if *show_overlay {
                                // Render Mode Dropdown for input display
                                // Text: Normal only (no streaming for input)
                                // Image/Audio: Normal or Markdown (đẹp)
                                let current_mode_label = if render_mode == "markdown" {
                                    match viewer.ui_language.as_str() {
                                        "vi" => "Đẹp",
                                        "ko" => "마크다운",
                                        _ => "Markdown",
                                    }
                                } else {
                                    match viewer.ui_language.as_str() {
                                        "vi" => "Thường",
                                        "ko" => "일반",
                                        _ => "Normal",
                                    }
                                };

                                let popup_id = ui.make_persistent_id(format!(
                                    "input_render_mode_popup_{:?}",
                                    node_id
                                ));
                                let btn = ui.add(
                                    egui::Button::new(current_mode_label)
                                        .fill(egui::Color32::from_rgba_unmultiplied(
                                            80, 80, 80, 180,
                                        ))
                                        .corner_radius(4.0),
                                );
                                if btn.clicked() {
                                    ui.memory_mut(|mem| mem.toggle_popup(popup_id));
                                }
                                egui::popup_below_widget(
                                    ui,
                                    popup_id,
                                    &btn,
                                    egui::PopupCloseBehavior::CloseOnClickOutside,
                                    |ui| {
                                        ui.set_min_width(60.0);
                                        let (lbl_norm, lbl_md) = match viewer.ui_language.as_str() {
                                            "vi" => ("Thường", "Đẹp"),
                                            "ko" => ("일반", "마크다운"),
                                            _ => ("Normal", "Markdown"),
                                        };

                                        if ui
                                            .selectable_label(render_mode == "plain", lbl_norm)
                                            .clicked()
                                        {
                                            *render_mode = "plain".to_string();
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                        if ui
                                            .selectable_label(render_mode == "markdown", lbl_md)
                                            .clicked()
                                        {
                                            *render_mode = "markdown".to_string();
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                    },
                                );
                            }
                        });

                        // Copy/Speak toggles for Input - Conditional based on Type
                        ui.horizontal(|ui| {
                            // Logic:
                            // Text Input: Show Both
                            // Image Input: Show Copy, Hide Speak
                            // Audio Input: Hide Both

                            let show_copy = actual_type != "audio"; // Hide for audio
                            let show_speak = actual_type == "text"; // Show only for text

                            if show_copy {
                                let is_text_input = actual_type == "text";

                                if is_text_input {
                                    // Enforce Auto-Copy ON for Text Input
                                    // Required for text extraction mechanism
                                    if !*auto_copy {
                                        *auto_copy = true;
                                        viewer.changed = true;
                                    }

                                    // Render as active Copy Icon, but ignore clicks (locked ON)
                                    // We don't disable the UI, so it looks full color/active.
                                    // We just don't react to .clicked() to toggle it off.
                                    let _ = icon_button(ui, Icon::Copy)
                                        .on_hover_text(viewer.text.input_auto_copy_tooltip);
                                } else {
                                    // Copy icon toggle for other inputs (Image, Audio, etc.)
                                    let copy_icon = if *auto_copy {
                                        Icon::Copy
                                    } else {
                                        Icon::CopyDisabled
                                    };
                                    if icon_button(ui, copy_icon)
                                        .on_hover_text(viewer.text.input_auto_copy_tooltip)
                                        .clicked()
                                    {
                                        *auto_copy = !*auto_copy;
                                        viewer.changed = true;
                                        if *auto_copy {
                                            auto_copy_triggered = true;
                                        }
                                    }
                                }
                            }

                            if show_speak {
                                // Speak icon toggle
                                let speak_icon = if *auto_speak {
                                    Icon::Speaker
                                } else {
                                    Icon::SpeakerDisabled
                                };
                                if icon_button(ui, speak_icon)
                                    .on_hover_text(viewer.text.input_auto_speak_tooltip)
                                    .clicked()
                                {
                                    *auto_speak = !*auto_speak;
                                    viewer.changed = true;
                                }
                            }
                        });
                    }
                    ChainNode::Special {
                        model,
                        prompt,
                        language_vars,
                        show_overlay,
                        streaming_enabled,
                        render_mode,
                        auto_copy,
                        auto_speak,
                        ..
                    } => {
                        // Special nodes use different model types based on preset type
                        let target_model_type = match viewer.preset_type.as_str() {
                            "image" => ModelType::Vision,
                            "audio" => ModelType::Audio,
                            _ => ModelType::Text,
                        };

                        // Row 1: Model
                        let model_label = match viewer.ui_language.as_str() {
                            "vi" => "Mô hình:",
                            "ko" => "모델:",
                            _ => "Model:",
                        };
                        ui.horizontal(|ui| {
                            ui.label(model_label);
                            let model_def = get_model_by_id(model);
                            let display_name = model_def
                                .as_ref()
                                .map(|m| match viewer.ui_language.as_str() {
                                    "vi" => m.name_vi.as_str(),
                                    "ko" => m.name_ko.as_str(),
                                    _ => m.name_en.as_str(),
                                })
                                .unwrap_or(model.as_str());

                            // Model selector button with manual popup for tight width

                            let button_response = ui.button(display_name);
                            if button_response.clicked() {
                                egui::Popup::toggle_id(ui.ctx(), button_response.id);
                                // Trigger background scan when popup opens
                                if viewer.use_ollama {
                                    trigger_ollama_model_scan();
                                }
                            }
                            let popup_layer_id = button_response.id;
                            egui::Popup::from_toggle_button_response(&button_response).show(|ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend); // No text wrapping, auto width

                                // Show Ollama loading indicator if scanning
                                if viewer.use_ollama && is_ollama_scan_in_progress() {
                                    let loading_text = match viewer.ui_language.as_str() {
                                        "vi" => "⏳ Đang quét các model local...",
                                        "ko" => "⏳ 로컬 모델 스캔 중...",
                                        _ => "⏳ Scanning local models...",
                                    };
                                    ui.label(egui::RichText::new(loading_text).weak().italics());
                                    ui.separator();
                                }

                                for m in get_all_models_with_ollama() {
                                    if m.enabled
                                        && m.model_type == target_model_type
                                        && viewer.is_provider_enabled(&m.provider)
                                    {
                                        let name = match viewer.ui_language.as_str() {
                                            "vi" => &m.name_vi,
                                            "ko" => &m.name_ko,
                                            _ => &m.name_en,
                                        };
                                        let quota = match viewer.ui_language.as_str() {
                                            "vi" => &m.quota_limit_vi,
                                            "ko" => &m.quota_limit_ko,
                                            _ => &m.quota_limit_en,
                                        };
                                        let provider_icon = match m.provider.as_str() {
                                            "google" => "💠 ",
                                            "google-gtx" => "🌍 ",
                                            "groq" => "⚡ ",
                                            "cerebras" => "🔥 ",
                                            "openrouter" => "🌐 ",
                                            "ollama" => "🏠 ",
                                            "qrserver" => "🔳 ",
                                            _ => "⚙️ ",
                                        };
                                        let search_suffix = if model_supports_search(&m.id) {
                                            " 🔍"
                                        } else {
                                            ""
                                        };
                                        let label = format!(
                                            "{}{} - {} - {}{}",
                                            provider_icon, name, m.full_name, quota, search_suffix
                                        );
                                        let is_selected = *model == m.id;

                                        if ui.selectable_label(is_selected, label).clicked() {
                                            *model = m.id.clone();
                                            viewer.changed = true;
                                            egui::Popup::toggle_id(ui.ctx(), popup_layer_id);
                                        }
                                    }
                                }
                            });
                        });

                        // Only show prompt UI for LLM models (not QR scanner, GTX, Whisper, etc.)
                        if !model_is_non_llm(model) {
                            // Row 2: Prompt Label + Add Tag Button
                            ui.horizontal(|ui| {
                                let prompt_label = match viewer.ui_language.as_str() {
                                    "vi" => "Lệnh:",
                                    "ko" => "프롬프트:",
                                    _ => "Prompt:",
                                };
                                ui.label(prompt_label);

                                let btn_label = match viewer.ui_language.as_str() {
                                    "vi" => "+ Ngôn ngữ",
                                    "ko" => "+ 언어",
                                    _ => "+ Language",
                                };
                                let is_dark = ui.visuals().dark_mode;
                                let lang_btn_bg = if is_dark {
                                    egui::Color32::from_rgb(50, 100, 110)
                                } else {
                                    egui::Color32::from_rgb(100, 160, 170)
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(btn_label)
                                                .small()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(lang_btn_bg)
                                        .corner_radius(8.0),
                                    )
                                    .clicked()
                                {
                                    insert_next_language_tag(prompt, language_vars);
                                    viewer.changed = true;
                                }
                            });

                            // Row 3: Prompt TextEdit
                            if ui
                                .add(
                                    egui::TextEdit::multiline(prompt)
                                        .desired_width(152.0)
                                        .desired_rows(2),
                                )
                                .changed()
                            {
                                viewer.changed = true;
                            }

                            // Row 4+: Language Variables
                            show_language_vars(
                                ui,
                                &viewer.ui_language,
                                prompt,
                                language_vars,
                                &mut viewer.changed,
                                &mut viewer.language_search,
                            );
                        }

                        // Bottom Row: Settings
                        ui.horizontal(|ui| {
                            let icon = if *show_overlay {
                                Icon::EyeOpen
                            } else {
                                Icon::EyeClosed
                            };
                            if icon_button(ui, icon).clicked() {
                                *show_overlay = !*show_overlay;
                                viewer.changed = true;
                            }

                            if *show_overlay {
                                // Render Mode Dropdown (Normal, Stream, Markdown) - using button+popup
                                let current_mode_label =
                                    match (render_mode.as_str(), *streaming_enabled) {
                                        ("markdown", _) => match viewer.ui_language.as_str() {
                                            "vi" => "Đẹp",
                                            "ko" => "마크다운",
                                            _ => "Markdown",
                                        },
                                        (_, true) => match viewer.ui_language.as_str() {
                                            "vi" => "Stream",
                                            "ko" => "스트림",
                                            _ => "Stream",
                                        },
                                        (_, false) => match viewer.ui_language.as_str() {
                                            "vi" => "Thường",
                                            "ko" => "일반",
                                            _ => "Normal",
                                        },
                                    };

                                let popup_id = ui
                                    .make_persistent_id(format!("render_mode_popup_{:?}", node_id));
                                let btn = ui.add(
                                    egui::Button::new(current_mode_label)
                                        .fill(egui::Color32::from_rgba_unmultiplied(
                                            80, 80, 80, 180,
                                        ))
                                        .corner_radius(4.0),
                                );
                                if btn.clicked() {
                                    ui.memory_mut(|mem| mem.toggle_popup(popup_id));
                                }
                                egui::popup_below_widget(
                                    ui,
                                    popup_id,
                                    &btn,
                                    egui::PopupCloseBehavior::CloseOnClickOutside,
                                    |ui| {
                                        ui.set_min_width(60.0);
                                        let (lbl_norm, lbl_stm, lbl_md) =
                                            match viewer.ui_language.as_str() {
                                                "vi" => ("Thường", "Stream", "Đẹp"),
                                                "ko" => ("일반", "스트림", "마크다운"),
                                                _ => ("Normal", "Stream", "Markdown"),
                                            };

                                        if ui
                                            .selectable_label(
                                                render_mode == "plain" && !*streaming_enabled,
                                                lbl_norm,
                                            )
                                            .clicked()
                                        {
                                            *render_mode = "plain".to_string();
                                            *streaming_enabled = false;
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                        if ui
                                            .selectable_label(
                                                (render_mode == "stream" || render_mode == "plain")
                                                    && *streaming_enabled,
                                                lbl_stm,
                                            )
                                            .clicked()
                                        {
                                            *render_mode = "stream".to_string();
                                            *streaming_enabled = true;
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                        if ui
                                            .selectable_label(render_mode == "markdown", lbl_md)
                                            .clicked()
                                        {
                                            *render_mode = "markdown".to_string();
                                            *streaming_enabled = false;
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                    },
                                );
                            }

                            let show_copy = true;
                            let show_speak = true;

                            // Copy icon toggle
                            if show_copy {
                                // Copy icon toggle
                                let copy_icon = if *auto_copy {
                                    Icon::Copy
                                } else {
                                    Icon::CopyDisabled
                                };
                                if icon_button(ui, copy_icon)
                                    .on_hover_text(viewer.text.input_auto_copy_tooltip)
                                    .clicked()
                                {
                                    *auto_copy = !*auto_copy;
                                    viewer.changed = true;
                                    if *auto_copy {
                                        auto_copy_triggered = true;
                                    }
                                }
                            }

                            if show_speak {
                                // Speak icon toggle
                                let speak_icon = if *auto_speak {
                                    Icon::Speaker
                                } else {
                                    Icon::SpeakerDisabled
                                };
                                if icon_button(ui, speak_icon)
                                    .on_hover_text(viewer.text.input_auto_speak_tooltip)
                                    .clicked()
                                {
                                    *auto_speak = !*auto_speak;
                                    viewer.changed = true;
                                }
                            }
                        });
                    }
                    ChainNode::Process {
                        model,
                        prompt,
                        language_vars,
                        show_overlay,
                        streaming_enabled,
                        render_mode,
                        auto_copy,
                        auto_speak,
                        ..
                    } => {
                        // Process nodes always use Text models (text-to-text transformation)
                        let target_model_type = ModelType::Text;

                        // Row 1: Model
                        let model_label = match viewer.ui_language.as_str() {
                            "vi" => "Mô hình:",
                            "ko" => "모델:",
                            _ => "Model:",
                        };
                        ui.horizontal(|ui| {
                            ui.label(model_label);
                            let model_def = get_model_by_id(model);
                            let display_name = model_def
                                .as_ref()
                                .map(|m| match viewer.ui_language.as_str() {
                                    "vi" => m.name_vi.as_str(),
                                    "ko" => m.name_ko.as_str(),
                                    _ => m.name_en.as_str(),
                                })
                                .unwrap_or(model.as_str());

                            let button_response = ui.button(display_name);
                            if button_response.clicked() {
                                egui::Popup::toggle_id(ui.ctx(), button_response.id);
                                if viewer.use_ollama {
                                    trigger_ollama_model_scan();
                                }
                            }
                            let popup_layer_id = button_response.id;
                            egui::Popup::from_toggle_button_response(&button_response).show(|ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                                if viewer.use_ollama && is_ollama_scan_in_progress() {
                                    let loading_text = match viewer.ui_language.as_str() {
                                        "vi" => "⏳ Đang quét các model local...",
                                        "ko" => "⏳ 로컬 모델 스캔 중...",
                                        _ => "⏳ Scanning local models...",
                                    };
                                    ui.label(egui::RichText::new(loading_text).weak().italics());
                                    ui.separator();
                                }

                                for m in get_all_models_with_ollama() {
                                    if m.enabled
                                        && m.model_type == target_model_type
                                        && viewer.is_provider_enabled(&m.provider)
                                    {
                                        let name = match viewer.ui_language.as_str() {
                                            "vi" => &m.name_vi,
                                            "ko" => &m.name_ko,
                                            _ => &m.name_en,
                                        };
                                        let quota = match viewer.ui_language.as_str() {
                                            "vi" => &m.quota_limit_vi,
                                            "ko" => &m.quota_limit_ko,
                                            _ => &m.quota_limit_en,
                                        };
                                        let provider_icon = match m.provider.as_str() {
                                            "google" => "💠 ",
                                            "google-gtx" => "🌍 ",
                                            "groq" => "⚡ ",
                                            "cerebras" => "🔥 ",
                                            "openrouter" => "🌐 ",
                                            "ollama" => "🏠 ",
                                            "qrserver" => "🔳 ",
                                            _ => "⚙️ ",
                                        };
                                        let search_suffix = if model_supports_search(&m.id) {
                                            " 🔍"
                                        } else {
                                            ""
                                        };
                                        let label = format!(
                                            "{}{} - {} - {}{}",
                                            provider_icon, name, m.full_name, quota, search_suffix
                                        );
                                        let is_selected = *model == m.id;

                                        if ui.selectable_label(is_selected, label).clicked() {
                                            *model = m.id.clone();
                                            viewer.changed = true;
                                            egui::Popup::toggle_id(ui.ctx(), popup_layer_id);
                                        }
                                    }
                                }
                            });
                        });

                        // Only show prompt UI for LLM models (not GTX, etc.)
                        if !model_is_non_llm(model) {
                            // Row 2: Prompt Label + Add Tag Button
                            ui.horizontal(|ui| {
                                let prompt_label = match viewer.ui_language.as_str() {
                                    "vi" => "Lệnh:",
                                    "ko" => "프롬프트:",
                                    _ => "Prompt:",
                                };
                                ui.label(prompt_label);

                                let btn_label = match viewer.ui_language.as_str() {
                                    "vi" => "+ Ngôn ngữ",
                                    "ko" => "+ 언어",
                                    _ => "+ Language",
                                };
                                let is_dark = ui.visuals().dark_mode;
                                let lang_btn_bg = if is_dark {
                                    egui::Color32::from_rgb(50, 100, 110)
                                } else {
                                    egui::Color32::from_rgb(100, 160, 170)
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(btn_label)
                                                .small()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(lang_btn_bg)
                                        .corner_radius(8.0),
                                    )
                                    .clicked()
                                {
                                    insert_next_language_tag(prompt, language_vars);
                                    viewer.changed = true;
                                }
                            });

                            // Row 3: Prompt TextEdit
                            if ui
                                .add(
                                    egui::TextEdit::multiline(prompt)
                                        .desired_width(152.0)
                                        .desired_rows(2),
                                )
                                .changed()
                            {
                                viewer.changed = true;
                            }

                            // Row 4+: Language Variables
                            show_language_vars(
                                ui,
                                &viewer.ui_language,
                                prompt,
                                language_vars,
                                &mut viewer.changed,
                                &mut viewer.language_search,
                            );
                        }

                        // Bottom Row: Settings
                        ui.horizontal(|ui| {
                            let icon = if *show_overlay {
                                Icon::EyeOpen
                            } else {
                                Icon::EyeClosed
                            };
                            if icon_button(ui, icon).clicked() {
                                *show_overlay = !*show_overlay;
                                viewer.changed = true;
                            }

                            if *show_overlay {
                                let current_mode_label =
                                    match (render_mode.as_str(), *streaming_enabled) {
                                        ("markdown", _) => match viewer.ui_language.as_str() {
                                            "vi" => "Đẹp",
                                            "ko" => "마크다운",
                                            _ => "Markdown",
                                        },
                                        (_, true) => match viewer.ui_language.as_str() {
                                            "vi" => "Stream",
                                            "ko" => "스트림",
                                            _ => "Stream",
                                        },
                                        (_, false) => match viewer.ui_language.as_str() {
                                            "vi" => "Thường",
                                            "ko" => "일반",
                                            _ => "Normal",
                                        },
                                    };

                                let popup_id = ui
                                    .make_persistent_id(format!("render_mode_popup_{:?}", node_id));
                                let btn = ui.add(
                                    egui::Button::new(current_mode_label)
                                        .fill(egui::Color32::from_rgba_unmultiplied(
                                            80, 80, 80, 180,
                                        ))
                                        .corner_radius(4.0),
                                );
                                if btn.clicked() {
                                    ui.memory_mut(|mem| mem.toggle_popup(popup_id));
                                }
                                egui::popup_below_widget(
                                    ui,
                                    popup_id,
                                    &btn,
                                    egui::PopupCloseBehavior::CloseOnClickOutside,
                                    |ui| {
                                        ui.set_min_width(60.0);
                                        let (lbl_norm, lbl_stm, lbl_md) =
                                            match viewer.ui_language.as_str() {
                                                "vi" => ("Thường", "Stream", "Đẹp"),
                                                "ko" => ("일반", "스트림", "마크다운"),
                                                _ => ("Normal", "Stream", "Markdown"),
                                            };

                                        if ui
                                            .selectable_label(
                                                render_mode == "plain" && !*streaming_enabled,
                                                lbl_norm,
                                            )
                                            .clicked()
                                        {
                                            *render_mode = "plain".to_string();
                                            *streaming_enabled = false;
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                        if ui
                                            .selectable_label(
                                                (render_mode == "stream" || render_mode == "plain")
                                                    && *streaming_enabled,
                                                lbl_stm,
                                            )
                                            .clicked()
                                        {
                                            *render_mode = "stream".to_string();
                                            *streaming_enabled = true;
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                        if ui
                                            .selectable_label(render_mode == "markdown", lbl_md)
                                            .clicked()
                                        {
                                            *render_mode = "markdown".to_string();
                                            *streaming_enabled = false;
                                            viewer.changed = true;
                                            ui.memory_mut(|mem| mem.close_popup(popup_id));
                                        }
                                    },
                                );
                            }

                            let show_copy = true;
                            let show_speak = true;

                            if show_copy {
                                let copy_icon = if *auto_copy {
                                    Icon::Copy
                                } else {
                                    Icon::CopyDisabled
                                };
                                if icon_button(ui, copy_icon)
                                    .on_hover_text(viewer.text.input_auto_copy_tooltip)
                                    .clicked()
                                {
                                    *auto_copy = !*auto_copy;
                                    viewer.changed = true;
                                    if *auto_copy {
                                        auto_copy_triggered = true;
                                    }
                                }
                            }

                            if show_speak {
                                let speak_icon = if *auto_speak {
                                    Icon::Speaker
                                } else {
                                    Icon::SpeakerDisabled
                                };
                                if icon_button(ui, speak_icon)
                                    .on_hover_text(viewer.text.input_auto_speak_tooltip)
                                    .clicked()
                                {
                                    *auto_speak = !*auto_speak;
                                    viewer.changed = true;
                                }
                            }
                        });
                    }
                }
            });
        }

        // Enforce auto-copy exclusivity
        if auto_copy_triggered {
            for node in snarl.nodes_mut() {
                if node.id() != current_node_uuid {
                    node.set_auto_copy(false);
                }
            }
        }
    }
}
