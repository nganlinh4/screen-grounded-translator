mod init;
mod logic;
mod rendering;
mod types;
mod utils;

pub use types::SettingsApp;
pub use utils::signal_restore_window;

use eframe::egui;

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Updater
        self.check_updater();

        // Theme & Tray
        self.update_theme_and_tray(ctx);

        // Startup Logic
        self.update_startup(ctx);

        // Bubble Sync
        self.update_bubble_sync();

        // Splash
        self.update_splash(ctx);

        // Restore Signal
        self.check_restore_signal(ctx);

        // Hotkey Recording
        self.update_hotkey_recording(ctx);

        // Event Handling
        self.handle_events(ctx);

        // Close Request
        self.handle_close_request(ctx);

        // Tips Logic
        self.update_tips_logic(ctx);

        // --- UI LAYOUT ---
        // Footer & Tips Modal
        self.render_footer_and_tips_modal(ctx);

        // Main Layout
        self.render_main_layout(ctx);

        // Fade In Overlay (Last)
        self.render_fade_overlay(ctx);

        // Render Splash Overlay (Last Last)
        if let Some(splash) = &self.splash {
            splash.paint(ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray_icon = None;
    }
}
