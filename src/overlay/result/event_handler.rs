use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::mem::size_of;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, Mutex};

use crate::overlay::utils::to_wstring;
use super::state::{WINDOW_STATES, InteractionMode, ResizeEdge, RefineContext, link_windows, WindowType};
use super::layout::{get_copy_btn_rect, get_edit_btn_rect, get_undo_btn_rect, get_resize_edge};
use super::logic;
use super::paint;
use super::window::{create_result_window, update_window_text}; 

// Helper to apply rounded corners (duplicate needed since it's private in window.rs)
unsafe fn set_rounded_edit_region(h_edit: HWND, w: i32, h: i32) {
    let rgn = CreateRoundRectRgn(0, 0, w, h, 12, 12);
    SetWindowRgn(h_edit, rgn, true);
}

pub unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
            
            let mut is_over_edit = false;
            {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    if state.is_editing {
                        let edit_left = 10;
                        let edit_top = 10;
                        let edit_right = rect.right - 10;
                        let edit_bottom = 10 + 40;
                        
                        is_over_edit = pt.x >= edit_left && pt.x <= edit_right 
                                    && pt.y >= edit_top && pt.y <= edit_bottom;
                    }
                }
            }
            
            if is_over_edit {
                SetCursor(LoadCursorW(None, IDC_IBEAM).unwrap());
                return LRESULT(1);
            }
            
            let edge = get_resize_edge(rect.right, rect.bottom, pt.x, pt.y);
            
            match edge {
                ResizeEdge::Top | ResizeEdge::Bottom => cursor_id = IDC_SIZENS,
                ResizeEdge::Left | ResizeEdge::Right => cursor_id = IDC_SIZEWE,
                ResizeEdge::TopLeft | ResizeEdge::BottomRight => cursor_id = IDC_SIZENWSE,
                ResizeEdge::TopRight | ResizeEdge::BottomLeft => cursor_id = IDC_SIZENESW,
                ResizeEdge::None => {
                    let copy_rect = get_copy_btn_rect(rect.right, rect.bottom);
                    let edit_rect = get_edit_btn_rect(rect.right, rect.bottom);
                    let undo_rect = get_undo_btn_rect(rect.right, rect.bottom);
                    
                    let on_copy = pt.x >= copy_rect.left && pt.x <= copy_rect.right && pt.y >= copy_rect.top && pt.y <= copy_rect.bottom;
                    let on_edit = pt.x >= edit_rect.left && pt.x <= edit_rect.right && pt.y >= edit_rect.top && pt.y <= edit_rect.bottom;
                    
                    let mut has_history = false;
                    {
                        let states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get(&(hwnd.0 as isize)) {
                            has_history = !state.text_history.is_empty();
                        }
                    }
                    
                    let on_undo = has_history && pt.x >= undo_rect.left && pt.x <= undo_rect.right && pt.y >= undo_rect.top && pt.y <= undo_rect.bottom;
                    
                    if on_copy || on_edit || on_undo {
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
                let undo_rect = get_undo_btn_rect(rect.right, rect.bottom);
                let padding = 4;
                state.on_copy_btn = x as i32 >= copy_rect.left - padding && x as i32 <= copy_rect.right + padding && y as i32 >= copy_rect.top - padding && y as i32 <= copy_rect.bottom + padding;
                state.on_edit_btn = x as i32 >= edit_rect.left - padding && x as i32 <= edit_rect.right + padding && y as i32 >= edit_rect.top - padding && y as i32 <= edit_rect.bottom + padding;
                if !state.text_history.is_empty() {
                    state.on_undo_btn = x as i32 >= undo_rect.left - padding && x as i32 <= undo_rect.right + padding && y as i32 >= undo_rect.top - padding && y as i32 <= undo_rect.bottom + padding;
                } else {
                    state.on_undo_btn = false;
                }

                if !state.is_hovered {
                    state.is_hovered = true;
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    TrackMouseEvent(&mut tme);
                }

                match state.interaction_mode {
                    InteractionMode::DraggingWindow => {
                        let mut curr_pt = POINT::default();
                        GetCursorPos(&mut curr_pt);
                        let dx = curr_pt.x - state.drag_start_mouse.x;
                        let dy = curr_pt.y - state.drag_start_mouse.y;
                        if dx.abs() > 3 || dy.abs() > 3 { state.has_moved_significantly = true; }
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
                        let min_w = 20; let min_h = 20;
                        match edge {
                            ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight => { new_rect.right = (state.drag_start_window_rect.right + dx).max(state.drag_start_window_rect.left + min_w); }
                            ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft => { new_rect.left = (state.drag_start_window_rect.left + dx).min(state.drag_start_window_rect.right - min_w); }
                            _ => {}
                        }
                        match edge {
                            ResizeEdge::Bottom | ResizeEdge::BottomRight | ResizeEdge::BottomLeft => { new_rect.bottom = (state.drag_start_window_rect.bottom + dy).max(state.drag_start_window_rect.top + min_h); }
                            ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => { new_rect.top = (state.drag_start_window_rect.top + dy).min(state.drag_start_window_rect.bottom - min_h); }
                            _ => {}
                        }
                        let w = new_rect.right - new_rect.left;
                        let h = new_rect.bottom - new_rect.top;
                        SetWindowPos(hwnd, HWND(0), new_rect.left, new_rect.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                        if state.is_editing {
                             let edit_w = w - 20;
                             let edit_h = 40;
                             SetWindowPos(state.edit_hwnd, HWND_TOP, 10, 10, edit_w, edit_h, SWP_NOACTIVATE);
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
                state.on_undo_btn = false; 
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
            let mut is_undo_click = false;
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.interaction_mode = InteractionMode::None;
                    if !state.has_moved_significantly {
                        perform_click = true;
                        is_copy_click = state.on_copy_btn;
                        is_edit_click = state.on_edit_btn;
                        is_undo_click = state.on_undo_btn;
                    }
                }
            }
            
            if perform_click {
                 if is_undo_click {
                    let mut prev_text = None;
                    {
                        let mut states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                            if let Some(last) = state.text_history.pop() {
                                prev_text = Some(last.clone());
                                state.full_text = last;
                            }
                        }
                    }
                    if let Some(txt) = prev_text {
                        let wide_text = to_wstring(&txt);
                        SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                        let mut states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                            state.font_cache_dirty = true;
                        }
                        InvalidateRect(hwnd, None, false);
                    }
                 } else if is_edit_click {
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
                      let linked_hwnd = {
                          let states = WINDOW_STATES.lock().unwrap();
                          if let Some(state) = states.get(&(hwnd.0 as isize)) { state.linked_window } else { None }
                      };
                      if let Some(linked) = linked_hwnd {
                          if IsWindow(linked).as_bool() {
                              PostMessageW(linked, WM_CLOSE, WPARAM(0), LPARAM(0));
                          }
                      }
                      PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
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

        WM_MBUTTONUP => {
            let mut targets = Vec::new();
            {
                if let Ok(states) = WINDOW_STATES.lock() {
                    for (&hwnd_int, _) in states.iter() {
                        targets.push(HWND(hwnd_int));
                    }
                }
            }

            for target in targets {
                if IsWindow(target).as_bool() {
                    PostMessageW(target, WM_CLOSE, WPARAM(0), LPARAM(0));
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
            
            let mut trigger_refine = false;
            let mut user_input = String::new();
            let mut text_to_refine = String::new();
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                     // Handle animation updates if refining
                     if state.is_refining {
                         state.animation_offset -= 8.0; 
                         if state.animation_offset < -3600.0 { state.animation_offset += 3600.0; }
                         need_repaint = true;
                     }

                     // Throttle
                     if state.pending_text.is_some() && 
                        (state.last_text_update_time == 0 || now.wrapping_sub(state.last_text_update_time) > 16) {
                          
                          pending_update = state.pending_text.take();
                          state.last_text_update_time = now;
                      }
                      
                      if state.is_editing && GetFocus() == state.edit_hwnd {
                           // ESCAPE to dismiss edit
                           if (GetKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 {
                               state.is_editing = false;
                               SetWindowTextW(state.edit_hwnd, w!("")); 
                               ShowWindow(state.edit_hwnd, SW_HIDE);
                               SetFocus(hwnd); 
                           }
                           // Ctrl+A to Select All
                           else if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 
                               && (GetKeyState(0x41) as u16 & 0x8000) != 0 { 
                               const EM_SETSEL: u32 = 0x00B1;
                               SendMessageW(state.edit_hwnd, EM_SETSEL, WPARAM(0), LPARAM(-1));
                           }
                           // ENTER to submit (unless Shift is held)
                           else if (GetKeyState(VK_RETURN.0 as i32) as u16 & 0x8000) != 0 {
                               let shift_pressed = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                               
                               if !shift_pressed {
                                   let len = GetWindowTextLengthW(state.edit_hwnd) + 1;
                                   let mut buf = vec![0u16; len as usize];
                                   GetWindowTextW(state.edit_hwnd, &mut buf);
                                   user_input = String::from_utf16_lossy(&buf[..len as usize - 1]).to_string();
                                   
                                   // Capture text BEFORE clearing it
                                   text_to_refine = state.full_text.clone();

                                   // Save current state to history
                                   state.text_history.push(text_to_refine.clone());
                                   
                                   SetWindowTextW(state.edit_hwnd, w!(""));
                                   ShowWindow(state.edit_hwnd, SW_HIDE);
                                   state.is_editing = false;
                                   trigger_refine = true;
                                   
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

            // --- TYPE MODE PROMPT LOGIC ---
            if trigger_refine && !user_input.trim().is_empty() {
                  let (context_data, model_id, provider, streaming, preset_prompt, _retrans_config_opt) = {
                      let states = WINDOW_STATES.lock().unwrap();
                      if let Some(s) = states.get(&(hwnd.0 as isize)) {
                          (s.context_data.clone(), s.model_id.clone(), s.provider.clone(), s.streaming_enabled, s.preset_prompt.clone(), s.retrans_config.clone())
                      } else {
                          (RefineContext::None, "scout".to_string(), "groq".to_string(), false, "".to_string(), None)
                      }
                  };
                  
                  let (final_prev_text, final_user_prompt) = if text_to_refine.trim().is_empty() && !preset_prompt.is_empty() {
                       (user_input, preset_prompt)
                  } else {
                       (text_to_refine, user_input)
                  };

                  std::thread::spawn(move || {
                      let (groq_key, gemini_key) = {
                          let app = crate::APP.lock().unwrap();
                          (app.config.api_key.clone(), app.config.gemini_api_key.clone())
                      };

                      let mut acc_text = String::new();
                      let mut first_chunk = true;

                      let result = crate::api::refine_text_streaming(
                           &groq_key, &gemini_key, 
                           context_data, final_prev_text, final_user_prompt,
                           &model_id, &provider, streaming,
                           move |chunk| {
                               let mut states = WINDOW_STATES.lock().unwrap();
                               if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
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
                      
                      // Removed redundant retranslation trigger block.
                      // Refinement should ONLY update the current window, not spawn new windows.

                      let mut states = WINDOW_STATES.lock().unwrap();
                      if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                          state.is_refining = false;
                          if let Err(e) = result {
                              let lang = {
                                  let app = crate::APP.lock().unwrap();
                                  app.config.ui_language.clone()
                              };
                              let err_msg = crate::overlay::utils::get_error_message(&e.to_string(), &lang);
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
                if state.edit_font.0 != 0 {
                    DeleteObject(state.edit_font);
                }
            }
            LRESULT(0)
        }

        WM_PAINT => {
            paint::paint_window(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
