use super::types::{SettingsApp, RESTORE_SIGNAL};
use crate::config::save_config;
use eframe::egui;
use std::sync::atomic::Ordering;
use windows::core::*;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::*;

// Simple Linear Congruential Generator for randomness without external crate
pub fn simple_rand(seed: u32) -> u32 {
    seed.wrapping_mul(1103515245).wrapping_add(12345)
}

/// Public function to signal the main window to restore (called from tray popup)
pub fn signal_restore_window() {
    RESTORE_SIGNAL.store(true, Ordering::SeqCst);
    unsafe {
        if let Ok(event) = OpenEventW(
            EVENT_ALL_ACCESS,
            false,
            w!("Global\\ScreenGoatedToolboxRestoreEvent"),
        ) {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}

impl SettingsApp {
    pub(crate) fn save_and_sync(&mut self) {
        if let crate::gui::settings_ui::ViewMode::Preset(idx) = self.view_mode {
            self.config.active_preset_idx = idx;
        }

        let mut state = self.app_state_ref.lock().unwrap();
        state.hotkeys_updated = true;
        state.config = self.config.clone();
        drop(state);
        save_config(&self.config);

        // Sync PromptDJ settings if window is active
        crate::overlay::prompt_dj::update_settings();

        unsafe {
            let class = w!("HotkeyListenerClass");
            let title = w!("Listener");
            let hwnd = windows::Win32::UI::WindowsAndMessaging::FindWindowW(class, title)
                .unwrap_or_default();
            if !hwnd.is_invalid() {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    0x0400 + 101,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }

    pub(crate) fn restore_window(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::Normal,
        ));
        ctx.request_repaint();
    }

    pub(crate) fn check_hotkey_conflict(
        &self,
        vk: u32,
        mods: u32,
        current_preset_idx: usize,
    ) -> Option<String> {
        for (idx, preset) in self.config.presets.iter().enumerate() {
            if idx == current_preset_idx {
                continue;
            }
            for hk in &preset.hotkeys {
                if hk.code == vk && hk.modifiers == mods {
                    return Some(format!(
                        "Conflict with '{}' in preset '{}'",
                        hk.name, preset.name
                    ));
                }
            }
        }
        // Check global screen record hotkey
        let sr_hk = &self.config.screen_record_hotkey;
        if sr_hk.code == vk && sr_hk.modifiers == mods {
            return Some(format!(
                "Conflict with global hotkey '{}' (Screen Record)",
                sr_hk.name
            ));
        }
        None
    }
}

/// Robustly restart the application on Windows.
/// Uses a temporary batch file with a small delay to ensure the current process exits
/// and releases its single-instance mutex before the new instance starts.
pub fn restart_app() {
    if let Ok(exe_path) = std::env::current_exe() {
        // Create a temporary batch file to handle the delayed restart reliably
        let kill_mutex_cmd = "timeout /t 1 /nobreak > NUL".to_string();
        // Pass --restarted flag to show notification on next start
        let start_cmd = format!("start \"\" \"{}\" --restarted", exe_path.to_string_lossy());
        let self_del_cmd = "(goto) 2>nul & del \"%~f0\"";

        let batch_content = format!(
            "@echo off\r\n{}\r\n{}\r\n{}",
            kill_mutex_cmd, start_cmd, self_del_cmd
        );

        let temp_dir = std::env::temp_dir();
        let bat_path = temp_dir.join(format!("sgt_restart_{}.bat", std::process::id()));

        if let Ok(_) = std::fs::write(&bat_path, batch_content) {
            // Spawn the batch file hidden via cmd /C with CREATE_NO_WINDOW
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/C", &bat_path.to_string_lossy()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn();
            std::process::exit(0);
        } else {
            // Fallback: Just try to spawn directly if batch fails
            let _ = std::process::Command::new(exe_path)
                .arg("--restarted")
                .spawn();
            std::process::exit(0);
        }
    }
}
