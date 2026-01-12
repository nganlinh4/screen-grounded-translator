mod init;
mod input_handler;
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
        // Handle Dropped Files and Paste FIRST (before any UI consumes events)
        input_handler::handle_dropped_files(ctx);
        input_handler::handle_paste(ctx);

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
        // Title Bar (Custom Windows Bar)
        self.render_title_bar(ctx);

        // Footer & Tips Modal
        self.render_footer_and_tips_modal(ctx);

        // Main Layout
        self.render_main_layout(ctx);

        // Window Resizing (Must be last to override cursors at edges)
        self.render_window_resize_handles(ctx);

        // Overlays
        self.render_fade_overlay(ctx);
        self.render_drop_overlay(ctx);

        // Render Minimal Mode Overlay (Realtime)
        crate::overlay::realtime_egui::render_minimal_overlay(ctx);

        // Render Splash Overlay (Last Last)
        if let Some(splash) = &self.splash {
            splash.paint(ctx);
        }

        // Render Drop Overlay when dragging files (Very Last)
        self.render_drop_overlay(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray_icon = None;
    }
}
