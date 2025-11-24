use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::overlay::broom_assets::BroomState;

// --- PHYSICS & ANIMATION STATES ---

pub struct DustParticle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32, // 1.0 to 0.0
    pub _color: u32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AnimationMode {
    Idle,
    MovingLeft,
    MovingRight,
    Sweeping,
}

pub struct CursorPhysics {
    pub x: f32,
    pub y: f32,
    
    // Animation State
    pub mode: AnimationMode,
    pub frame_timer: i32,
    pub sweep_stage: usize, 
    pub idle_frame: usize, 
    pub sway_decay: i32, // Smoothing for directional changes

    pub particles: Vec<DustParticle>,
    pub bitmaps: HashMap<usize, HBITMAP>, 
    pub initialized: bool,
}

impl Default for CursorPhysics {
    fn default() -> Self {
        Self {
            x: 0.0, y: 0.0,
            mode: AnimationMode::Idle,
            frame_timer: 0,
            sweep_stage: 0,
            idle_frame: 0,
            sway_decay: 0,
            particles: Vec::new(),
            bitmaps: HashMap::new(),
            initialized: false,
        }
    }
}

// State for each window
pub struct WindowState {
    pub alpha: u8,
    pub is_hovered: bool,
    pub on_copy_btn: bool,
    pub copy_success: bool,
    pub bg_color: u32,
    pub linked_window: Option<HWND>,
    pub physics: CursorPhysics,
}

lazy_static::lazy_static! {
    pub static ref WINDOW_STATES: Mutex<HashMap<isize, WindowState>> = Mutex::new(HashMap::new());
}

pub enum WindowType {
    Primary,
    Secondary,
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

// Map enum variants to integers for HashMap
pub fn state_to_idx(s: BroomState) -> usize {
    match s {
        BroomState::Idle1 => 0,
        BroomState::Idle2 => 1,
        BroomState::Left => 2,
        BroomState::Right => 3,
        BroomState::Sweep1Windup => 4,
        BroomState::Sweep2Smash => 5,
        BroomState::Sweep3DragR => 6,
        BroomState::Sweep4DragL => 7,
        BroomState::Sweep5Lift => 8,
    }
}
