use super::types::SettingsApp;
use crate::gui::locale::LocaleText;
use crate::gui::settings_ui::node_graph::{blocks_to_snarl, snarl_to_graph};
use crate::gui::settings_ui::{
    render_footer, render_global_settings, render_history_panel, render_preset_editor,
    render_sidebar, ViewMode,
};
use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use image;
use image::GenericImageView;

// compile_error!("Find me!");
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
                    .fill(footer_bg)
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
                            0
                        } else {
                            12
                        },
                        se: if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
                            0
                        } else {
                            12
                        },
                    }),
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

    pub(crate) fn render_title_bar(&mut self, ctx: &egui::Context) {
        let text = LocaleText::get(&self.config.ui_language);
        let is_dark = ctx.style().visuals.dark_mode;
        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        // Match Footer Color
        let bar_bg = if is_dark {
            egui::Color32::from_gray(20)
        } else {
            egui::Color32::from_gray(240)
        };

        egui::TopBottomPanel::top("title_bar")
            .exact_height(40.0)
            .frame(
                egui::Frame::default()
                    .inner_margin(if is_maximized {
                        egui::Margin {
                            left: 8,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        }
                    } else {
                        egui::Margin {
                            left: 8,
                            right: 8,
                            top: 6,
                            bottom: 6,
                        }
                    })
                    .fill(bar_bg)
                    .corner_radius(egui::CornerRadius {
                        nw: if is_maximized { 0 } else { 12 },
                        ne: if is_maximized { 0 } else { 12 },
                        sw: 0,
                        se: 0,
                    })
                    .stroke(egui::Stroke::NONE),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                // --- DRAG HANDLE (Whole Bar) ---
                // We use interact instead of allocate_response to avoid pushing content
                let drag_resp =
                    ui.interact(ui.max_rect(), ui.id().with("drag_bar"), egui::Sense::drag());
                if drag_resp.dragged() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // --- LEFT SIDE: Sidebar Controls ---
                    // Theme Switcher
                    let (theme_icon, tooltip) = match self.config.theme_mode {
                        crate::config::ThemeMode::Dark => {
                            (crate::gui::icons::Icon::Moon, "Theme: Dark")
                        }
                        crate::config::ThemeMode::Light => {
                            (crate::gui::icons::Icon::Sun, "Theme: Light")
                        }
                        crate::config::ThemeMode::System => {
                            (crate::gui::icons::Icon::Device, "Theme: System (Auto)")
                        }
                    };

                    if crate::gui::icons::icon_button_sized(ui, theme_icon, 18.0)
                        .on_hover_text(tooltip)
                        .clicked()
                    {
                        self.config.theme_mode = match self.config.theme_mode {
                            crate::config::ThemeMode::System => crate::config::ThemeMode::Dark,
                            crate::config::ThemeMode::Dark => crate::config::ThemeMode::Light,
                            crate::config::ThemeMode::Light => crate::config::ThemeMode::System,
                        };
                        self.save_and_sync();
                    }

                    // Language Switcher
                    let original_lang = self.config.ui_language.clone();
                    let lang_flag = match self.config.ui_language.as_str() {
                        "vi" => "🇻🇳",
                        "ko" => "🇰🇷",
                        _ => "🇺🇸",
                    };
                    egui::ComboBox::from_id_salt("title_lang_switch")
                        .width(30.0)
                        .selected_text(lang_flag)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.ui_language,
                                "en".to_string(),
                                "🇺🇸 English",
                            );
                            ui.selectable_value(
                                &mut self.config.ui_language,
                                "vi".to_string(),
                                "🇻🇳 Tiếng Việt",
                            );
                            ui.selectable_value(
                                &mut self.config.ui_language,
                                "ko".to_string(),
                                "🇰🇷 한국어",
                            );
                        });
                    if original_lang != self.config.ui_language {
                        self.save_and_sync();
                    }

                    // History Button
                    ui.spacing_mut().item_spacing.x = 2.0;
                    crate::gui::icons::draw_icon_static(
                        ui,
                        crate::gui::icons::Icon::History,
                        Some(14.0),
                    );
                    let is_history = matches!(self.view_mode, ViewMode::History);
                    if ui
                        .selectable_label(
                            is_history,
                            egui::RichText::new(text.history_btn).size(13.0),
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::History;
                    }

                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.add_space(2.0);

                    // Chill Corner (PromptDJ)
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("🎵 {}", text.prompt_dj_btn))
                                    .color(egui::Color32::WHITE)
                                    .size(12.0),
                            )
                            .fill(egui::Color32::from_rgb(100, 100, 200))
                            .corner_radius(6.0),
                        )
                        .clicked()
                    {
                        crate::overlay::prompt_dj::show_prompt_dj();
                    }

                    // Help Assistant
                    let help_bg = if is_dark {
                        egui::Color32::from_rgb(80, 60, 120)
                    } else {
                        egui::Color32::from_rgb(180, 160, 220)
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("❓ {}", text.help_assistant_btn))
                                    .color(egui::Color32::WHITE)
                                    .size(12.0),
                            )
                            .fill(help_bg)
                            .corner_radius(6.0),
                        )
                        .on_hover_text(text.help_assistant_title)
                        .clicked()
                    {
                        std::thread::spawn(|| {
                            crate::gui::settings_ui::help_assistant::show_help_input();
                        });
                    }

                    // Global Settings
                    ui.spacing_mut().item_spacing.x = 2.0;
                    crate::gui::icons::draw_icon_static(
                        ui,
                        crate::gui::icons::Icon::Settings,
                        Some(14.0),
                    );
                    let is_global = matches!(self.view_mode, ViewMode::Global);
                    if ui
                        .selectable_label(
                            is_global,
                            egui::RichText::new(text.global_settings).size(13.0),
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::Global;
                    }

                    // --- RIGHT SIDE: Window Controls & Branding ---
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        let grid_h = if is_maximized { 40.0 } else { 28.0 };
                        let btn_size = egui::vec2(40.0, grid_h);

                        // Close Button
                        let close_resp = ui.allocate_response(btn_size, egui::Sense::click());
                        if close_resp.clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if close_resp.hovered() {
                            ui.painter().rect_filled(
                                close_resp.rect,
                                0.0,
                                egui::Color32::from_rgb(232, 17, 35),
                            );
                        }
                        crate::gui::icons::paint_icon(
                            ui.painter(),
                            close_resp
                                .rect
                                .shrink2(egui::vec2(12.0, if is_maximized { 12.0 } else { 6.0 })),
                            crate::gui::icons::Icon::Close,
                            if close_resp.hovered() {
                                egui::Color32::WHITE
                            } else {
                                if is_dark {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::BLACK
                                }
                            },
                        );

                        // Maximize / Restore
                        let max_resp = ui.allocate_response(btn_size, egui::Sense::click());
                        if max_resp.clicked() {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                        }
                        if max_resp.hovered() {
                            ui.painter().rect_filled(
                                max_resp.rect,
                                0.0,
                                if is_dark {
                                    egui::Color32::from_gray(60)
                                } else {
                                    egui::Color32::from_gray(220)
                                },
                            );
                        }
                        let max_icon = if is_maximized {
                            crate::gui::icons::Icon::Restore
                        } else {
                            crate::gui::icons::Icon::Maximize
                        };
                        crate::gui::icons::paint_icon(
                            ui.painter(),
                            max_resp
                                .rect
                                .shrink2(egui::vec2(13.0, if is_maximized { 13.0 } else { 7.0 })),
                            max_icon,
                            if is_dark {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::BLACK
                            },
                        );

                        // Minimize
                        let min_resp = ui.allocate_response(btn_size, egui::Sense::click());
                        if min_resp.clicked() {
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        if min_resp.hovered() {
                            ui.painter().rect_filled(
                                min_resp.rect,
                                0.0,
                                if is_dark {
                                    egui::Color32::from_gray(60)
                                } else {
                                    egui::Color32::from_gray(220)
                                },
                            );
                        }
                        crate::gui::icons::paint_icon(
                            ui.painter(),
                            min_resp
                                .rect
                                .shrink2(egui::vec2(13.0, if is_maximized { 13.0 } else { 7.0 })),
                            crate::gui::icons::Icon::Minimize,
                            if is_dark {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::BLACK
                            },
                        );

                        ui.add_space(8.0);

                        // Title Text
                        let title_text =
                            egui::RichText::new("Screen Goated Toolbox (by nganlinh4)")
                                .strong()
                                .size(13.0)
                                .color(if is_dark {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::BLACK
                                });

                        if ui
                            .add(egui::Label::new(title_text).sense(egui::Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(
                                "https://github.com/nganlinh4/screen-goated-toolbox",
                            ));
                        }

                        ui.add_space(6.0);

                        // App Icon
                        let icon_handle = if is_dark {
                            if self.icon_dark.is_none() {
                                let bytes = include_bytes!("../../../assets/app-icon-small.png");
                                if let Ok(image) = image::load_from_memory(bytes) {
                                    let resized = image.resize(
                                        128,
                                        20,
                                        image::imageops::FilterType::Lanczos3,
                                    );
                                    let image_buffer = resized.to_rgba8();
                                    let size =
                                        [image_buffer.width() as _, image_buffer.height() as _];
                                    let pixels = image_buffer.as_raw();
                                    let color_image =
                                        egui::ColorImage::from_rgba_unmultiplied(size, pixels);
                                    let handle = ctx.load_texture(
                                        "app-icon-dark",
                                        color_image,
                                        Default::default(),
                                    );
                                    self.icon_dark = Some(handle);
                                }
                            }
                            self.icon_dark.as_ref()
                        } else {
                            if self.icon_light.is_none() {
                                let bytes =
                                    include_bytes!("../../../assets/app-icon-small-light.png");
                                if let Ok(image) = image::load_from_memory(bytes) {
                                    let resized = image.resize(
                                        128,
                                        20,
                                        image::imageops::FilterType::Lanczos3,
                                    );
                                    let image_buffer = resized.to_rgba8();
                                    let size =
                                        [image_buffer.width() as _, image_buffer.height() as _];
                                    let pixels = image_buffer.as_raw();
                                    let color_image =
                                        egui::ColorImage::from_rgba_unmultiplied(size, pixels);
                                    let handle = ctx.load_texture(
                                        "app-icon-light",
                                        color_image,
                                        Default::default(),
                                    );
                                    self.icon_light = Some(handle);
                                }
                            }
                            self.icon_light.as_ref()
                        };

                        if let Some(texture) = icon_handle {
                            ui.add(egui::Image::new(texture).max_height(20.0));
                        }
                    });
                });
            });
    }

    pub(crate) fn render_main_layout(&mut self, ctx: &egui::Context) {
        let text = LocaleText::get(&self.config.ui_language);
        let is_dark = ctx.style().visuals.dark_mode;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(ctx.style().visuals.panel_fill)
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 0, // Footer handles bottom corners now
                        se: 0, // Footer handles bottom corners now
                    }),
            )
            .show(ctx, |ui| {
                let available_width = ui.available_width();
                let left_width = available_width * 0.35;
                let right_width = available_width * 0.65;

                ui.horizontal(|ui| {
                    // Left Sidebar
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            // Add Left Margin/Padding for Sidebar
                            egui::Frame::NONE
                                .inner_margin(egui::Margin {
                                    left: 8,
                                    right: 0,
                                    top: 8,
                                    bottom: 0,
                                })
                                .show(ui, |ui| {
                                    if render_sidebar(
                                        ui,
                                        &mut self.config,
                                        &mut self.view_mode,
                                        &text,
                                    ) {
                                        self.save_and_sync();
                                    }
                                });
                        },
                    );

                    ui.add_space(10.0);

                    // Right Detail View
                    ui.allocate_ui_with_layout(
                        egui::vec2((right_width - 20.0).max(0.0), ui.available_height()),
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
                                        &mut self.show_cerebras_api_key,
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

    pub(crate) fn render_window_resize_handles(&self, ctx: &egui::Context) {
        let border = 8.0; // Increased sensitivity
        let corner = 16.0; // Larger corner area

        // Fix recursive lock: Get inner_rect first, release lock, then fallback
        let inner_rect = ctx.input(|i| i.viewport().inner_rect);
        let viewport_rect = inner_rect.unwrap_or_else(|| ctx.viewport_rect());
        let size = viewport_rect.size();

        // Use a single Area for all resize handles to reduce overhead
        // Disable resize when maximized
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }

        egui::Area::new(egui::Id::new("resize_handles_overlay"))
            .order(egui::Order::Debug)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let directions = [
                    // Corners (NorthWest, NorthEast, SouthWest, SouthEast)
                    (
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(corner, corner)),
                        egui::viewport::ResizeDirection::NorthWest,
                        "nw",
                    ),
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(size.x - corner, 0.0),
                            egui::Pos2::new(size.x, corner),
                        ),
                        egui::viewport::ResizeDirection::NorthEast,
                        "ne",
                    ),
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(0.0, size.y - corner),
                            egui::Pos2::new(corner, size.y),
                        ),
                        egui::viewport::ResizeDirection::SouthWest,
                        "sw",
                    ),
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(size.x - corner, size.y - corner),
                            egui::Pos2::new(size.x, size.y),
                        ),
                        egui::viewport::ResizeDirection::SouthEast,
                        "se",
                    ),
                    // Edges (North, South, West, East)
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(corner, 0.0),
                            egui::Pos2::new(size.x - corner, border),
                        ),
                        egui::viewport::ResizeDirection::North,
                        "n",
                    ),
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(corner, size.y - border),
                            egui::Pos2::new(size.x - corner, size.y),
                        ),
                        egui::viewport::ResizeDirection::South,
                        "s",
                    ),
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(0.0, corner),
                            egui::Pos2::new(border, size.y - corner),
                        ),
                        egui::viewport::ResizeDirection::West,
                        "w",
                    ),
                    (
                        egui::Rect::from_min_max(
                            egui::Pos2::new(size.x - border, corner),
                            egui::Pos2::new(size.x, size.y - corner),
                        ),
                        egui::viewport::ResizeDirection::East,
                        "e",
                    ),
                ];

                for (rect, dir, id_suffix) in directions {
                    // Use ui.allocate_rect to reserve space (though in an Area it might not push layout much if fixed,
                    // but interaction is key). ui.interact is better for explicit rects.
                    let response = ui.interact(rect, ui.id().with(id_suffix), egui::Sense::drag());

                    if response.hovered() || response.dragged() {
                        ui.ctx().set_cursor_icon(match dir {
                            egui::viewport::ResizeDirection::North
                            | egui::viewport::ResizeDirection::South => {
                                egui::CursorIcon::ResizeVertical
                            }
                            egui::viewport::ResizeDirection::East
                            | egui::viewport::ResizeDirection::West => {
                                egui::CursorIcon::ResizeHorizontal
                            }
                            egui::viewport::ResizeDirection::NorthWest
                            | egui::viewport::ResizeDirection::SouthEast => {
                                egui::CursorIcon::ResizeNwSe
                            }
                            egui::viewport::ResizeDirection::NorthEast
                            | egui::viewport::ResizeDirection::SouthWest => {
                                egui::CursorIcon::ResizeNeSw
                            }
                        });
                    }

                    if response.drag_started() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                    }
                }
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
