use super::types::SettingsApp;
use crate::gui::locale::LocaleText;
use crate::gui::settings_ui::node_graph::{blocks_to_snarl, snarl_to_graph};
use crate::gui::settings_ui::{
    render_footer, render_global_settings, render_history_panel, render_preset_editor,
    render_sidebar, ViewMode,
};
use eframe::egui;
use egui::text::{LayoutJob, TextFormat};

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

            // Dark semi-transparent backdrop (disabled)
            // let backdrop_layer =
            //     egui::LayerId::new(egui::Order::Middle, egui::Id::new("tips_backdrop"));
            // let backdrop_painter = ctx.layer_painter(backdrop_layer);
            // backdrop_painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(120));

            // Popup area centered on screen
            egui::Area::new(tips_popup_id)
                .order(egui::Order::Tooltip) // High priority layer
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_max_width(1000.0);
                            ui.set_max_height(550.0);

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
                                .max_height(400.0)
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    for (i, tip) in tips_list_copy.iter().enumerate() {
                                        let is_dark_mode = ctx.style().visuals.dark_mode;
                                        let layout_job =
                                            format_tip_with_bold(i + 1, tip, is_dark_mode);
                                        ui.label(layout_job);
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

        // Help assistant is now handled via TextInput overlay (show_help_input)
        // No egui modal rendering needed
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

    /// Render a drop overlay when files are being dragged over the window
    pub(crate) fn render_drop_overlay(&mut self, ctx: &egui::Context) {
        use super::input_handler::is_files_hovered;
        use crate::gui::locale::LocaleText;

        // --- ANIMATION LOGIC ---
        let delta = ctx.input(|i| i.stable_dt).min(0.1);
        let is_hovered = is_files_hovered(ctx);
        let fade_speed = 8.0_f32;

        if is_hovered {
            self.drop_overlay_fade += fade_speed * delta;
        } else {
            self.drop_overlay_fade -= fade_speed * delta;
        }
        self.drop_overlay_fade = self.drop_overlay_fade.clamp(0.0, 1.0);

        // If completely invisible and not hovered, do nothing
        if self.drop_overlay_fade <= 0.0 {
            return;
        }

        // Keep repainting while animating
        if self.drop_overlay_fade > 0.0 && self.drop_overlay_fade < 1.0 {
            ctx.request_repaint();
        } else if is_hovered {
            ctx.request_repaint(); // Animate bobbing
        }

        // --- RENDER ---
        let text = LocaleText::get(&self.config.ui_language);
        let screen_rect = ctx.available_rect();

        // Overlay layer (Debug order to stay on top)
        let overlay_layer = egui::LayerId::new(egui::Order::Debug, egui::Id::new("drop_overlay"));
        let painter = ctx.layer_painter(overlay_layer);

        // Backdrop with fade
        let max_alpha = 180;
        let alpha = (max_alpha as f32 * self.drop_overlay_fade) as u8;
        let backdrop_color = egui::Color32::from_rgba_unmultiplied(0, 120, 215, alpha);
        painter.rect_filled(screen_rect, 0.0, backdrop_color);

        // Content opacity
        let content_opacity = self.drop_overlay_fade;
        let element_color = egui::Color32::from_white_alpha((255.0_f32 * content_opacity) as u8);

        // Dashed border with pulse
        let inset = 24.0;
        let inner_rect = screen_rect.shrink(inset);
        let time = ctx.input(|i| i.time);
        let pulse = (time * 2.5).sin() as f32 * 0.2_f32 + 0.8_f32;
        let border_alpha = (255.0_f32 * content_opacity * pulse) as u8;
        let border_color = egui::Color32::from_white_alpha(border_alpha);
        let stroke = egui::Stroke::new(3.0, border_color);

        let dash_length = 12.0;
        let gap_length = 8.0;

        // Helper to draw dashed line
        let draw_dashed_line = |p1: egui::Pos2, p2: egui::Pos2| {
            let vec = p2 - p1;
            let len = vec.length();
            let dir = vec / len;
            let count = (len / (dash_length + gap_length)).ceil() as i32;

            for i in 0..count {
                let start = p1 + dir * (i as f32 * (dash_length + gap_length));
                let end = start + dir * dash_length;
                let end = if (end - p1).length() > len { p2 } else { end };
                painter.line_segment([start, end], stroke);
            }
        };

        draw_dashed_line(inner_rect.left_top(), inner_rect.right_top());
        draw_dashed_line(inner_rect.right_top(), inner_rect.right_bottom());
        draw_dashed_line(inner_rect.right_bottom(), inner_rect.left_bottom());
        draw_dashed_line(inner_rect.left_bottom(), inner_rect.left_top());

        // Center content
        let center = screen_rect.center();
        let icon_size = 64.0;

        // Bobbing animation
        let bob_offset = (time * 5.0).sin() as f32 * 4.0_f32;

        // Draw Rounded Document Icon
        let file_width = icon_size * 0.7;
        let file_height = icon_size * 0.9;
        let file_rect = egui::Rect::from_center_size(center, egui::vec2(file_width, file_height));

        painter.rect_stroke(
            file_rect,
            8.0_f32,
            egui::Stroke::new(3.0, element_color),
            egui::StrokeKind::Middle,
        );

        // Draw Arrow (Bobbing inside)
        let arrow_center = center + egui::vec2(0.0, bob_offset);
        let arrow_len = icon_size * 0.4;
        let arrow_start = arrow_center - egui::vec2(0.0, arrow_len * 0.5);
        let arrow_end = arrow_center + egui::vec2(0.0, arrow_len * 0.5);

        let arrow_stroke = egui::Stroke::new(4.0, element_color);
        painter.line_segment([arrow_start, arrow_end], arrow_stroke);

        let arrow_head_size = 10.0;
        painter.line_segment(
            [
                arrow_end,
                arrow_end + egui::vec2(-arrow_head_size, -arrow_head_size),
            ],
            arrow_stroke,
        );
        painter.line_segment(
            [
                arrow_end,
                arrow_end + egui::vec2(arrow_head_size, -arrow_head_size),
            ],
            arrow_stroke,
        );

        // Text below
        let text_offset_y = icon_size * 0.8;
        let text_pos = center + egui::vec2(0.0, text_offset_y);
        let galley = painter.layout_no_wrap(
            text.drop_overlay_text.to_string(),
            egui::FontId::proportional(22.0),
            element_color,
        );
        let text_rect = galley.rect;
        painter.galley(
            text_pos - egui::vec2(text_rect.width() * 0.5, 0.0),
            galley,
            element_color,
        );

        // Request repaint for close animation
        if self.drop_overlay_fade > 0.0 {
            ctx.request_repaint();
        }
    }
}

// Helper function to format tips with bold text using LayoutJob
fn format_tip_with_bold(tip_number: usize, text: &str, is_dark_mode: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let number_text = format!("{}. ", tip_number);

    // Color scheme based on theme
    let regular_color = if is_dark_mode {
        egui::Color32::from_rgb(180, 180, 180) // Gray for dark mode
    } else {
        egui::Color32::from_rgb(100, 100, 100) // Darker gray for light mode
    };

    let bold_color = if is_dark_mode {
        egui::Color32::from_rgb(150, 200, 255) // Soft cyan for dark mode
    } else {
        egui::Color32::from_rgb(40, 100, 180) // Dark blue for light mode
    };

    // Create text format for regular text
    let mut text_format = TextFormat::default();
    text_format.font_id = egui::FontId::proportional(13.0);
    text_format.color = regular_color;

    // Append number in regular color
    job.append(&number_text, 0.0, text_format.clone());

    // Parse text for **bold** markers
    let mut current_text = String::new();
    let mut chars = text.chars().peekable();
    let mut is_bold = false;

    while let Some(ch) = chars.next() {
        if ch == '*' && chars.peek() == Some(&'*') {
            // Found ** marker
            chars.next(); // consume second *

            if !current_text.is_empty() {
                // Append accumulated text
                let mut fmt = text_format.clone();
                if is_bold {
                    fmt.color = bold_color;
                }
                job.append(&current_text, 0.0, fmt);
                current_text.clear();
            }

            is_bold = !is_bold;
        } else {
            current_text.push(ch);
        }
    }

    // Append remaining text
    if !current_text.is_empty() {
        let mut fmt = text_format.clone();
        if is_bold {
            fmt.color = bold_color;
        }
        job.append(&current_text, 0.0, fmt);
    }

    job
}
