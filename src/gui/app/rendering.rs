use super::types::SettingsApp;
use crate::gui::locale::LocaleText;
use crate::gui::settings_ui::node_graph::{blocks_to_snarl, snarl_to_graph};
use crate::gui::settings_ui::{
    render_footer, render_global_settings, render_history_panel, render_preset_editor,
    render_sidebar, ViewMode,
};
use eframe::egui;

impl SettingsApp {
    pub(crate) fn render_footer_and_tips_modal(&mut self, ctx: &egui::Context) {
        let text = LocaleText::get(&self.config.ui_language);
        let visuals = ctx.style().visuals.clone();
        let footer_bg = if visuals.dark_mode {
            egui::Color32::from_gray(20)
        } else {
            egui::Color32::from_gray(240)
        };

        // Determine current tip text for footer
        let current_tip = text
            .tips_list
            .get(self.current_tip_idx)
            .unwrap_or(&"")
            .to_string();

        egui::TopBottomPanel::bottom("footer_panel")
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::default()
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .fill(footer_bg),
            )
            .show(ctx, |ui| {
                render_footer(
                    ui,
                    &text,
                    current_tip.clone(),
                    self.tip_fade_state,
                    &mut self.show_tips_modal,
                );
            });

        // [TIPS POPUP]
        let tips_popup_id = egui::Id::new("tips_popup_modal");

        if self.show_tips_modal {
            // Register this as an open popup so any_popup_open() returns true
            egui::Popup::open_id(ctx, tips_popup_id);

            let tips_list_copy = text.tips_list.clone();
            let tips_title = text.tips_title;
            let screen_rect = ctx.input(|i| {
                i.viewport().inner_rect.unwrap_or(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::ZERO,
                ))
            });

            // Dark semi-transparent backdrop
            let backdrop_layer =
                egui::LayerId::new(egui::Order::Middle, egui::Id::new("tips_backdrop"));
            let backdrop_painter = ctx.layer_painter(backdrop_layer);
            backdrop_painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(120));

            // Popup area centered on screen
            egui::Area::new(tips_popup_id)
                .order(egui::Order::Tooltip) // High priority layer
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_max_width(750.0);

                            // Header with title and close button
                            ui.horizontal(|ui| {
                                ui.heading(tips_title);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::gui::icons::icon_button(
                                            ui,
                                            crate::gui::icons::Icon::Close,
                                        )
                                        .clicked()
                                        {
                                            self.show_tips_modal = false;
                                        }
                                    },
                                );
                            });
                            ui.separator();
                            ui.add_space(8.0);

                            // Scrollable tips list
                            egui::ScrollArea::vertical()
                                .max_height(450.0)
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    for (i, tip) in tips_list_copy.iter().enumerate() {
                                        ui.label(
                                            egui::RichText::new(*tip)
                                                .size(13.0)
                                                .line_height(Some(18.0)),
                                        );
                                        if i < tips_list_copy.len() - 1 {
                                            ui.add_space(8.0);
                                            ui.separator();
                                            ui.add_space(8.0);
                                        }
                                    }
                                });
                        });
                });

            // Close on click outside (check if clicked outside the popup area)
            if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    // Check if click is on the backdrop (outside popup content)
                    if let Some(layer) = ctx.layer_id_at(pos) {
                        if layer.id == egui::Id::new("tips_backdrop") {
                            self.show_tips_modal = false;
                        }
                    }
                }
            }

            // Close on Escape
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.show_tips_modal = false;
            }
        }
    }

    pub(crate) fn render_main_layout(&mut self, ctx: &egui::Context) {
        let text = LocaleText::get(&self.config.ui_language);
        egui::CentralPanel::default().show(ctx, |ui| {
            let available_width = ui.available_width();
            let left_width = available_width * 0.35;
            let right_width = available_width * 0.65;

            ui.horizontal(|ui| {
                // Left Sidebar
                ui.allocate_ui_with_layout(
                    egui::vec2(left_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if render_sidebar(ui, &mut self.config, &mut self.view_mode, &text) {
                            self.save_and_sync();
                        }
                    },
                );

                ui.add_space(10.0);

                // Right Detail View
                ui.allocate_ui_with_layout(
                    egui::vec2(right_width - 20.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        match self.view_mode {
                            ViewMode::Global => {
                                let usage_stats = {
                                    let app = self.app_state_ref.lock().unwrap();
                                    app.model_usage_stats.clone()
                                };
                                if render_global_settings(
                                    ui,
                                    &mut self.config,
                                    &mut self.show_api_key,
                                    &mut self.show_gemini_api_key,
                                    &mut self.show_openrouter_api_key,
                                    &usage_stats,
                                    &self.updater,
                                    &self.update_status,
                                    &mut self.run_at_startup,
                                    &self.auto_launcher,
                                    self.current_admin_state, // <-- Pass current admin state
                                    &text,
                                    &mut self.show_usage_modal,
                                    &mut self.show_tts_modal,
                                    &self.cached_audio_devices,
                                ) {
                                    self.save_and_sync();
                                }
                            }
                            ViewMode::History => {
                                let history_manager = {
                                    let app = self.app_state_ref.lock().unwrap();
                                    app.history.clone()
                                };
                                if render_history_panel(
                                    ui,
                                    &mut self.config,
                                    &history_manager,
                                    &mut self.search_query,
                                    &text,
                                ) {
                                    self.save_and_sync();
                                }
                            }
                            ViewMode::Preset(idx) => {
                                // Sync snarl state if switching presets or first load
                                if self.last_edited_preset_idx != Some(idx) {
                                    if idx < self.config.presets.len() {
                                        self.snarl = Some(blocks_to_snarl(
                                            &self.config.presets[idx].blocks,
                                            &self.config.presets[idx].block_connections,
                                            &self.config.presets[idx].preset_type,
                                        ));
                                        self.last_edited_preset_idx = Some(idx);
                                    }
                                }

                                if let Some(snarl) = &mut self.snarl {
                                    if render_preset_editor(
                                        ui,
                                        &mut self.config,
                                        idx,
                                        &mut self.search_query,
                                        &mut self.cached_monitors,
                                        &mut self.recording_hotkey_for_preset,
                                        &self.hotkey_conflict_msg,
                                        &text,
                                        snarl,
                                    ) {
                                        // Sync back to blocks and connections
                                        if idx < self.config.presets.len() {
                                            let (blocks, connections) = snarl_to_graph(snarl);
                                            self.config.presets[idx].blocks = blocks;
                                            self.config.presets[idx].block_connections =
                                                connections;
                                        }
                                        self.save_and_sync();
                                    }
                                }
                            }
                        }
                    },
                );
            });
        });
    }

    pub(crate) fn render_fade_overlay(&mut self, ctx: &egui::Context) {
        if let Some(start_time) = self.fade_in_start {
            let elapsed = ctx.input(|i| i.time) - start_time;
            if elapsed < 0.6 {
                let opacity = 1.0 - (elapsed / 0.6) as f32;
                let rect = ctx.input(|i| {
                    i.viewport().inner_rect.unwrap_or(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::ZERO,
                    ))
                });
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("fade_overlay"),
                ));
                painter.rect_filled(
                    rect,
                    0.0,
                    eframe::egui::Color32::from_black_alpha((opacity * 255.0) as u8),
                );
                ctx.request_repaint();
            } else {
                self.fade_in_start = None;
            }
        }
    }
}
