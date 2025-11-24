use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::mem::size_of;

use crate::overlay::utils::to_wstring;
use crate::overlay::broom_assets::{BroomState, get_broom_pixels};

mod state;
mod paint;
mod logic;

use state::{WINDOW_STATES, WindowState, CursorPhysics, AnimationMode, state_to_idx};
pub use state::{WindowType, link_windows};

static mut CURRENT_BG_COLOR: u32 = 0x00222222;

pub fn create_result_window(target_rect: RECT, win_type: WindowType) -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        let mut wc = WNDCLASSW::default();
        if !GetClassInfoW(instance, class_name, &mut wc).as_bool() {
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = HCURSOR(0); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(0);
            RegisterClassW(&wc);
        }

        let width = (target_rect.right - target_rect.left).abs();
        let height = (target_rect.bottom - target_rect.top).abs();
        
        let (x, y, color) = match win_type {
            WindowType::Primary => {
                CURRENT_BG_COLOR = 0x00222222; 
                (target_rect.left, target_rect.top, 0x00222222)
            },
            WindowType::Secondary => {
                let padding = 10;
                let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let screen_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let screen_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                let screen_right = screen_x + screen_w;
                let screen_bottom = screen_y + screen_h;

                let right_x = target_rect.right + padding;
                let bottom_y = target_rect.bottom + padding;
                let left_x = target_rect.left - width - padding;
                let top_y = target_rect.top - height - padding;

                let (new_x, new_y) = if right_x + width <= screen_right {
                    (right_x, target_rect.top)
                } else if bottom_y + height <= screen_bottom {
                    (target_rect.left, bottom_y)
                } else if left_x >= screen_x {
                    (left_x, target_rect.top)
                } else if top_y >= screen_y {
                    (target_rect.left, top_y)
                } else {
                    (right_x, target_rect.top)
                };
                CURRENT_BG_COLOR = 0x002d4a22; 
                (new_x, new_y, 0x002d4a22)
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

        // Pre-generate assets
        let mut physics = CursorPhysics::default();
        let states_list = [
            BroomState::Idle1, BroomState::Idle2, BroomState::Left, BroomState::Right,
            BroomState::Sweep1Windup, BroomState::Sweep2Smash, BroomState::Sweep3DragR,
            BroomState::Sweep4DragL, BroomState::Sweep5Lift
        ];
        
        for s in states_list {
            let px = get_broom_pixels(s);
            physics.bitmaps.insert(state_to_idx(s), paint::create_bitmap_from_pixels(&px));
        }
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
            });
        }

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA);
        
        // Enable rounded corners (Windows 11)
        let corner_preference = 2u32; // DWMWCP_ROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33), // DWMWA_WINDOW_CORNER_PREFERENCE
            &corner_preference as *const _ as *const _,
            size_of::<u32>() as u32
        );
        
        // 60 FPS Animation Timer (approx 16ms)
        SetTimer(hwnd, 3, 16, None);
        
        InvalidateRect(hwnd, None, false);
        UpdateWindow(hwnd);
        
        hwnd
    }
}

pub fn update_window_text(hwnd: HWND, text: &str) {
    unsafe {
        if !IsWindow(hwnd).as_bool() { return; }
        let wide_text = to_wstring(text);
        SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
        InvalidateRect(hwnd, None, false);
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
                    // Show hand cursor if over copy button
                    if state.on_copy_btn {
                        show_system_cursor = true;
                    }
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
                
                // Copy Button Hit Test
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                let btn_size = 24;
                let btn_rect = RECT { left: width - btn_size, top: height - btn_size, right: width, bottom: height };
                state.on_copy_btn = x as i32 >= btn_rect.left && x as i32 <= btn_rect.right && y as i32 >= btn_rect.top && y as i32 <= btn_rect.bottom;

                // Physics: Only update sway if not sweeping
                if state.physics.mode != AnimationMode::Sweeping {
                    // Increased threshold to 2.0 to prevent jitter
                    if dx > 2.0 {
                        state.physics.mode = AnimationMode::MovingRight;
                        state.physics.sway_decay = 8; // Reset decay counter
                    } else if dx < -2.0 {
                        state.physics.mode = AnimationMode::MovingLeft;
                        state.physics.sway_decay = 8;
                    } else {
                        // Slowly return to idle
                        if state.physics.sway_decay > 0 {
                            state.physics.sway_decay -= 1;
                        } else {
                            state.physics.mode = AnimationMode::Idle;
                        }
                    }
                }
                
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
            
            // Check Copy Button
            let btn_size = 24;
            let btn_rect = RECT { 
                left: width - btn_size, 
                top: height - btn_size, 
                right: width, 
                bottom: height 
            };
            let is_copy_click = x >= btn_rect.left && x <= btn_rect.right && y >= btn_rect.top && y <= btn_rect.bottom;

            if is_copy_click || msg == WM_RBUTTONUP {
                // Copy Logic
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

            // --- TRIGGER SWEEP ANIMATION ---
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.physics.mode = AnimationMode::Sweeping;
                    state.physics.sweep_stage = 0;
                    state.physics.frame_timer = 0;
                }
            }
            LRESULT(0)
        }

        WM_TIMER => {
            logic::handle_timer(hwnd, wparam);
            LRESULT(0)
        }

        WM_DESTROY => {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.remove(&(hwnd.0 as isize)) {
                for (_, hbm) in state.physics.bitmaps {
                    DeleteObject(hbm);
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
