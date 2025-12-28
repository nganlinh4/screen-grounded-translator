use super::types::{SettingsApp, RESTORE_SIGNAL};
use crate::config::save_config;
use eframe::egui;
use std::sync::atomic::Ordering;
use windows::core::*;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

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
        None
    }
}
