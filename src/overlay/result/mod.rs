mod state;
pub mod paint;
mod logic;
pub mod layout;
mod window;
mod event_handler;
pub mod markdown_view;
pub mod refine_input;

pub use state::{WindowType, link_windows, RefineContext, RetranslationConfig, WINDOW_STATES, close_windows_with_token};
pub use window::{create_result_window, update_window_text, get_chain_color};
