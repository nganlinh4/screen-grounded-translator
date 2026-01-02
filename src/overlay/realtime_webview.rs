pub mod app_selection;
pub mod manager;
pub mod state;
pub mod webview;
pub mod wndproc;

pub use manager::{
    is_realtime_overlay_active, is_warmed_up, show_realtime_overlay, stop_realtime_overlay, warmup,
};
pub use state::*;
pub use webview::sync_visibility_to_webviews;
