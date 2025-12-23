pub mod utils;
mod selection;
pub mod result;
pub mod recording; 
pub mod process;
pub mod broom_assets;
pub mod paint_utils;
pub mod text_selection;
pub mod text_input; // NEW MODULE
pub mod preset_wheel; // MASTER preset wheel
// realtime_overlay module removed (was old GDI-based, now using realtime_webview)
pub mod realtime_webview; // New WebView2-based with smooth scrolling

pub use selection::{show_selection_overlay, is_selection_overlay_active_and_dismiss};
pub use recording::{show_recording_overlay, is_recording_overlay_active, stop_recording_and_submit};
pub use text_selection::show_text_selection_tag;
// Use the new WebView2-based realtime overlay
pub use realtime_webview::{show_realtime_overlay, is_realtime_overlay_active, stop_realtime_overlay};
