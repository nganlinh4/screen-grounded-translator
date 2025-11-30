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

use state::{WINDOW_STATES, WindowState, CursorPhysics, AnimationMode, InteractionMode, ResizeEdge};

pub use state::{WindowType, link_windows, RefineContext};

static mut CURRENT_BG_COLOR: u32 = 0x00222222;

static REGISTER_RESULT_CLASS: Once = Once::new();

// Helper to apply rounded corners to the edit control
unsafe fn set_rounded_edit_region(h_edit: HWND, w: i32, h: i32) {
    // radius (12, 12) matches the overlay style
    let rgn = CreateRoundRectRgn(0, 0, w, h, 12, 12);
    SetWindowRgn(h_edit, rgn, true);
}

pub fn create_result_window(
    target_rect: RECT,
    win_type: WindowType,
    context: RefineContext,
    model_id: String,
    provider: String,
    streaming_enabled: bool
) -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        REGISTER_RESULT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap(); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS; 
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
                CURRENT_BG_COLOR = 0x002d4a22; 
                (target_rect.left, target_rect.top, 0x002d4a22)
            },
            WindowType::Secondary => {
                let padding = 10;
                let hmonitor = MonitorFromRect(&target_rect, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO::default();
                mi.cbSize = size_of::<MONITORINFO>() as u32;
                GetMonitorInfoW(hmonitor, &mut mi);
                let work_rect = mi.rcWork;

                let pos_right_x = target_rect.right + padding;
                let pos_left_x  = target_rect.left - width - padding;
                let pos_bottom_y = target_rect.bottom + padding;
                let pos_top_y    = target_rect.top - height - padding;

                let space_right  = work_rect.right - pos_right_x;
                let space_left   = (target_rect.left - padding) - work_rect.left;
                let space_bottom = work_rect.bottom - pos_bottom_y;
                let space_top    = (target_rect.top - padding) - work_rect.top;

                let (mut best_x, mut best_y) = if space_right >= width {
                    (pos_right_x, target_rect.top)
                } else if space_bottom >= height {
                    (target_rect.left, pos_bottom_y)
                } else if space_left >= width {
                    (pos_left_x, target_rect.top)
                } else if space_top >= height {
                    (target_rect.left, pos_top_y)
                } else {
                    let max_space = space_right.max(space_left).max(space_bottom).max(space_top);
                    if max_space == space_right { (pos_right_x, target_rect.top) } 
                    else if max_space == space_left { (pos_left_x, target_rect.top) } 
                    else if max_space == space_bottom { (target_rect.left, pos_bottom_y) } 
                    else { (target_rect.left, pos_top_y) }
                };
                
                let safe_w = width.min(work_rect.right - work_rect.left);
                let safe_h = height.min(work_rect.bottom - work_rect.top);
                best_x = best_x.clamp(work_rect.left, work_rect.right - safe_w);
                best_y = best_y.clamp(work_rect.top, work_rect.bottom - safe_h);

                CURRENT_BG_COLOR = 0x002d4a22; 
                (best_x, best_y, 0x002d4a22)
            }
        };

        // FIX 1: Add WS_CLIPCHILDREN to prevent parent drawing over child (Fixes Blinking)
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name,
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN, 
            x, y, width, height,
            None, None, instance, None
        );

        let edit_style = WINDOW_STYLE(
            WS_CHILD.0 | 
            WS_BORDER.0 | 
            WS_CLIPSIBLINGS.0 |
            (ES_MULTILINE as u32) |
            (ES_AUTOVSCROLL as u32)
        );
        
        let h_edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            edit_style,
            0, 0, 0, 0, // Sized dynamically
            hwnd,
            HMENU(101),
            instance,
            None
        );
        
        let hfont = CreateFontW(14, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        SendMessageW(h_edit, WM_SETFONT, WPARAM(hfont.0 as usize), LPARAM(1));

        let mut physics = CursorPhysics::default();
        physics.initialized = true;

        {
            let mut states = WINDOW_STATES.lock().unwrap();
            states.insert(hwnd.0 as isize, WindowState {
                alpha: 220,
                is_hovered: false,
                on_copy_btn: false,
                copy_success: false,
                on_edit_btn: false,
                is_editing: false,
                edit_hwnd: h_edit,
                context_data: context,
                full_text: String::new(),
                is_refining: false,
                animation_offset: 0.0,
                model_id,
                provider,
                streaming_enabled,
                bg_color: color,
                linked_window: None,
                physics,
                interaction_mode: InteractionMode::None,
                current_resize_edge: ResizeEdge::None,
                drag_start_mouse: POINT { x: 0, y: 0 },
                drag_start_window_rect: RECT::default(),
                has_moved_significantly: false,
                font_cache_dirty: true,
                cached_font_size: 72,
                content_bitmap: HBITMAP(0),
                last_w: 0,
                last_h: 0,
                pending_text: None,
                last_text_update_time: 0,
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
        state.full_text = text.to_string();
    }
}

fn get_copy_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let btn_size = 28;
    let margin = 12;
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

fn get_edit_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let copy_rect = get_copy_btn_rect(window_w, window_h);
    let gap = 8;
    let width = copy_rect.right - copy_rect.left;
    RECT {
        left: copy_rect.left - width - gap,
        top: copy_rect.top,
        right: copy_rect.left - gap,
        bottom: copy_rect.bottom
    }
}

fn get_resize_edge(width: i32, height: i32, x: i32, y: i32) -> ResizeEdge {
    let margin = 8;
    let left = x < margin;
    let right = x >= width - margin;
    let top = y < margin;
    let bottom = y >= height - margin;

    if top && left { ResizeEdge::TopLeft }
    else if top && right { ResizeEdge::TopRight }
    else if bottom && left { ResizeEdge::BottomLeft }
    else if bottom && right { ResizeEdge::BottomRight }
    else if left { ResizeEdge::Left }
    else if right { ResizeEdge::Right }
    else if top { ResizeEdge::Top }
    else if bottom { ResizeEdge::Bottom }
    else { ResizeEdge::None }
}

unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as isize);
            SetBkMode(hdc, OPAQUE);
            SetBkColor(hdc, COLORREF(0x00FFFFFF)); 
            SetTextColor(hdc, COLORREF(0x00000000));
            let hbrush = GetStockObject(WHITE_BRUSH);
            LRESULT(hbrush.0 as isize)
        }
        
        WM_SETCURSOR => {
            let mut cursor_id = PCWSTR(std::ptr::null());
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            ScreenToClient(hwnd, &mut pt);
            
            let edge = get_resize_edge(rect.right, rect.bottom, pt.x, pt.y);
            
            match edge {
                ResizeEdge::Top | ResizeEdge::Bottom => cursor_id = IDC_SIZENS,
                ResizeEdge::Left | ResizeEdge::Right => cursor_id = IDC_SIZEWE,
                ResizeEdge::TopLeft | ResizeEdge::BottomRight => cursor_id = IDC_SIZENWSE,
                ResizeEdge::TopRight | ResizeEdge::BottomLeft => cursor_id = IDC_SIZENESW,
                ResizeEdge::None => {
                    let copy_rect = get_copy_btn_rect(rect.right, rect.bottom);
                    let edit_rect = get_edit_btn_rect(rect.right, rect.bottom);
                    let on_copy = pt.x >= copy_rect.left && pt.x <= copy_rect.right && pt.y >= copy_rect.top && pt.y <= copy_rect.bottom;
                    let on_edit = pt.x >= edit_rect.left && pt.x <= edit_rect.right && pt.y >= edit_rect.top && pt.y <= edit_rect.bottom;
                    if on_copy || on_edit {
                        cursor_id = IDC_HAND;
                    }
                }
            }
            
            if !cursor_id.0.is_null() {
                 SetCursor(LoadCursorW(None, cursor_id).unwrap());
                 LRESULT(1)
            } else {
                 SetCursor(HCURSOR(0));
                 LRESULT(1)
            }
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let width = rect.right;
            let height = rect.bottom;
            
            let edge = get_resize_edge(width, height, x, y);
            
            let mut window_rect = RECT::default();
            GetWindowRect(hwnd, &mut window_rect);
            
            let mut screen_pt = POINT::default();
            GetCursorPos(&mut screen_pt);

            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                state.drag_start_mouse = screen_pt;
                state.drag_start_window_rect = window_rect;
                state.has_moved_significantly = false;
                
                if edge != ResizeEdge::None {
                    state.interaction_mode = InteractionMode::Resizing(edge);
                } else {
                    state.interaction_mode = InteractionMode::DraggingWindow;
                }
            }
            SetCapture(hwnd);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let hover_edge = get_resize_edge(rect.right, rect.bottom, x as i32, y as i32);
            
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                state.current_resize_edge = hover_edge;

                let dx = x - state.physics.x;
                let drag_impulse = if state.interaction_mode == InteractionMode::DraggingWindow { 0.0 } else { (dx * 1.5).clamp(-20.0, 20.0) };
                state.physics.tilt_velocity -= drag_impulse * 0.2; 
                state.physics.current_tilt = state.physics.current_tilt.clamp(-22.5, 22.5);
                state.physics.x = x;
                state.physics.y = y;
                
                let copy_rect = get_copy_btn_rect(rect.right, rect.bottom);
                let edit_rect = get_edit_btn_rect(rect.right, rect.bottom);
                let padding = 4;
                state.on_copy_btn = 
                    x as i32 >= copy_rect.left - padding && 
                    x as i32 <= copy_rect.right + padding && 
                    y as i32 >= copy_rect.top - padding && 
                    y as i32 <= copy_rect.bottom + padding;
                state.on_edit_btn = 
                    x as i32 >= edit_rect.left - padding && 
                    x as i32 <= edit_rect.right + padding && 
                    y as i32 >= edit_rect.top - padding && 
                    y as i32 <= edit_rect.bottom + padding;

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

                match state.interaction_mode {
                    InteractionMode::DraggingWindow => {
                        let mut curr_pt = POINT::default();
                        GetCursorPos(&mut curr_pt);
                        
                        let dx = curr_pt.x - state.drag_start_mouse.x;
                        let dy = curr_pt.y - state.drag_start_mouse.y;
                        
                        if dx.abs() > 3 || dy.abs() > 3 {
                            state.has_moved_significantly = true;
                        }
                        
                        let new_x = state.drag_start_window_rect.left + dx;
                        let new_y = state.drag_start_window_rect.top + dy;
                        
                        SetWindowPos(hwnd, HWND(0), new_x, new_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    InteractionMode::Resizing(edge) => {
                        state.has_moved_significantly = true;
                        
                        let mut curr_pt = POINT::default();
                        GetCursorPos(&mut curr_pt);
                        let dx = curr_pt.x - state.drag_start_mouse.x;
                        let dy = curr_pt.y - state.drag_start_mouse.y;
                        
                        let mut new_rect = state.drag_start_window_rect;
                        let min_w = 20;
                        let min_h = 20;
                        
                        match edge {
                            ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight => {
                                new_rect.right = (state.drag_start_window_rect.right + dx).max(state.drag_start_window_rect.left + min_w);
                            }
                            ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft => {
                                new_rect.left = (state.drag_start_window_rect.left + dx).min(state.drag_start_window_rect.right - min_w);
                            }
                            _ => {}
                        }
                        match edge {
                            ResizeEdge::Bottom | ResizeEdge::BottomRight | ResizeEdge::BottomLeft => {
                                new_rect.bottom = (state.drag_start_window_rect.bottom + dy).max(state.drag_start_window_rect.top + min_h);
                            }
                            ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => {
                                new_rect.top = (state.drag_start_window_rect.top + dy).min(state.drag_start_window_rect.bottom - min_h);
                            }
                            _ => {}
                        }
                        
                        let w = new_rect.right - new_rect.left;
                        let h = new_rect.bottom - new_rect.top;
                        SetWindowPos(hwnd, HWND(0), new_rect.left, new_rect.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);

                        // FIX 5: Dynamic Edit Field Resizing
                        // If we are resizing the overlay, we must resize the edit control to match new dimensions
                        if state.is_editing {
                             let edit_w = w - 20;
                             let edit_h = 40;
                             SetWindowPos(state.edit_hwnd, HWND_TOP, 10, 10, edit_w, edit_h, SWP_NOACTIVATE);
                             // FIX 2: Re-apply rounded region on resize
                             set_rounded_edit_region(state.edit_hwnd, edit_w, edit_h);
                        }
                    }
                    _ => {}
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
                state.current_resize_edge = ResizeEdge::None; 
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            ReleaseCapture();
            let mut perform_click = false;
            let mut is_copy_click = false;
            let mut is_edit_click = false;
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.interaction_mode = InteractionMode::None;
                    
                    if !state.has_moved_significantly {
                        perform_click = true;
                        is_copy_click = state.on_copy_btn;
                        is_edit_click = state.on_edit_btn;
                    }
                }
            }
            
            if perform_click {
                 if is_edit_click {
                    let mut show = false;
                    let mut h_edit = HWND(0);
                    {
                        let mut states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                            state.is_editing = !state.is_editing;
                            show = state.is_editing;
                            h_edit = state.edit_hwnd;
                        }
                    }
                    
                    if show {
                        let mut rect = RECT::default();
                        GetClientRect(hwnd, &mut rect);
                        let w = rect.right - 20;
                        let h = 40; 
                        SetWindowPos(h_edit, HWND_TOP, 10, 10, w, h, SWP_SHOWWINDOW);
                        // FIX 2: Apply rounded corners when showing
                        set_rounded_edit_region(h_edit, w, h);
                        SetFocus(h_edit);
                    } else {
                        ShowWindow(h_edit, SW_HIDE);
                    }
                 } else if is_copy_click {
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
                 } else {
                      {
                         let mut states = WINDOW_STATES.lock().unwrap();
                         if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                             state.physics.mode = AnimationMode::Smashing;
                             state.physics.state_timer = 0.0;
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
                  }
            }
            LRESULT(0)
        }
        
        WM_RBUTTONUP => {
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
            LRESULT(0)
        }

        WM_TIMER => {
            let mut need_repaint = false;
            let mut pending_update: Option<String> = None;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u32)
                .unwrap_or(0);
            
            let mut trigger_refine = false;
            let mut user_prompt = String::new();
            let mut text_to_refine = String::new();
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                     // NEW: Handle animation updates if refining
                     if state.is_refining {
                         // Rapid Clockwise Animation for Processing (similar to recording.rs)
                         state.animation_offset -= 8.0; 
                         if state.animation_offset < -3600.0 { state.animation_offset += 3600.0; }
                         need_repaint = true;
                     }

                     if state.pending_text.is_some() && 
                        (state.last_text_update_time == 0 || now.wrapping_sub(state.last_text_update_time) > 66) {
                          
                          pending_update = state.pending_text.take();
                          state.last_text_update_time = now;
                      }
                      
                      if state.is_editing && GetFocus() == state.edit_hwnd {
                           // FIX 4: Check for ESCAPE to dismiss edit (exit edit mode)
                           if (GetKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 {
                               state.is_editing = false;
                               SetWindowTextW(state.edit_hwnd, w!("")); // Optional: Clear text on dismiss?
                               ShowWindow(state.edit_hwnd, SW_HIDE);
                               SetFocus(hwnd); // Return focus to parent
                           }
                           // FIX 3: Check for ENTER. 
                           // If Shift is NOT pressed, Submit. 
                           // If Shift IS pressed, do nothing (Edit control handles newline).
                           else if (GetKeyState(VK_RETURN.0 as i32) as u16 & 0x8000) != 0 {
                               let shift_pressed = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                               
                               if !shift_pressed {
                                   let len = GetWindowTextLengthW(state.edit_hwnd) + 1;
                                   let mut buf = vec![0u16; len as usize];
                                   GetWindowTextW(state.edit_hwnd, &mut buf);
                                   user_prompt = String::from_utf16_lossy(&buf[..len as usize - 1]).to_string();
                                   
                                   // FIX: Capture text BEFORE clearing it
                                   text_to_refine = state.full_text.clone();

                                   SetWindowTextW(state.edit_hwnd, w!(""));
                                   ShowWindow(state.edit_hwnd, SW_HIDE);
                                   state.is_editing = false;
                                   trigger_refine = true;
                                   
                                   // NEW: Enable refining state immediately to show animation
                                   state.is_refining = true;
                                   state.full_text = String::new(); // Clear previous text so animation is visible
                                   state.pending_text = Some(String::new()); // Force clear update
                               }
                           }
                       }
                }
            }

            if let Some(txt) = pending_update {
                let wide_text = to_wstring(&txt);
                SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.font_cache_dirty = true;
                    state.full_text = txt.clone();
                }
                need_repaint = true;
            }

            if trigger_refine && !user_prompt.trim().is_empty() {
                  let (context_data, model_id, provider, streaming) = {
                      let states = WINDOW_STATES.lock().unwrap();
                      if let Some(s) = states.get(&(hwnd.0 as isize)) {
                          (s.context_data.clone(), s.model_id.clone(), s.provider.clone(), s.streaming_enabled)
                      } else {
                          (RefineContext::None, "scout".to_string(), "groq".to_string(), false)
                      }
                  };
                  
                  // Use captured text
                  let previous_text = text_to_refine;

                  std::thread::spawn(move || {
                      let (groq_key, gemini_key) = {
                          let app = crate::APP.lock().unwrap();
                          (app.config.api_key.clone(), app.config.gemini_api_key.clone())
                      };

                      let mut acc_text = String::new();
                      let mut first_chunk = true;

                      let result = crate::api::refine_text_streaming(
                           &groq_key, &gemini_key, 
                           context_data, previous_text, user_prompt,
                           &model_id, &provider, streaming,
                           move |chunk| {
                               let mut states = WINDOW_STATES.lock().unwrap();
                               if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                   // Stop animation on first chunk
                                   if first_chunk {
                                       state.is_refining = false;
                                       first_chunk = false;
                                   }
                                   
                                   acc_text.push_str(chunk); 
                                   state.pending_text = Some(acc_text.clone());
                                   state.full_text = acc_text.clone();
                               }
                           }
                      );
                      
                      // Ensure it stops if finished without chunks (e.g. error or empty)
                      let mut states = WINDOW_STATES.lock().unwrap();
                      if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                          state.is_refining = false;
                          // FIX: Handle error result and show it
                          if let Err(e) = result {
                              let err_msg = format!("Error: {}", e);
                              state.pending_text = Some(err_msg.clone());
                              state.full_text = err_msg;
                          }
                      }
                  });
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
