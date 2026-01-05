use crate::api::realtime_audio::{start_realtime_transcription, RealtimeState};
use crate::overlay::realtime_webview::state::*;
use crate::APP;
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

lazy_static::lazy_static! {
    pub static ref MINIMAL_ACTIVE: AtomicBool = AtomicBool::new(false);
    pub static ref MINIMAL_PRESET_IDX: AtomicUsize = AtomicUsize::new(0);
    static ref UI_STATE: Mutex<RealtimeUiState> = Mutex::new(RealtimeUiState::default());
    static ref USER_REQUESTED_CLOSE: AtomicBool = AtomicBool::new(false);
}

struct RealtimeUiState {
    font_size: f32,
    apps_list: Vec<(u32, String)>,
    show_transcription: bool,
    show_translation: bool,
    last_spoken_len: usize,
    show_app_picker: bool,
    show_tts_panel: bool,
    last_committed_len: usize,
    prev_window_size: egui::Vec2,
    prev_has_content: bool,
    committed_segments: Vec<String>,
}

impl Default for RealtimeUiState {
    fn default() -> Self {
        Self {
            font_size: 24.0,
            apps_list: Vec::new(),
            show_transcription: true,
            show_translation: true,
            last_spoken_len: 0,
            show_app_picker: false,
            show_tts_panel: false,
            last_committed_len: 0,
            prev_window_size: egui::Vec2::ZERO,
            prev_has_content: false,
            committed_segments: Vec::new(),
        }
    }
}

pub fn show_realtime_egui_overlay(preset_idx: usize) {
    if MINIMAL_ACTIVE.load(Ordering::SeqCst) || unsafe { IS_ACTIVE } {
        return;
    }

    unsafe {
        IS_ACTIVE = true;
        REALTIME_STOP_SIGNAL.store(false, Ordering::SeqCst);
        MIC_VISIBLE.store(true, Ordering::SeqCst);
        TRANS_VISIBLE.store(true, Ordering::SeqCst);
        AUDIO_SOURCE_CHANGE.store(false, Ordering::SeqCst);
        LANGUAGE_CHANGE.store(false, Ordering::SeqCst);
        TRANSLATION_MODEL_CHANGE.store(false, Ordering::SeqCst);

        {
            let mut state = REALTIME_STATE.lock().unwrap();
            *state = RealtimeState::new();
        }
    }
    
    LAST_SPOKEN_LENGTH.store(0, Ordering::SeqCst);
    REALTIME_TTS_ENABLED.store(false, Ordering::SeqCst);
    SELECTED_APP_PID.store(0, Ordering::SeqCst);
    if let Ok(mut name) = SELECTED_APP_NAME.lock() { name.clear(); }
    if let Ok(mut queue) = COMMITTED_TRANSLATION_QUEUE.lock() { queue.clear(); }
    USER_REQUESTED_CLOSE.store(false, Ordering::SeqCst);

    MINIMAL_ACTIVE.store(true, Ordering::SeqCst);
    MINIMAL_PRESET_IDX.store(preset_idx, Ordering::SeqCst);
    
    let app = APP.lock().unwrap();
    let preset = app.config.presets[preset_idx].clone();
    let font_size = app.config.realtime_font_size as f32;
    let config_language = app.config.realtime_target_language.clone();
    let config_audio_source = app.config.realtime_audio_source.clone();
    drop(app);
    
    let is_device_saved = config_audio_source == "device";
    
    if let Ok(mut ui_state) = UI_STATE.lock() {
        ui_state.font_size = font_size;
        ui_state.apps_list.clear();
        ui_state.show_transcription = true;
        ui_state.show_translation = true;
        ui_state.last_spoken_len = 0;
        ui_state.last_committed_len = 0;
        ui_state.show_app_picker = is_device_saved;
        ui_state.show_tts_panel = false;
        ui_state.prev_window_size = egui::Vec2::ZERO;
        ui_state.prev_has_content = false;
        ui_state.committed_segments.clear();
        // Don't lazy load apps here to avoid blocking
    }
    
    let effective_source = if config_audio_source.is_empty() { "device".to_string() } else { config_audio_source };
    
    if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
        *new_source = effective_source.clone();
    }
    
    if !config_language.is_empty() {
        if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
            *new_lang = config_language.clone();
        }
        LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
    }

    let mut final_preset = preset.clone();
    final_preset.audio_source = effective_source;

    start_realtime_transcription(
        final_preset,
        REALTIME_STOP_SIGNAL.clone(),
        windows::Win32::Foundation::HWND::default(),
        Some(windows::Win32::Foundation::HWND::default()), 
        REALTIME_STATE.clone(),
    );

    if let Ok(guard) = crate::gui::GUI_CONTEXT.lock() {
        if let Some(ctx) = guard.as_ref() {
            ctx.request_repaint();
        }
    }
}

pub fn render_minimal_overlay(ctx: &egui::Context) {
    if !MINIMAL_ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    
    if USER_REQUESTED_CLOSE.load(Ordering::SeqCst) {
        MINIMAL_ACTIVE.store(false, Ordering::SeqCst);
        unsafe { IS_ACTIVE = false; }
        REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
        crate::api::tts::TTS_MANAGER.stop();
        USER_REQUESTED_CLOSE.store(false, Ordering::SeqCst);
        return;
    }
    
    let mut ui_state = UI_STATE.lock().unwrap();
    let ui_language = APP.lock().map(|a| a.config.ui_language.clone()).unwrap_or_else(|_| "en".to_string());
    let title = crate::gui::settings_ui::get_localized_preset_name("preset_realtime_audio_translate", &ui_language);

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("minimal_realtime_overlay"),
        egui::ViewportBuilder::default()
            .with_inner_size([700.0, 200.0])  
            .with_title(title)
            .with_always_on_top(),
        |ctx, _class| {
             if ctx.input(|i| i.viewport().close_requested()) {
                 USER_REQUESTED_CLOSE.store(true, Ordering::SeqCst);
             }
             
             egui::CentralPanel::default().show(ctx, |ui| {
                  render_main_ui(ui, &mut ui_state);
             });
        }
    );
}

fn render_main_ui(ui: &mut egui::Ui, state: &mut RealtimeUiState) {
    let current_source = NEW_AUDIO_SOURCE.lock().map(|s| s.clone()).unwrap_or_else(|_| "mic".to_string());
    let is_device_mode = current_source == "device";
    let app_pid = SELECTED_APP_PID.load(Ordering::SeqCst);
    let tts_enabled = REALTIME_TTS_ENABLED.load(Ordering::SeqCst);
    let ui_language = APP.lock().map(|a| a.config.ui_language.clone()).unwrap_or_else(|_| "en".to_string());
    let locale = crate::gui::locale::LocaleText::get(&ui_language);
    
    // ===== HEADER BAR =====
    ui.horizontal(|ui| {
        // Warning Logic REPLACES Title
        if is_device_mode && app_pid == 0 && !state.show_app_picker {
             ui.label(egui::RichText::new(locale.device_mode_warning)
                .color(egui::Color32::from_rgb(255, 180, 100)).size(11.0));
             if ui.small_button(locale.select_app_btn).clicked() {
                state.show_app_picker = true;
                if state.apps_list.is_empty() {
                    state.apps_list = crate::overlay::realtime_webview::app_selection::enumerate_audio_apps();
                }
             }
        }
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Visibility toggles
            if ui.selectable_label(state.show_translation, "🌐").on_hover_text(locale.toggle_translation_tooltip).clicked() {
                state.show_translation = !state.show_translation;
                TRANS_VISIBLE.store(state.show_translation, Ordering::SeqCst);
                if !state.show_translation {
                    crate::api::tts::TTS_MANAGER.stop();
                }
                if !state.show_transcription && !state.show_translation {
                    USER_REQUESTED_CLOSE.store(true, Ordering::SeqCst);
                }
            }
            
            if ui.selectable_label(state.show_transcription, "📝").on_hover_text(locale.toggle_transcription_tooltip).clicked() {
                state.show_transcription = !state.show_transcription;
                MIC_VISIBLE.store(state.show_transcription, Ordering::SeqCst);
                if !state.show_transcription && !state.show_translation {
                    USER_REQUESTED_CLOSE.store(true, Ordering::SeqCst);
                }
            }
            
            ui.separator();
            
            // Font controls
            if ui.small_button("➖").on_hover_text(locale.font_minus_tooltip).clicked() {
                state.font_size = (state.font_size - 2.0).max(10.0);
                if let Ok(mut app) = APP.lock() {
                    app.config.realtime_font_size = state.font_size as u32;
                }
            }
            if ui.small_button("➕").on_hover_text(locale.font_plus_tooltip).clicked() {
                state.font_size = (state.font_size + 2.0).min(40.0);
                if let Ok(mut app) = APP.lock() {
                    app.config.realtime_font_size = state.font_size as u32;
                }
            }
            
            ui.separator();
            
            // TTS button
            if state.show_translation {
                let tts_label = if tts_enabled { "🔊" } else { "🔇" };
                if ui.small_button(tts_label).on_hover_text(locale.tts_settings_title).clicked() {
                    state.show_tts_panel = !state.show_tts_panel;
                }
                
                // Model toggle
                let current_model = APP.lock().map(|a| a.config.realtime_translation_model.clone()).unwrap_or_default();
                let model_label = match current_model.as_str() {
                    "google-gemma" => "✨",
                    "google-gtx" => "🌍",
                    _ => "🔥"
                };
                
                ui.menu_button(model_label, |ui| {
                    if ui.selectable_label(current_model == "cerebras-oss", "🔥 Cerebras").clicked() {
                        if let Ok(mut m) = NEW_TRANSLATION_MODEL.lock() { *m = "cerebras-oss".to_string(); }
                        TRANSLATION_MODEL_CHANGE.store(true, Ordering::SeqCst);
                        if let Ok(mut app) = APP.lock() { app.config.realtime_translation_model = "cerebras-oss".to_string(); }
                        ui.close();
                    }
                    if ui.selectable_label(current_model == "google-gemma", "✨ Gemma").clicked() {
                        if let Ok(mut m) = NEW_TRANSLATION_MODEL.lock() { *m = "google-gemma".to_string(); }
                        TRANSLATION_MODEL_CHANGE.store(true, Ordering::SeqCst);
                        if let Ok(mut app) = APP.lock() { app.config.realtime_translation_model = "google-gemma".to_string(); }
                        ui.close();
                    }
                    if ui.selectable_label(current_model == "google-gtx", format!("🌍 {}", locale.google_gtx_label)).clicked() {
                        if let Ok(mut m) = NEW_TRANSLATION_MODEL.lock() { *m = "google-gtx".to_string(); }
                        TRANSLATION_MODEL_CHANGE.store(true, Ordering::SeqCst);
                        if let Ok(mut app) = APP.lock() { app.config.realtime_translation_model = "google-gtx".to_string(); }
                        ui.close();
                    }
                });
                
                // Language selector
                let current_lang = NEW_TARGET_LANGUAGE.lock().map(|l| if l.is_empty() { "English".to_string() } else { l.clone() }).unwrap_or_else(|_| "English".to_string());
                let lang_code = isolang::Language::from_name(&current_lang)
                    .and_then(|l| l.to_639_1())
                    .map(|c| c.to_uppercase())
                    .unwrap_or_else(|| current_lang.chars().take(2).collect::<String>().to_uppercase());
                
                let btn_resp = ui.button(&lang_code);
                if btn_resp.clicked() {
                    egui::Popup::toggle_id(ui.ctx(), btn_resp.id);
                }
                let popup_id = btn_resp.id;
                
                egui::Popup::from_toggle_button_response(&btn_resp)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_min_width(120.0);
                        let search_id = egui::Id::new("realtime_lang_search");
                        let mut search_text: String = ui.data_mut(|d| d.get_temp(search_id).unwrap_or_default());
                        
                        let response = ui.add(egui::TextEdit::singleline(&mut search_text).hint_text("Search...").desired_width(120.0));
                        if response.changed() {
                            ui.data_mut(|d| d.insert_temp(search_id, search_text.clone()));
                        }
                        if response.clicked() {
                            response.request_focus();
                        }

                        ui.separator();

                        egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                            for lang in crate::config::get_all_languages() {
                                let matches = search_text.is_empty() || lang.to_lowercase().contains(&search_text.to_lowercase());
                                if matches {
                                    if ui.selectable_label(current_lang == *lang, lang).clicked() {
                                        if let Ok(mut l) = NEW_TARGET_LANGUAGE.lock() { *l = lang.to_string(); }
                                        LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
                                        if let Ok(mut app) = APP.lock() { app.config.realtime_target_language = lang.to_string(); }
                                        ui.data_mut(|d| d.remove_temp::<String>(search_id));
                                        
                                        egui::Popup::toggle_id(ui.ctx(), popup_id);
                                    }
                                }
                            }
                        });
                    });
            }
            
            ui.separator();
            
            // Audio source toggle
            if ui.selectable_label(!is_device_mode, "🎤").on_hover_text(locale.audio_src_mic).clicked() {
                if let Ok(mut s) = NEW_AUDIO_SOURCE.lock() { *s = "mic".to_string(); }
                SELECTED_APP_PID.store(0, Ordering::SeqCst);
                if let Ok(mut name) = SELECTED_APP_NAME.lock() { name.clear(); }
                AUDIO_SOURCE_CHANGE.store(true, Ordering::SeqCst);
                if let Ok(mut app) = APP.lock() { app.config.realtime_audio_source = "mic".to_string(); }
                state.show_app_picker = false;
            }
            
            if ui.selectable_label(is_device_mode, "🔊").on_hover_text(locale.audio_src_device).clicked() {
                state.show_app_picker = true;
                if state.apps_list.is_empty() {
                    state.apps_list = crate::overlay::realtime_webview::app_selection::enumerate_audio_apps();
                }
            }
        });
    });
    
    // ===== TTS PANEL =====
    if state.show_tts_panel && state.show_translation {
        ui.horizontal(|ui| {
            let can_enable_tts = !is_device_mode || app_pid > 0;
            let mut tts_on = tts_enabled;
            
            ui.add_enabled_ui(can_enable_tts, |ui| {
                if ui.checkbox(&mut tts_on, "TTS").changed() {
                    if tts_on {
                        REALTIME_TTS_ENABLED.store(true, Ordering::SeqCst);
                        if is_device_mode && app_pid == 0 {
                            state.show_app_picker = true;
                            if state.apps_list.is_empty() {
                                state.apps_list = crate::overlay::realtime_webview::app_selection::enumerate_audio_apps();
                            }
                        }
                    } else {
                        REALTIME_TTS_ENABLED.store(false, Ordering::SeqCst);
                        crate::api::tts::TTS_MANAGER.stop();
                        LAST_SPOKEN_LENGTH.store(0, Ordering::SeqCst);
                        state.last_spoken_len = 0;
                        if let Ok(mut queue) = COMMITTED_TRANSLATION_QUEUE.lock() { queue.clear(); }
                    }
                }
            });
            
            let current_speed = CURRENT_TTS_SPEED.load(Ordering::Relaxed);
            let base_speed = REALTIME_TTS_SPEED.load(Ordering::Relaxed);
            let auto_speed = REALTIME_TTS_AUTO_SPEED.load(Ordering::Relaxed);
            
            ui.label(format!("{:.1}x", current_speed as f32 / 100.0));
            
            let mut speed_val = base_speed as i32;
            if ui.add(egui::Slider::new(&mut speed_val, 50..=200).show_value(false)).changed() {
                REALTIME_TTS_SPEED.store(speed_val as u32, Ordering::SeqCst);
                REALTIME_TTS_AUTO_SPEED.store(false, Ordering::SeqCst);
            }
            
            let mut auto_on = auto_speed;
            if ui.checkbox(&mut auto_on, locale.realtime_tts_auto).changed() {
                REALTIME_TTS_AUTO_SPEED.store(auto_on, Ordering::SeqCst);
            }
        });
    }
    
    // ===== APP PICKER PANEL =====
    if state.show_app_picker {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(locale.app_select_title).strong().size(11.0));
            if ui.small_button("🔄").clicked() {
                state.apps_list = crate::overlay::realtime_webview::app_selection::enumerate_audio_apps();
            }
            if ui.small_button("✖").clicked() {
                state.show_app_picker = false;
            }
            let selected_name = SELECTED_APP_NAME.lock().map(|n| n.clone()).unwrap_or_default();
            if !selected_name.is_empty() {
                ui.label(egui::RichText::new(format!("✓ {}", selected_name)).color(egui::Color32::GREEN).size(10.0));
            }
        });
        
        if state.apps_list.is_empty() {
            state.apps_list = crate::overlay::realtime_webview::app_selection::enumerate_audio_apps();
        }
        
        egui::ScrollArea::vertical().max_height(80.0).id_salt("app_list").show(ui, |ui| {
            for (pid, name) in state.apps_list.clone() {
                let is_selected = app_pid == pid;
                let display = if name.chars().count() > 40 { 
                    format!("{}...", name.chars().take(37).collect::<String>()) 
                } else { 
                    name.clone() 
                };
                
                if ui.selectable_label(is_selected, &display).clicked() {
                    SELECTED_APP_PID.store(pid, Ordering::SeqCst);
                    if let Ok(mut app_name) = SELECTED_APP_NAME.lock() {
                        *app_name = name.clone();
                    }
                    // REALTIME_TTS_ENABLED.store(true, Ordering::SeqCst); // User requested removal
                    if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
                        *new_source = "device".to_string();
                    }
                    AUDIO_SOURCE_CHANGE.store(true, Ordering::SeqCst);
                    state.show_app_picker = false;
                }
            }
        });
    }
    
    // ===== CONTENT AREA =====
    let state_data = REALTIME_STATE.lock().unwrap();
    let font = egui::FontId::new(state.font_size, egui::FontFamily::Proportional);

    // TTS Logic
    if state.show_translation && TRANS_VISIBLE.load(Ordering::SeqCst) {
        let committed = &state_data.committed_translation;
        let old_len = committed.len();
        
        let is_mic_mode = current_source.is_empty() || current_source == "mic";
        let tts_allowed = is_mic_mode || app_pid > 0;
        
        // Re-read enabled state in case it changed in this frame
        let current_tts_enabled = REALTIME_TTS_ENABLED.load(Ordering::SeqCst);
        
        if current_tts_enabled && tts_allowed && !committed.is_empty() {
            if state.last_spoken_len == 0 && old_len > 50 {
                let text = committed.trim_end();
                let search_limit = text.len().saturating_sub(1);
                if search_limit > 0 {
                    if let Some(idx) = text[..search_limit].rfind(|c| c == '.' || c == '?' || c == '!' || c == '\n') {
                        state.last_spoken_len = idx + 1;
                    }
                }
            }

            if old_len > state.last_spoken_len {
                let new_committed = committed[state.last_spoken_len..].to_string();
                if !new_committed.trim().is_empty() {
                    if let Ok(mut queue) = COMMITTED_TRANSLATION_QUEUE.lock() {
                        queue.push_back(new_committed.clone());
                    }
                    let text_to_speak = new_committed;
                    std::thread::spawn(move || {
                        crate::api::tts::TTS_MANAGER.speak_realtime(&text_to_speak, 0);
                    });
                }
                state.last_spoken_len = old_len;
            }
        }
    }

    let (full_transcript, last_committed_pos, committed_translation, uncommitted_translation) = (
        state_data.full_transcript.clone(),
        state_data.last_committed_pos,
        state_data.committed_translation.clone(),
        state_data.uncommitted_translation.clone(),
    );
    drop(state_data);

    let available_height = ui.available_height();
    let rect = ui.ctx().input(|i| i.viewport().inner_rect);
    let current_window_size = rect.map(|r| r.size()).unwrap_or(egui::Vec2::ZERO);
    
    // logic: trigger scroll if committed text grows OR window resized OR content just appeared
    // logic: trigger scroll if committed text grows OR window resized OR content just appeared
    let current_len = committed_translation.len();
    
    if current_len < state.last_committed_len {
        // Reset detected (e.g. language switch or clear)
        state.committed_segments.clear();
        state.last_committed_len = 0;
    }
    
    let committed_grew = current_len > state.last_committed_len;
    
    if committed_grew {
        let new_segment = committed_translation[state.last_committed_len..].to_string();
        state.committed_segments.push(new_segment);
        state.last_committed_len = current_len;
    } else {
        // Ensure sync (should be equal)
        state.last_committed_len = current_len;
    }
    
    let window_resized = (current_window_size - state.prev_window_size).length() > 1.0;
    if window_resized {
        state.prev_window_size = current_window_size;
    }
    
    let has_content = !committed_translation.is_empty() || !uncommitted_translation.is_empty();
    let content_appeared = has_content && !state.prev_has_content;
    if has_content != state.prev_has_content {
        state.prev_has_content = has_content;
    }
    
    let should_scroll_trans = committed_grew || window_resized || content_appeared;

    // Render content
    if state.show_transcription && state.show_translation {
        let available_width = ui.available_width();
        let col_width = (available_width - 10.0) / 2.0;
        let content_height = available_height.max(50.0);
        
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(col_width);
                ui.set_min_height(content_height);
                egui::ScrollArea::vertical()
                    .id_salt("trans_scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        render_transcript(ui, &full_transcript, last_committed_pos, &font);
                    });
            });
            
            ui.separator();

            ui.vertical(|ui| {
                ui.set_width(col_width);
                ui.set_min_height(content_height);
                egui::ScrollArea::vertical()
                    .id_salt("transl_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        render_translation(ui, &state.committed_segments, &uncommitted_translation, &font);
                        if should_scroll_trans {
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                        }
                    });
            });
        });
    } else if state.show_transcription {
        egui::ScrollArea::vertical()
            .id_salt("trans_full")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                render_transcript(ui, &full_transcript, last_committed_pos, &font);
            });
    } else if state.show_translation {
        egui::ScrollArea::vertical()
            .id_salt("transl_full")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                 render_translation(ui, &state.committed_segments, &uncommitted_translation, &font);
                 if should_scroll_trans {
                     ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                 }
            });
    }
}

fn render_transcript(ui: &mut egui::Ui, full: &str, split_pos: usize, font: &egui::FontId) {
    let split_idx = split_pos.min(full.len());
    let split_idx = if full.is_char_boundary(split_idx) { split_idx } else {
        full.char_indices().take_while(|(i, _)| *i < split_idx).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0)
    };
    
    let committed = full[..split_idx].trim_end();
    let uncommitted = full[split_idx..].trim_start();
    let dark_mode = ui.visuals().dark_mode;
    
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if !committed.is_empty() {
             ui.label(egui::RichText::new(committed).font(font.clone()).color(get_text_color(true, dark_mode)));
        }
        if !uncommitted.is_empty() {
            if !committed.is_empty() { ui.label(" "); }
            let color = if dark_mode { egui::Color32::WHITE } else { egui::Color32::BLACK };
            ui.label(egui::RichText::new(uncommitted).font(font.clone()).color(color).italics());
        }
    });
}

fn render_translation(ui: &mut egui::Ui, segments: &[String], uncommitted: &str, font: &egui::FontId) {
    let uncommitted = uncommitted.trim_start();
    let dark_mode = ui.visuals().dark_mode;
    
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        
        for (i, segment) in segments.iter().enumerate() {
            let color = get_segment_color(i, dark_mode);
            ui.label(egui::RichText::new(segment).font(font.clone()).color(color));
        }
        
        if !uncommitted.is_empty() {
            // Uncommitted text color
            let color = if dark_mode { egui::Color32::YELLOW } else { egui::Color32::from_rgb(200, 100, 0) }; // Dark Orange for light mode
            ui.label(egui::RichText::new(uncommitted).font(font.clone()).color(color).italics());
        }
    });
}

// Helpers
#[allow(dead_code)]
fn tr(key: &str, lang: &str) -> String {
    match lang {
         "vi" => match key {
             "device_mode_warning" => "⚠ Đã chọn âm thanh thiết bị nhưng chưa chọn ứng dụng".to_string(),
             "select_app" => "Chọn ứng dụng".to_string(),
             "toggle_translation" => "Tắt/Mở dịch".to_string(),
             "toggle_transcription" => "Tắt/Mở phụ đề".to_string(),
             "font_minus" => "Giảm cỡ chữ".to_string(),
             "font_plus" => "Tăng cỡ chữ".to_string(),
             "tts_settings" => "Cài đặt đọc văn bản (TTS)".to_string(),
             "microphone" => "Microphone".to_string(),
             "system_audio" => "Âm thanh hệ thống".to_string(),
             "select_app_title" => "🎧 Chọn ứng dụng để thu âm".to_string(),
             "auto" => "Tự động".to_string(),
             _ => key.to_string(),
         },
         _ => match key {
             "device_mode_warning" => "⚠ Device audio selected but no app chosen".to_string(),
             "select_app" => "Select App".to_string(),
             "toggle_translation" => "Toggle Translation".to_string(),
             "toggle_transcription" => "Toggle Transcription".to_string(),
             "font_minus" => "Font -".to_string(),
             "font_plus" => "Font +".to_string(),
             "tts_settings" => "TTS Settings".to_string(),
             "microphone" => "Microphone".to_string(),
             "system_audio" => "System Audio".to_string(),
             "select_app_title" => "🎧 Select App to Record".to_string(),
             "auto" => "Auto".to_string(),
             _ => key.to_string(),
         }
    }
}

fn get_segment_color(index: usize, dark_mode: bool) -> egui::Color32 {
    if dark_mode {
        if index % 2 == 0 {
            egui::Color32::from_gray(230)
        } else {
            egui::Color32::from_rgb(180, 210, 255) // Light Blue
        }
    } else {
        if index % 2 == 0 {
            egui::Color32::from_gray(30) // Dark Gray (almost black) for readability
        } else {
            egui::Color32::from_rgb(0, 80, 200) // Deep Blue
        }
    }
}

fn get_text_color(is_committed: bool, dark_mode: bool) -> egui::Color32 {
    if dark_mode {
        if is_committed { egui::Color32::from_gray(200) } else { egui::Color32::WHITE }
    } else {
        if is_committed { egui::Color32::from_gray(60) } else { egui::Color32::BLACK }
    }
}
