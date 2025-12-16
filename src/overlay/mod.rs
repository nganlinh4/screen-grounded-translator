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

pub use selection::{show_selection_overlay, is_selection_overlay_active_and_dismiss};
pub use recording::{show_recording_overlay, is_recording_overlay_active, stop_recording_and_submit};
pub use text_selection::show_text_selection_tag;
pub use preset_wheel::{show_preset_wheel, is_wheel_active, dismiss_wheel};
