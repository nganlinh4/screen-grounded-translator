pub mod auto_copy_badge; // Auto-copy notification badge
pub mod broom_assets;
pub mod paint_utils;
pub mod preset_wheel;
pub mod process;
pub mod recording;
pub mod result;
mod selection;
pub mod text_input; // NEW MODULE
pub mod text_selection;
pub mod utils; // MASTER preset wheel
               // realtime_overlay module removed (was old GDI-based, now using realtime_webview)
pub mod favorite_bubble; // Floating bubble for favorite presets
pub mod html_components; // Split HTML components (CSS/JS)
pub mod realtime_html; // HTML generation for realtime overlay
pub mod realtime_webview; // New WebView2-based with smooth scrolling
pub mod tray_popup; // Custom non-blocking tray popup menu

pub use recording::{
    is_recording_overlay_active, show_recording_overlay, stop_recording_and_submit,
};
pub use selection::{is_selection_overlay_active_and_dismiss, show_selection_overlay};
pub use text_selection::show_text_selection_tag;
// Use the new WebView2-based realtime overlay
pub use realtime_webview::{
    is_realtime_overlay_active, show_realtime_overlay, stop_realtime_overlay,
};

/// Get the shared WebView2 data directory path.
/// All WebViews using this same path will share browser processes, reducing RAM usage.
/// Uses %APPDATA%/SGT/webview_data on Windows.
pub fn get_shared_webview_data_dir() -> std::path::PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    path.push("SGT");
    path.push("webview_data");
    // Ensure the directory exists
    let _ = std::fs::create_dir_all(&path);
    path
}
