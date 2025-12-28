pub mod html;
pub mod panel;
pub mod render;
pub mod state;
pub mod utils;
pub mod window;

pub use panel::update_favorites_panel;
pub use window::{hide_favorite_bubble, show_favorite_bubble, trigger_blink_animation};
