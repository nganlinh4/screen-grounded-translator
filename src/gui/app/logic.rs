use super::types::{
    SettingsApp, UserEvent, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, RESTORE_SIGNAL,
};
use crate::config::{Hotkey, ThemeMode};
use crate::gui::app::utils::simple_rand;
use crate::gui::key_mapping::{egui_key_to_vk, egui_pointer_to_vk};
use crate::gui::locale::LocaleText;
use crate::icon_gen;
use crate::{WINDOW_HEIGHT, WINDOW_WIDTH};
use eframe::egui;
use std::sync::atomic::Ordering;
use tray_icon::{MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

impl SettingsApp {
    pub(crate) fn check_updater(&mut self) {
        while let Ok(status) = self.update_rx.try_recv() {
            // Show popup notification when update is available
            if let crate::updater::UpdateStatus::UpdateAvailable { ref version, .. } = status {
                // Show blue-themed update notification with longer duration
                let ui_lang = self.config.ui_language.clone();
                let locale = crate::gui::locale::LocaleText::get(&ui_lang);
                let notification_text =
                    format!("{} v{}", locale.update_available_notification, version);
                crate::overlay::auto_copy_badge::show_update_notification(&notification_text);
            }
            self.update_status = status;
        }
    }

    pub(crate) fn update_theme_and_tray(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);

        // 1. Check if we need to poll system theme (only if in System mode)
        let mut current_system_dark = self.last_system_theme_dark;

        if now - self.theme_check_timer > 1.0 {
            self.theme_check_timer = now;
            // Always update system state tracker, even if not currently used
            current_system_dark = crate::gui::utils::is_system_in_dark_mode();
            self.last_system_theme_dark = current_system_dark;
        }

        // 2. Calculate Effective Theme
        let effective_dark = match self.config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => current_system_dark,
        };

        // 3. Apply Changes if Effective Theme Changed
        if effective_dark != self.last_effective_theme_dark {
            self.last_effective_theme_dark = effective_dark;

            // A. Update Visuals (egui)
            if effective_dark {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }

            // B. Update Native Icons (Tray & Window) based on Effective Theme
            if let Some(tray) = &mut self.tray_icon {
                let new_icon = icon_gen::get_tray_icon(effective_dark);
                let _ = tray.set_icon(Some(new_icon));
            }
            crate::gui::utils::update_window_icon_native(effective_dark);

            // C. Update Realtime Webviews
            unsafe {
                use crate::api::realtime_audio::WM_THEME_UPDATE;
                use crate::overlay::realtime_webview::state::{REALTIME_HWND, TRANSLATION_HWND};
                use windows::Win32::Foundation::{LPARAM, WPARAM};
                use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

                let realtime_hwnd = std::ptr::addr_of!(REALTIME_HWND).read();
                if !realtime_hwnd.is_invalid() {
                    let _ =
                        PostMessageW(Some(realtime_hwnd), WM_THEME_UPDATE, WPARAM(0), LPARAM(0));
                }
                let translation_hwnd = std::ptr::addr_of!(TRANSLATION_HWND).read();
                if !translation_hwnd.is_invalid() {
                    let _ = PostMessageW(
                        Some(translation_hwnd),
                        WM_THEME_UPDATE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }

                use crate::overlay::realtime_webview::state::APP_SELECTION_HWND;
                let app_sel_val = APP_SELECTION_HWND.load(std::sync::atomic::Ordering::SeqCst);
                if app_sel_val != 0 {
                    let hwnd =
                        windows::Win32::Foundation::HWND(app_sel_val as *mut std::ffi::c_void);
                    let _ = PostMessageW(Some(hwnd), WM_THEME_UPDATE, WPARAM(0), LPARAM(0));
                }
            }
        }

        // --- TRAY MENU I18N UPDATE ---
        // Update tray menu items when language changes
        if self.config.ui_language != self.last_ui_language {
            self.last_ui_language = self.config.ui_language.clone();
            let new_locale = LocaleText::get(&self.config.ui_language);
            self.tray_settings_item.set_text(new_locale.tray_settings);
            self.tray_quit_item.set_text(new_locale.tray_quit);
        }

        // --- LAZY TRAY ICON RECONCILE ---
        if self.tray_icon.is_none() {
            if now - self.tray_retry_timer > 1.0 {
                self.tray_retry_timer = now;
                let icon = icon_gen::get_tray_icon(self.last_effective_theme_dark);
                if let Ok(tray) = TrayIconBuilder::new()
                    .with_tooltip("Screen Goated Toolbox (nganlinh4)")
                    .with_icon(icon)
                    .build()
                {
                    self.tray_icon = Some(tray);
                }
            }
        }
    }

    pub(crate) fn update_startup(&mut self, ctx: &egui::Context) {
        if self.startup_stage == 0 {
            unsafe {
                let mut cursor_pos = POINT::default();
                let _ = GetCursorPos(&mut cursor_pos);
                let h_monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO::default();
                mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                let _ = GetMonitorInfoW(h_monitor, &mut mi);

                let work_w = (mi.rcWork.right - mi.rcWork.left) as f32;
                let work_h = (mi.rcWork.bottom - mi.rcWork.top) as f32;
                let work_left = mi.rcWork.left as f32;
                let work_top = mi.rcWork.top as f32;

                let pixels_per_point = ctx.pixels_per_point();
                let win_w_physical = WINDOW_WIDTH * pixels_per_point;
                let win_h_physical = WINDOW_HEIGHT * pixels_per_point;

                let center_x_physical = work_left + (work_w - win_w_physical) / 2.0;
                let center_y_physical = work_top + (work_h - win_h_physical) / 2.0;

                let x_logical = center_x_physical / pixels_per_point;
                let y_logical = center_y_physical / pixels_per_point;

                if !self.config.start_in_tray {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                        x_logical, y_logical,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        WINDOW_WIDTH,
                        WINDOW_HEIGHT,
                    )));
                }

                self.startup_stage = 1;
                ctx.request_repaint();
                return;
            }
        } else if self.startup_stage == 1 {
            // --- EARLY INIT: TRULY BEFORE SPLASH ---

            // 1. Start favorite bubble (WebView creation)
            let has_favorites = self.config.presets.iter().any(|p| p.is_favorite);
            if self.config.show_favorite_bubble && has_favorites {
                crate::overlay::favorite_bubble::show_favorite_bubble();
            }

            // 2. Trigger auto-update check (Network/Disk IO)
            if let Some(updater) = &self.updater {
                updater.check_for_updates();
            }

            self.startup_stage = 2;
            ctx.request_repaint();
            return;
        } else if self.startup_stage < 35 {
            // Wait for ~35 frames to let background windows (Bubble/Tray) settle
            self.startup_stage += 1;
            ctx.request_repaint();
            return;
        } else if self.startup_stage == 35 {
            // CRITICAL: Wait for Tray Icon to be ready before starting splash
            // This ensures all shell integration is settled.
            if self.tray_icon.is_none() {
                // If tray failed or is still initializing, keep waiting
                ctx.request_repaint();
                return;
            }

            if self.config.start_in_tray {
                // ENSURE HIDDEN: If starting in tray, we must stay invisible.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                // SHOW SPLASH: Create it NOW for perfect t=0 timing.
                if self.splash.is_none() {
                    self.splash = Some(crate::gui::splash::SplashScreen::new(ctx));
                }

                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    WINDOW_WIDTH,
                    WINDOW_HEIGHT,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }

            self.startup_stage = 36;
        }
    }

    /// Called exactly once when the splash screen finishes its exit animation.
    fn on_splash_finished(&mut self) {
        // High-prio tasks now done before splash.
    }

    pub(crate) fn update_bubble_sync(&mut self) {
        // --- FAVORITE BUBBLE SYNC (Change-Detection Only) ---
        // Only trigger show/hide when state actually changes to avoid per-frame overhead
        let current_has_favorites = self.config.presets.iter().any(|p| p.is_favorite);
        let current_bubble_enabled = self.config.show_favorite_bubble;

        // Update tray item enabled state (cheap operation)
        self.tray_favorite_bubble_item
            .set_enabled(current_has_favorites);

        // Detect state change
        let state_changed = current_bubble_enabled != self.last_bubble_enabled
            || current_has_favorites != self.last_has_favorites;

        if state_changed {
            self.last_bubble_enabled = current_bubble_enabled;
            self.last_has_favorites = current_has_favorites;

            if current_bubble_enabled && current_has_favorites {
                crate::overlay::favorite_bubble::show_favorite_bubble();
            } else {
                crate::overlay::favorite_bubble::hide_favorite_bubble();
            }
        }
    }

    pub(crate) fn update_splash(&mut self, ctx: &egui::Context) {
        if let Some(splash) = &mut self.splash {
            match splash.update(ctx) {
                crate::gui::splash::SplashStatus::Ongoing => {
                    // Do NOT return here. Continue to render main UI underneath.
                }
                crate::gui::splash::SplashStatus::Finished => {
                    self.splash = None;
                    self.on_splash_finished();
                }
            }
        }
    }

    pub(crate) fn check_restore_signal(&mut self, ctx: &egui::Context) {
        if RESTORE_SIGNAL.swap(false, Ordering::SeqCst) {
            self.restore_window(ctx);
        }
    }

    pub(crate) fn update_tips_logic(&mut self, ctx: &egui::Context) {
        let text = LocaleText::get(&self.config.ui_language);
        let now = ctx.input(|i| i.time);

        // Initialize timer on first run
        if self.tip_timer == 0.0 {
            self.tip_timer = now;
        }

        // Calculate duration based on text length (reading speed ~ 15 chars/sec + 2s base)
        let current_tip = text
            .tips_list
            .get(self.current_tip_idx)
            .unwrap_or(&"")
            .to_string();
        let display_duration = (2.0 + (current_tip.len() as f64 * 0.06)) as f32;
        let fade_duration = 0.5f32;

        let elapsed = (now - self.tip_timer) as f32;

        if self.tip_is_fading_in {
            // Fading In
            self.tip_fade_state = (elapsed / fade_duration as f32).min(1.0);
            if elapsed >= fade_duration {
                self.tip_fade_state = 1.0;
                // Fully visible, wait for duration
                if elapsed >= fade_duration + display_duration {
                    self.tip_is_fading_in = false; // Start fading out
                    self.tip_timer = now; // Reset timer for fade-out
                }
            }
            ctx.request_repaint();
        } else {
            // Fading Out
            self.tip_fade_state = (1.0 - (elapsed / fade_duration as f32)).max(0.0);
            if elapsed >= fade_duration {
                self.tip_fade_state = 0.0;

                // Switch to next random tip
                self.rng_seed = simple_rand(self.rng_seed);
                if !text.tips_list.is_empty() {
                    let next = (self.rng_seed as usize) % text.tips_list.len();
                    // Avoid repeating same tip if possible
                    if next == self.current_tip_idx && text.tips_list.len() > 1 {
                        self.current_tip_idx = (next + 1) % text.tips_list.len();
                    } else {
                        self.current_tip_idx = next;
                    }
                }

                self.tip_timer = now; // Reset timer
                self.tip_is_fading_in = true; // Start fading in
            }
            ctx.request_repaint();
        }
    }

    pub(crate) fn update_hotkey_recording(&mut self, ctx: &egui::Context) {
        if let Some(preset_idx) = self.recording_hotkey_for_preset {
            let mut key_recorded: Option<(u32, u32, String)> = None;
            let mut cancel = false;

            ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    cancel = true;
                } else {
                    let mut modifiers_bitmap = 0;
                    if i.modifiers.ctrl {
                        modifiers_bitmap |= MOD_CONTROL;
                    }
                    if i.modifiers.alt {
                        modifiers_bitmap |= MOD_ALT;
                    }
                    if i.modifiers.shift {
                        modifiers_bitmap |= MOD_SHIFT;
                    }
                    if i.modifiers.command {
                        modifiers_bitmap |= MOD_WIN;
                    }

                    // Check Keyboard Events
                    for event in &i.events {
                        if let egui::Event::Key {
                            key, pressed: true, ..
                        } = event
                        {
                            if let Some(vk) = egui_key_to_vk(key) {
                                if !matches!(vk, 16 | 17 | 18 | 91 | 92) {
                                    let key_name =
                                        format!("{:?}", key).trim_start_matches("Key").to_string();
                                    key_recorded = Some((vk, modifiers_bitmap, key_name));
                                }
                            }
                        }
                    }

                    // Check Mouse Events (Middle, Extra1, Extra2)
                    if key_recorded.is_none() {
                        let mouse_buttons = [
                            egui::PointerButton::Middle,
                            egui::PointerButton::Extra1,
                            egui::PointerButton::Extra2,
                        ];

                        for btn in mouse_buttons {
                            if i.pointer.button_pressed(btn) {
                                if let Some(vk) = egui_pointer_to_vk(&btn) {
                                    let name = match btn {
                                        egui::PointerButton::Middle => "Middle Click",
                                        egui::PointerButton::Extra1 => "Mouse Back",
                                        egui::PointerButton::Extra2 => "Mouse Forward",
                                        _ => "Mouse",
                                    }
                                    .to_string();
                                    key_recorded = Some((vk, modifiers_bitmap, name));
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            if cancel {
                self.recording_hotkey_for_preset = None;
                self.hotkey_conflict_msg = None;
            } else if let Some((vk, mods, key_name)) = key_recorded {
                if let Some(msg) = self.check_hotkey_conflict(vk, mods, preset_idx) {
                    self.hotkey_conflict_msg = Some(msg);
                } else {
                    let mut name_parts = Vec::new();
                    if (mods & MOD_CONTROL) != 0 {
                        name_parts.push("Ctrl".to_string());
                    }
                    if (mods & MOD_ALT) != 0 {
                        name_parts.push("Alt".to_string());
                    }
                    if (mods & MOD_SHIFT) != 0 {
                        name_parts.push("Shift".to_string());
                    }
                    if (mods & MOD_WIN) != 0 {
                        name_parts.push("Win".to_string());
                    }
                    name_parts.push(key_name);

                    let new_hotkey = Hotkey {
                        code: vk,
                        modifiers: mods,
                        name: name_parts.join(" + "),
                    };

                    if let Some(preset) = self.config.presets.get_mut(preset_idx) {
                        if !preset
                            .hotkeys
                            .iter()
                            .any(|h| h.code == vk && h.modifiers == mods)
                        {
                            preset.hotkeys.push(new_hotkey);
                            self.save_and_sync();
                        }
                    }
                    self.recording_hotkey_for_preset = None;
                    self.hotkey_conflict_msg = None;
                }
            }
        }
    }

    pub(crate) fn update_sr_hotkey_recording(&mut self, ctx: &egui::Context) {
        if self.recording_sr_hotkey {
            let mut key_recorded: Option<(u32, u32, String)> = None;
            let mut cancel = false;

            ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    cancel = true;
                } else {
                    let mut modifiers_bitmap = 0;
                    if i.modifiers.ctrl {
                        modifiers_bitmap |= MOD_CONTROL;
                    }
                    if i.modifiers.alt {
                        modifiers_bitmap |= MOD_ALT;
                    }
                    if i.modifiers.shift {
                        modifiers_bitmap |= MOD_SHIFT;
                    }
                    if i.modifiers.command {
                        modifiers_bitmap |= MOD_WIN;
                    }

                    // Check Keyboard Events
                    for event in &i.events {
                        if let egui::Event::Key {
                            key, pressed: true, ..
                        } = event
                        {
                            if let Some(vk) = egui_key_to_vk(key) {
                                if !matches!(vk, 16 | 17 | 18 | 91 | 92) {
                                    let key_name =
                                        format!("{:?}", key).trim_start_matches("Key").to_string();
                                    key_recorded = Some((vk, modifiers_bitmap, key_name));
                                }
                            }
                        }
                    }

                    // Check Mouse Events
                    if key_recorded.is_none() {
                        let mouse_buttons = [
                            egui::PointerButton::Middle,
                            egui::PointerButton::Extra1,
                            egui::PointerButton::Extra2,
                        ];

                        for btn in mouse_buttons {
                            if i.pointer.button_pressed(btn) {
                                if let Some(vk) = egui_pointer_to_vk(&btn) {
                                    let name = match btn {
                                        egui::PointerButton::Middle => "Middle Click",
                                        egui::PointerButton::Extra1 => "Mouse Back",
                                        egui::PointerButton::Extra2 => "Mouse Forward",
                                        _ => "Mouse",
                                    }
                                    .to_string();
                                    key_recorded = Some((vk, modifiers_bitmap, name));
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            if cancel {
                self.recording_sr_hotkey = false;
            } else if let Some((vk, mods, key_name)) = key_recorded {
                let mut name_parts = Vec::new();
                if (mods & MOD_CONTROL) != 0 {
                    name_parts.push("Ctrl".to_string());
                }
                if (mods & MOD_ALT) != 0 {
                    name_parts.push("Alt".to_string());
                }
                if (mods & MOD_SHIFT) != 0 {
                    name_parts.push("Shift".to_string());
                }
                if (mods & MOD_WIN) != 0 {
                    name_parts.push("Win".to_string());
                }
                name_parts.push(key_name);

                let new_hotkey = Hotkey {
                    code: vk,
                    modifiers: mods,
                    name: name_parts.join(" + "),
                };

                if let Some(msg) = self.config.check_hotkey_conflict(vk, mods, None) {
                    crate::log_info!("Hotkey conflict: {}", msg);
                } else {
                    self.config.screen_record_hotkeys.push(new_hotkey);
                    self.save_and_sync();
                }
                self.recording_sr_hotkey = false;
            }
        }
    }

    pub(crate) fn handle_events(&mut self, ctx: &egui::Context) {
        // --- Event Handling ---
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                UserEvent::Tray(tray_event) => match tray_event {
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => {
                        self.restore_window(ctx);
                    }

                    _ => {}
                },
                UserEvent::Menu(menu_event) => {
                    match menu_event.id.0.as_str() {
                        "1002" => {
                            self.restore_window(ctx);
                        }
                        "1003" => {
                            // Toggle favorite bubble
                            self.config.show_favorite_bubble = !self.config.show_favorite_bubble;
                            self.tray_favorite_bubble_item
                                .set_checked(self.config.show_favorite_bubble);
                            self.save_and_sync();

                            // Spawn or dismiss the bubble overlay
                            if self.config.show_favorite_bubble {
                                crate::overlay::favorite_bubble::show_favorite_bubble();
                            } else {
                                crate::overlay::favorite_bubble::hide_favorite_bubble();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(crate) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.is_quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }
}
