use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, AtomicU8},
    Once,
};
use wry::WebView;

// Constants
pub const BUBBLE_SIZE: i32 = 40;
pub const PANEL_WIDTH: i32 = 220;
pub const DRAG_THRESHOLD: i32 = 5; // Pixels of movement before counting as a drag

// Smooth opacity animation state
pub const OPACITY_TIMER_ID: usize = 1;
pub const OPACITY_STEP: u8 = 25; // Opacity change per frame (~150ms total animation)
pub const OPACITY_INACTIVE: u8 = 80; // ~31% opacity when not hovered
pub const OPACITY_ACTIVE: u8 = 255; // 100% opacity when hovered/expanded

pub const PHYSICS_TIMER_ID: usize = 2;

// Statics / Atomics
pub static REGISTER_BUBBLE_CLASS: Once = Once::new();
pub static REGISTER_PANEL_CLASS: Once = Once::new();
pub static BUBBLE_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static BUBBLE_HWND: AtomicIsize = AtomicIsize::new(0);
pub static PANEL_HWND: AtomicIsize = AtomicIsize::new(0);
pub static IS_EXPANDED: AtomicBool = AtomicBool::new(false);
pub static IS_HOVERED: AtomicBool = AtomicBool::new(false);
pub static IS_DRAGGING: AtomicBool = AtomicBool::new(false);
pub static IS_DRAGGING_MOVED: AtomicBool = AtomicBool::new(false);
pub static DRAG_START_X: AtomicIsize = AtomicIsize::new(0);
pub static DRAG_START_Y: AtomicIsize = AtomicIsize::new(0);

// Animation state
pub static CURRENT_OPACITY: AtomicU8 = AtomicU8::new(80); // Start at inactive opacity
pub static BLINK_STATE: AtomicU8 = AtomicU8::new(0); // 0=None, 1..4=Blink Phases

// Thread Locals
thread_local! {
    pub static PANEL_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    pub static PHYSICS_STATE: RefCell<(f32, f32)> = RefCell::new((0.0, 0.0));
}

// App icon embedded at compile time
const ICON_PNG_BYTES: &[u8] = include_bytes!("../../../assets/app-icon-small.png");

// Cached decoded RGBA pixels
lazy_static::lazy_static! {
    pub static ref ICON_RGBA: Vec<u8> = {
        if let Ok(img) = image::load_from_memory(ICON_PNG_BYTES) {
            let resized = img.resize_exact(
                BUBBLE_SIZE as u32,
                BUBBLE_SIZE as u32,
                image::imageops::FilterType::Lanczos3
            );
            resized.to_rgba8().into_raw()
        } else {
            vec![]
        }
    };
}
