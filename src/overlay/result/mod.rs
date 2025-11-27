use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::mem::size_of;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::overlay::utils::to_wstring;

mod state;
mod paint;
mod logic;

use state::{WINDOW_STATES, WindowState, CursorPhysics, AnimationMode};
pub use state::{WindowType, link_windows};

static mut CURRENT_BG_COLOR: u32 = 0x00222222;

// OPTIMIZATION: Thread-safe one-time window class registration
static REGISTER_RESULT_CLASS: Once = Once::new();

pub fn create_result_window(target_rect: RECT, win_type: WindowType) -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        // OPTIMIZATION: Register class only once, thread-safely
        REGISTER_RESULT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = HCURSOR(0); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(0);
            let _ = RegisterClassW(&wc);
        });

        let width = (target_rect.right - target_rect.left).abs();
        let height = (target_rect.bottom - target_rect.top).abs();
        
        let (x, y, color) = match win_type {
            WindowType::Primary => {
                CURRENT_BG_COLOR = 0x00222222; 
                (target_rect.left, target_rect.top, 0x00222222)
            },
            WindowType::SecondaryExplicit => {
                // Exact positioning + Secondary Color
                CURRENT_BG_COLOR = 0x002d4a22; 
                (target_rect.left, target_rect.top, 0x002d4a22)
            },
            WindowType::Secondary => {
                let padding = 10;
                
                // --- INTELLIGENT MONITOR-AWARE POSITIONING ---
                // 1. Get the monitor that contains the selection
                let hmonitor = MonitorFromRect(&target_rect, MONITOR_DEFAULTTONEAREST);
                
                // 2. Get that monitor's WORK AREA (excludes taskbars)
                let mut mi = MONITORINFO::default();
                mi.cbSize = size_of::<MONITORINFO>() as u32;
                GetMonitorInfoW(hmonitor, &mut mi);
                let work_rect = mi.rcWork;

                // Potential coordinates
                let pos_right_x = target_rect.right + padding;
                let pos_left_x  = target_rect.left - width - padding;
                let pos_bottom_y = target_rect.bottom + padding;
                let pos_top_y    = target_rect.top - height - padding;

                // Calculate available space on each side relative to the WORK AREA
                let space_right  = work_rect.right - pos_right_x;
                let space_left   = (target_rect.left - padding) - work_rect.left;
                let space_bottom = work_rect.bottom - pos_bottom_y;
                let space_top    = (target_rect.top - padding) - work_rect.top;

                // 3. Logic: Find best side
                // Priority: Right -> Bottom -> Left -> Top
                let (mut best_x, mut best_y) = if space_right >= width {
                    (pos_right_x, target_rect.top)
                } else if space_bottom >= height {
                    (target_rect.left, pos_bottom_y)
                } else if space_left >= width {
                    (pos_left_x, target_rect.top)
                } else if space_top >= height {
                    (target_rect.left, pos_top_y)
                } else {
                    // 4. Fallback: Pick the side with the MOST available space (minimizes overlap)
                    let max_space = space_right.max(space_left).max(space_bottom).max(space_top);
                    
                    if max_space == space_right {
                        (pos_right_x, target_rect.top)
                    } else if max_space == space_left {
                        (pos_left_x, target_rect.top)
                    } else if max_space == space_bottom {
                        (target_rect.left, pos_bottom_y)
                    } else {
                        (target_rect.left, pos_top_y)
                    }
                };
                
                // 5. FINAL SAFEGUARD: Hard Clamp to Monitor Work Area
                // This ensures the window is fully visible even if it has to overlap the selection.
                let safe_w = width.min(work_rect.right - work_rect.left);
                let safe_h = height.min(work_rect.bottom - work_rect.top);
                
                best_x = best_x.clamp(work_rect.left, work_rect.right - safe_w);
                best_y = best_y.clamp(work_rect.top, work_rect.bottom - safe_h);

                CURRENT_BG_COLOR = 0x002d4a22; 
                (best_x, best_y, 0x002d4a22)
            }
        };

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name,
            w!(""),
            WS_POPUP,
            x, y, width, height,
            None, None, instance, None
        );

        let mut physics = CursorPhysics::default();
        physics.initialized = true;

        {
            let mut states = WINDOW_STATES.lock().unwrap();
            states.insert(hwnd.0 as isize, WindowState {
                alpha: 220,
                is_hovered: false,
                on_copy_btn: false,
                copy_success: false,
                bg_color: color,
                linked_window: None,
                physics,
                font_cache_dirty: true,
                cached_font_size: 72,
                content_bitmap: HBITMAP(0),
                last_w: 0,
                last_h: 0,
                pending_text: None,
                last_text_update_time: 0,
                // FIX 1: Initialize background cache fields
                bg_bitmap: HBITMAP(0),
                bg_bits: std::ptr::null_mut(),
                bg_w: 0,
                bg_h: 0,
            });
        }

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA);
        
        let corner_preference = 2u32; 
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &corner_preference as *const _ as *const _,
            size_of::<u32>() as u32
        );
        
        SetTimer(hwnd, 3, 16, None);
        
        InvalidateRect(hwnd, None, false);
        UpdateWindow(hwnd);
        
        hwnd
    }
}

pub fn update_window_text(hwnd: HWND, text: &str) {
    if !unsafe { IsWindow(hwnd).as_bool() } { return; }
    
    let mut states = WINDOW_STATES.lock().unwrap();
    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
        state.pending_text = Some(text.to_string());
    }
}

// Helper to calculate button position dynamically
fn get_copy_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let btn_size = 28;
    let margin = 12;
    
    // Adaptive Positioning:
    // If window is too short (less than button + 2*margin), center vertically.
    // Otherwise, stick to bottom-right.
    let threshold_h = btn_size + (margin * 2);
    
    let top = if window_h < threshold_h {
        (window_h - btn_size) / 2
    } else {
        window_h - margin - btn_size
    };

    RECT {
        left: window_w - margin - btn_size,
        top,
        right: window_w - margin,
        bottom: top + btn_size,
    }
}

unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        
        WM_SETCURSOR => {
            let mut show_system_cursor = false;
            {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    if state.on_copy_btn { show_system_cursor = true; }
                }
            }
            if show_system_cursor {
                let h_cursor = LoadCursorW(None, IDC_HAND).unwrap_or(HCURSOR(0));
                SetCursor(h_cursor);
            } else {
                SetCursor(HCURSOR(0));
            }
            LRESULT(1)
        }

        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                let dx = x - state.physics.x;
                let impulse = (dx * 1.5).clamp(-20.0, 20.0);
                state.physics.tilt_velocity -= impulse * 0.2; 
                state.physics.current_tilt = state.physics.current_tilt.clamp(-22.5, 22.5);

                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                
                // === UPDATED HITBOX LOGIC WITH ADAPTIVE CENTERING ===
                let btn_rect = get_copy_btn_rect(width, height);
                let padding = 4;
                
                state.on_copy_btn = 
                    x as i32 >= btn_rect.left - padding && 
                    x as i32 <= btn_rect.right + padding && 
                    y as i32 >= btn_rect.top - padding && 
                    y as i32 <= btn_rect.bottom + padding;
                
                state.physics.x = x;
                state.physics.y = y;

                if !state.is_hovered {
                    state.is_hovered = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                }
                
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }

        0x02A3 => { // WM_MOUSELEAVE
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                state.is_hovered = false;
                state.on_copy_btn = false;
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }

        WM_LBUTTONUP | WM_RBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i32;
            
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            
            // === UPDATED HITBOX LOGIC FOR CLICK WITH ADAPTIVE CENTERING ===
            let btn_rect = get_copy_btn_rect(width, height);
            let padding = 4;
            
            let is_copy_click = 
                x >= btn_rect.left - padding && 
                x <= btn_rect.right + padding && 
                y >= btn_rect.top - padding && 
                y <= btn_rect.bottom + padding;

            if is_copy_click || msg == WM_RBUTTONUP {
                 let text_len = GetWindowTextLengthW(hwnd) + 1;
                let mut buf = vec![0u16; text_len as usize];
                GetWindowTextW(hwnd, &mut buf);
                let text = String::from_utf16_lossy(&buf[..text_len as usize - 1]).to_string();
                crate::overlay::utils::copy_to_clipboard(&text, hwnd);
                
                {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.copy_success = true;
                    }
                }
                SetTimer(hwnd, 1, 1500, None);
                if is_copy_click && msg == WM_LBUTTONUP { return LRESULT(0); }
            }

            if !is_copy_click {
                 {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.physics.mode = AnimationMode::Smashing;
                        state.physics.state_timer = 0.0;
                    }
                }
            }
            
            let (linked_hwnd, main_alpha) = {
                let states = WINDOW_STATES.lock().unwrap();
                let linked = if let Some(state) = states.get(&(hwnd.0 as isize)) { state.linked_window } else { None };
                let alpha = if let Some(state) = states.get(&(hwnd.0 as isize)) { state.alpha } else { 220 };
                (linked, alpha)
            };
            if let Some(linked) = linked_hwnd {
                if IsWindow(linked).as_bool() {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(linked.0 as isize)) {
                        state.physics.mode = AnimationMode::DragOut;
                        state.physics.state_timer = 0.0;
                        state.alpha = main_alpha;
                    }
                }
            }
            LRESULT(0)
        }

        WM_TIMER => {
            let mut need_repaint = false;
            let mut pending_update: Option<String> = None;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u32)
                .unwrap_or(0);
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                     // THROTTLING LOGIC:
                     // Update if we have text AND (it's been >66ms OR it's the very first update)
                     // 66ms ~= 15 FPS update rate for text. Physics stays at 60 FPS.
                     if state.pending_text.is_some() && 
                        (state.last_text_update_time == 0 || now.wrapping_sub(state.last_text_update_time) > 66) {
                         
                         pending_update = state.pending_text.take();
                         state.last_text_update_time = now;
                     }
                }
            }

            if let Some(txt) = pending_update {
                let wide_text = to_wstring(&txt);
                SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.font_cache_dirty = true;
                }
                need_repaint = true;
            }

            logic::handle_timer(hwnd, wparam);
            if need_repaint {
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.remove(&(hwnd.0 as isize)) {
                if state.content_bitmap.0 != 0 {
                    DeleteObject(state.content_bitmap);
                }
                // FIX 1: Clean up background cache
                if state.bg_bitmap.0 != 0 {
                    DeleteObject(state.bg_bitmap);
                }
            }
            LRESULT(0)
        }

        WM_PAINT => {
            paint::paint_window(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize { 
                 PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
