use windows::Win32::Foundation::*;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::Win32::Graphics::Gdi::{HBITMAP, HFONT};

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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ResizeEdge {
    None,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, PartialEq)]
pub enum InteractionMode {
    None,
    Resizing(ResizeEdge),
    DraggingWindow,
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
    pub needs_cleanup_repaint: bool, // Flag to trigger one final repaint when entering DragOut
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
            needs_cleanup_repaint: false,
        }
    }
}

// Context for Refinement
#[derive(Clone)]
pub enum RefineContext {
    None,
    Image(Vec<u8>), // PNG Bytes
}

// NEW: Config for Delayed Retranslation (triggered after generation)
#[derive(Clone)]
pub struct RetranslationConfig {
    pub enabled: bool,
    pub target_lang: String,
    pub model_id: String,
    pub provider: String,
    pub streaming: bool,
    pub auto_copy: bool,
}

pub struct WindowState {
    pub alpha: u8,
    pub is_hovered: bool,
    pub on_copy_btn: bool,
    pub copy_success: bool,
    pub on_edit_btn: bool, 
    pub on_undo_btn: bool, 
    
    // Edit Mode
    pub is_editing: bool,         // Is the edit box open?
    pub edit_hwnd: HWND,          // Handle to child EDIT control
    pub context_data: RefineContext, // Data needed for API call
    pub full_text: String,        // Current full text content
    
    // Text History for Undo
    pub text_history: Vec<String>, // Stack of previous text states
    
    // Refinement State
    pub is_refining: bool,
    pub animation_offset: f32,

    // Metadata for Refinement/Processing
    pub model_id: String,
    pub provider: String,
    pub streaming_enabled: bool,
    
    // NEW: Preset Prompt for "Type" mode logic
    pub preset_prompt: String,
    // NEW: Retranslation Config
    pub retrans_config: Option<RetranslationConfig>,
    
    pub bg_color: u32,
    pub linked_window: Option<HWND>,
    pub physics: CursorPhysics,
    
    // --- INTERACTION STATE ---
    pub interaction_mode: InteractionMode,
    pub current_resize_edge: ResizeEdge, // Track edge hover state for painting
    pub drag_start_mouse: POINT,
    pub drag_start_window_rect: RECT,
    pub has_moved_significantly: bool, // To distinguish click vs drag
    
    // --- CACHING & THROTTLING ---
    pub font_cache_dirty: bool,
    pub cached_font_size: i32,
    pub content_bitmap: HBITMAP, 
    pub last_w: i32,
    pub last_h: i32,
    
    // Handle pending updates to avoid flooding Paint
    pub pending_text: Option<String>,
    
    // Timestamp for throttling text updates (in milliseconds)
    pub last_text_update_time: u32,
    
    // BACKGROUND CACHING
    pub bg_bitmap: HBITMAP,
    pub bg_w: i32,
    pub bg_h: i32,
    
    // EDIT FONT HANDLE (must be deleted to avoid GDI leak)
    pub edit_font: HFONT,
    
    // Graphics mode for refining animation (standard vs minimal)
    pub graphics_mode: String,
}

// SAFETY: Raw pointers are not Send/Sync, but we only use them within the main thread
// This is safe because all access is synchronized via WINDOW_STATES mutex
unsafe impl Send for WindowState {}
unsafe impl Sync for WindowState {}

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
