pub mod auto_copy_badge; // Auto-copy notification badge
pub mod broom_assets;
pub mod continuous_mode; // Continuous mode for image/text presets (hold-to-activate)
pub mod input_history; // Persistent input history for arrow up/down navigation
pub mod paint_utils;
pub mod preset_wheel;
pub mod process;
pub mod prompt_dj;
pub mod recording;
pub mod result;
mod selection;
pub mod text_input; // NEW MODULE
pub mod text_selection;

use std::sync::atomic::{AtomicBool, Ordering};

static IS_BUSY_WITH_OVERLAY: AtomicBool = AtomicBool::new(false);

pub fn is_busy() -> bool {
    IS_BUSY_WITH_OVERLAY.load(Ordering::SeqCst)
}

pub fn set_is_busy(busy: bool) {
    IS_BUSY_WITH_OVERLAY.store(busy, Ordering::SeqCst);
}

pub mod utils; // MASTER preset wheel
               // realtime_overlay module removed (was old GDI-based, now using realtime_webview)
pub mod favorite_bubble; // Floating bubble for favorite presets
pub mod html_components; // Split HTML components (CSS/JS)
pub mod realtime_egui; // Minimal mode (native egui)
pub mod realtime_html; // HTML generation for realtime overlay
pub mod realtime_webview; // New WebView2-based with smooth scrolling
pub mod tray_popup; // Custom non-blocking tray popup menu

pub use recording::{
    is_recording_overlay_active, show_recording_overlay, stop_recording_and_submit,
};
pub use selection::{is_selection_overlay_active, show_selection_overlay};
pub use text_selection::show_text_selection_tag;
// Use the new WebView2-based realtime overlay
lazy_static::lazy_static! {
    /// Mutex to ensure only one WebView is being initialized at a time globally.
    /// This prevents deadlocks and resource exhaustion during startup warmup loops.
    pub static ref GLOBAL_WEBVIEW_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

// pub use crate::api::realtime_audio::show_realtime_overlay; // REMOVED - Incorrect path
pub use realtime_webview::{
    is_realtime_overlay_active, show_realtime_overlay, stop_realtime_overlay,
};

/// Get a WebView2 data directory path.
/// If subdir is provided, returns a component-specific folder to avoid file-lock contention.
pub fn get_shared_webview_data_dir(subdir: Option<&str>) -> std::path::PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("SGT");
    path.push("webview_data");
    if let Some(s) = subdir {
        path.push(s);
    }
    // Ensure the directory exists
    let _ = std::fs::create_dir_all(&path);
    path
}

/// Clear WebView permissions (MIDI, etc.) by removing the webview_data directory.
/// The directory will be recreated on next WebView initialization.
/// Returns true if successfully cleared, false otherwise.
///
/// On Windows, this function handles the "directory not empty" error (code 145)
/// that can occur when files are locked by WebView processes. It will retry
/// with delays and attempt per-file deletion as a fallback.
pub fn clear_webview_permissions() -> bool {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("SGT");
    path.push("webview_data");

    if !path.exists() {
        // Already clean
        return true;
    }

    // Try up to 3 times with increasing delays
    for attempt in 0..3 {
        if attempt > 0 {
            // Wait before retry (100ms, 500ms)
            std::thread::sleep(std::time::Duration::from_millis(if attempt == 1 {
                100
            } else {
                500
            }));
        }

        match std::fs::remove_dir_all(&path) {
            Ok(_) => {
                println!("WebView data cleared successfully at {:?}", path);
                return true;
            }
            Err(e) => {
                // Check if it's the "directory not empty" error (Windows error 145)
                if e.raw_os_error() == Some(145) {
                    eprintln!(
                        "Attempt {}: Directory not empty, trying per-file deletion...",
                        attempt + 1
                    );
                    // Try to delete files individually first
                    if delete_directory_contents_recursive(&path) {
                        // Now try to remove the empty directory
                        if std::fs::remove_dir(&path).is_ok() {
                            println!("WebView data cleared successfully (per-file) at {:?}", path);
                            return true;
                        }
                    }
                } else if attempt == 2 {
                    eprintln!(
                        "Failed to clear WebView data after {} attempts: {:?}",
                        attempt + 1,
                        e
                    );
                }
            }
        }
    }

    false
}

/// Recursively delete directory contents, ignoring errors for individual locked files.
/// Returns true if at least some cleanup was done.
fn delete_directory_contents_recursive(path: &std::path::Path) -> bool {
    let mut any_deleted = false;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                // Recursively clean subdirectory
                delete_directory_contents_recursive(&entry_path);
                // Try to remove the now-empty directory
                if std::fs::remove_dir(&entry_path).is_ok() {
                    any_deleted = true;
                }
            } else {
                // Try to remove the file
                if std::fs::remove_file(&entry_path).is_ok() {
                    any_deleted = true;
                }
            }
        }
    }

    any_deleted
}
/// Check if we should use dark mode based on config.
/// Uses direct registry check for System theme to avoid crate overhead/crashes.
pub fn is_dark_mode() -> bool {
    let mode = {
        if let Ok(app) = crate::APP.lock() {
            app.config.theme_mode.clone()
        } else {
            crate::config::ThemeMode::Dark
        }
    };

    match mode {
        crate::config::ThemeMode::Dark => true,
        crate::config::ThemeMode::Light => false,
        crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
    }
}
