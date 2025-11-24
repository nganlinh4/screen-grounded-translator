use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::mem::size_of;

use crate::overlay::utils::to_wstring;

mod state;
mod paint;
mod logic;

use state::{WINDOW_STATES, WindowState, CursorPhysics, AnimationMode};
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
                
                // Logic to position secondary window smartly
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
                cached_font_size: 72, // Initialize with MAX size, so it shrinks to fit
                content_bitmap: HBITMAP(0),
                last_w: 0,
                last_h: 0,
                pending_text: None,
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
        
        SetTimer(hwnd, 3, 16, None); // 60 FPS Timer
        
        InvalidateRect(hwnd, None, false);
        UpdateWindow(hwnd);
        
        hwnd
    }
}

// Updated: Does NOT invalidate rect immediately. 
// Just queues the data. Timer loop handles the rest.
pub fn update_window_text(hwnd: HWND, text: &str) {
    if !unsafe { IsWindow(hwnd).as_bool() } { return; }
    
    let mut states = WINDOW_STATES.lock().unwrap();
    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
        state.pending_text = Some(text.to_string());
        // We DO NOT call InvalidateRect here anymore.
        // This allows the text stream to run at 1000hz without killing the UI.
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
                let btn_size = 24;
                let btn_rect = RECT { left: width - btn_size, top: height - btn_size, right: width, bottom: height };
                state.on_copy_btn = x as i32 >= btn_rect.left && x as i32 <= btn_rect.right && y as i32 >= btn_rect.top && y as i32 <= btn_rect.bottom;
                
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
            
            let btn_size = 24;
            let btn_rect = RECT { left: width - btn_size, top: height - btn_size, right: width, bottom: height };
            let is_copy_click = x >= btn_rect.left && x <= btn_rect.right && y >= btn_rect.top && y <= btn_rect.bottom;

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
            // CHECK FOR PENDING TEXT UPDATES HERE
            // This caps the text updates to the timer speed (60fps)
            let mut need_repaint = false;
            let mut pending_update: Option<String> = None;
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                     if let Some(txt) = state.pending_text.take() {
                         pending_update = Some(txt);
                     }
                }
            }

            if let Some(txt) = pending_update {
                let wide_text = to_wstring(&txt);
                SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                
                // Now we mark dirty
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
