use windows::Win32::Foundation::*;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::Win32::Graphics::Gdi::HBITMAP;

// --- DYNAMIC PARTICLES ---
pub struct DustParticle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32, // 1.0 to 0.0
    pub size: f32,
    pub color: u32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AnimationMode {
    Idle,       // Normal mouse movement
    Smashing,   // User clicked (Sweep start)
    DragOut,    // User holding/dragging out
}

pub struct CursorPhysics {
    pub x: f32,
    pub y: f32,
    
    // Spring Physics
    pub current_tilt: f32,   // Current angle in degrees
    pub tilt_velocity: f32,  // Angular velocity
    
    // Deformation
    pub squish_factor: f32,  // 1.0 = normal, 0.5 = flat
    pub bristle_bend: f32,   // Lag of bristles
    
    // Logic
    pub mode: AnimationMode,
    pub state_timer: f32,
    pub particles: Vec<DustParticle>,
    
    // Clean up
    pub initialized: bool,
}

impl Default for CursorPhysics {
    fn default() -> Self {
        Self {
            x: 0.0, y: 0.0,
            current_tilt: 0.0,
            tilt_velocity: 0.0,
            squish_factor: 1.0,
            bristle_bend: 0.0,
            mode: AnimationMode::Idle,
            state_timer: 0.0,
            particles: Vec::new(),
            initialized: false,
        }
    }
}

pub struct WindowState {
    pub alpha: u8,
    pub is_hovered: bool,
    pub on_copy_btn: bool,
    pub copy_success: bool,
    pub bg_color: u32,
    pub linked_window: Option<HWND>,
    pub physics: CursorPhysics,
    
    // --- CACHING & THROTTLING ---
    pub font_cache_dirty: bool,
    pub cached_font_size: i32,
    pub content_bitmap: HBITMAP, 
    pub last_w: i32,
    pub last_h: i32,
    
    // New: Handle pending updates to avoid flooding Paint
    pub pending_text: Option<String>,
    
    // NEW: Timestamp for throttling text updates (in milliseconds)
    pub last_text_update_time: u32,
}

lazy_static::lazy_static! {
    pub static ref WINDOW_STATES: Mutex<HashMap<isize, WindowState>> = Mutex::new(HashMap::new());
}

pub enum WindowType {
    Primary,
    Secondary,
    SecondaryExplicit, // New type: Trust the coordinates, use Secondary color
}

pub fn link_windows(hwnd1: HWND, hwnd2: HWND) {
    let mut states = WINDOW_STATES.lock().unwrap();
    if let Some(s1) = states.get_mut(&(hwnd1.0 as isize)) {
        s1.linked_window = Some(hwnd2);
    }
    if let Some(s2) = states.get_mut(&(hwnd2.0 as isize)) {
        s2.linked_window = Some(hwnd1);
    }
}
