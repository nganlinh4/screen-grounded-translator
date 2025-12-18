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
use super::layout::{get_copy_btn_rect, get_edit_btn_rect, get_undo_btn_rect, get_redo_btn_rect, get_markdown_btn_rect, get_download_btn_rect, get_resize_edge};
use super::logic;
use super::paint;
use super::window::{create_result_window, update_window_text};
use super::markdown_view;
use super::refine_input;

// Custom message to defer WebView2 creation (avoids deadlock in button handler)
const WM_CREATE_WEBVIEW: u32 = WM_USER + 200; 

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
                    let mut is_browsing = false;
                    {
                        let states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get(&(hwnd.0 as isize)) {
                            has_history = !state.text_history.is_empty();
                            is_browsing = state.is_browsing;
                        }
                    }
                    
                    // Manual calc for Back button rect
                    let btn_size = 28;
                    let margin = 12;
                    let threshold_h = btn_size + (margin * 2);
                    let cy = if rect.bottom < threshold_h {
                        (rect.bottom as f32) / 2.0
                    } else {
                        (rect.bottom - margin - btn_size / 2) as f32
                    };
                    let cx_back = (margin + btn_size / 2) as i32;
                    let cy_back = cy as i32;
                    let back_rect = RECT { 
                        left: cx_back - 14, top: cy_back - 14, 
                        right: cx_back + 14, bottom: cy_back + 14 
                    };
                    
                    let on_back = is_browsing && pt.x >= back_rect.left && pt.x <= back_rect.right && pt.y >= back_rect.top && pt.y <= back_rect.bottom;
                    
                    let on_undo = has_history && pt.x >= undo_rect.left && pt.x <= undo_rect.right && pt.y >= undo_rect.top && pt.y <= undo_rect.bottom;
                    
                    let md_rect = get_markdown_btn_rect(rect.right, rect.bottom);
                    let on_md = pt.x >= md_rect.left && pt.x <= md_rect.right && pt.y >= md_rect.top && pt.y <= md_rect.bottom;
                    
                    let dl_rect = get_download_btn_rect(rect.right, rect.bottom);
                    let on_dl = pt.x >= dl_rect.left && pt.x <= dl_rect.right && pt.y >= dl_rect.top && pt.y <= dl_rect.bottom;
                    
                    if on_copy || on_edit || on_undo || on_md || on_back || on_dl {
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

        WM_RBUTTONDOWN => {
            let mut screen_pt = POINT::default();
            GetCursorPos(&mut screen_pt);
            
            let mut group_snapshot = Vec::new();
            let mut token_to_match = None;
            
            {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                     token_to_match = state.cancellation_token.clone();
                }
                
                // Strategy 1: Cancellation Token (Group Identity)
                if let Some(token) = token_to_match {
                    for (&h_val, s) in states.iter() {
                         if let Some(ref t) = s.cancellation_token {
                             if Arc::ptr_eq(&token, t) {
                                 let h = HWND(h_val as isize);
                                 let mut r = RECT::default();
                                 GetWindowRect(h, &mut r);
                                 group_snapshot.push((h, r));
                             }
                         }
                    }
                }

                // Strategy 2: Linked Window Chain (Fallback/Augment if Token logic was insufficient)
                // If we found 0 or 1 windows, it might just be an un-tokenized chain.
                if group_snapshot.len() <= 1 {
                     group_snapshot.clear(); // Restart to build full chain
                     
                     let mut visited = std::collections::HashSet::new();
                     let mut queue = std::collections::VecDeque::new();
                     
                     queue.push_back(hwnd);
                     visited.insert(hwnd.0);
                     
                     while let Some(current) = queue.pop_front() {
                         let mut r = RECT::default();
                         GetWindowRect(current, &mut r);
                         group_snapshot.push((current, r));
                         
                         // Find neighbor in state map
                         if let Some(s) = states.get(&(current.0 as isize)) {
                             if let Some(linked) = s.linked_window {
                                 // Basic validation that window is still managed
                                 if states.contains_key(&(linked.0 as isize)) {
                                     if !visited.contains(&linked.0) {
                                         visited.insert(linked.0);
                                         queue.push_back(linked);
                                     }
                                 }
                             }
                         }
                     }
                }
            }
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.drag_start_mouse = screen_pt;
                    state.has_moved_significantly = false;
                    state.interaction_mode = InteractionMode::DraggingGroup(group_snapshot);
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
            
            // Defer group moves to avoid deadlocks (holding lock while calling SetWindowPos on other windows)
            let mut group_moves = Vec::new();

            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.current_resize_edge = hover_edge;
                    let dx = x - state.physics.x;
                    let drag_impulse = if matches!(&state.interaction_mode, InteractionMode::DraggingWindow | InteractionMode::DraggingGroup(_)) { 0.0 } else { (dx * 1.5).clamp(-20.0, 20.0) };
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
                    
                    // Redo button hover state
                    let redo_rect = get_redo_btn_rect(rect.right, rect.bottom);
                    if !state.redo_history.is_empty() {
                        state.on_redo_btn = x as i32 >= redo_rect.left - padding && x as i32 <= redo_rect.right + padding && y as i32 >= redo_rect.top - padding && y as i32 <= redo_rect.bottom + padding;
                    } else {
                        state.on_redo_btn = false;
                    }
                    
                    // Calc Back Button state
                    if state.is_browsing {
                         let btn_size = 28;
                         let margin = 12;
                         let threshold_h = btn_size + (margin * 2);
                         let cy = if rect.bottom < threshold_h {
                            (rect.bottom as f32) / 2.0
                        } else {
                            (rect.bottom - margin - btn_size / 2) as f32
                        };
                        let cx_back = (margin + btn_size / 2) as i32;
                        let cy_back = cy as i32;
                        
                        // use padding like other buttons
                        let l = cx_back - 14 - padding;
                        let r = cx_back + 14 + padding;
                        let t = cy_back - 14 - padding;
                        let b = cy_back + 14 + padding;
                        
                        state.on_back_btn = x as i32 >= l && x as i32 <= r && y as i32 >= t && y as i32 <= b;
                    } else {
                        state.on_back_btn = false;
                    }
                    
                    let md_rect = get_markdown_btn_rect(rect.right, rect.bottom);
                    state.on_markdown_btn = x as i32 >= md_rect.left - padding && x as i32 <= md_rect.right + padding && y as i32 >= md_rect.top - padding && y as i32 <= md_rect.bottom + padding;

                    let dl_rect = get_download_btn_rect(rect.right, rect.bottom);
                    state.on_download_btn = x as i32 >= dl_rect.left - padding && x as i32 <= dl_rect.right + padding && y as i32 >= dl_rect.top - padding && y as i32 <= dl_rect.bottom + padding;

                    // In markdown mode, let the Timer handle is_hovered state to ensure it syncs with WebView resize
                    let handle_hover_in_mousemove = !state.is_markdown_mode;

                    if handle_hover_in_mousemove && !state.is_hovered {
                        state.is_hovered = true;
                        let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                        TrackMouseEvent(&mut tme);
                        // Note: WebView resize is now handled by Timer 2 to avoid race conditions
                    }

                    match &state.interaction_mode {
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
                        InteractionMode::DraggingGroup(snapshot) => {
                            let mut curr_pt = POINT::default();
                            GetCursorPos(&mut curr_pt);
                            let dx = curr_pt.x - state.drag_start_mouse.x;
                            let dy = curr_pt.y - state.drag_start_mouse.y;
                            if dx.abs() > 3 || dy.abs() > 3 { state.has_moved_significantly = true; }
                            
                            for (h, start_rect) in snapshot {
                                let new_x = start_rect.left + dx;
                                let new_y = start_rect.top + dy;
                                group_moves.push((*h, new_x, new_y));
                            }
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
                            // Resize markdown webview if in markdown mode
                            if state.is_markdown_mode {
                                markdown_view::resize_markdown_webview(hwnd, state.is_hovered);
                            }
                        }
                        _ => {}
                    }
                    InvalidateRect(hwnd, None, false);
                }
            } // Lock released (WINDOW_STATES)

            // Execute deferred group moves
            for (h, x, y) in group_moves {
                SetWindowPos(h, HWND(0), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }

            LRESULT(0)
        }

        0x02A3 => { // WM_MOUSELEAVE
            // Check if cursor is actually outside the window (not just moved to a child window like WebView)
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                // Clear button states
                state.on_copy_btn = false;
                state.on_edit_btn = false;
                state.on_undo_btn = false;
                state.on_redo_btn = false;
                state.on_markdown_btn = false;
                state.on_download_btn = false;
                state.current_resize_edge = ResizeEdge::None;
                
                // For plain text mode, also clear hover state here 
                // (markdown mode uses Timer 2 for this since WebView steals focus)
                if !state.is_markdown_mode {
                    state.is_hovered = false;
                }
                
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
            let mut is_redo_click = false;
            let mut is_markdown_click = false;
            let mut is_back_click = false;
            let mut is_download_click = false;
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.interaction_mode = InteractionMode::None;
                    if !state.has_moved_significantly {
                        perform_click = true;
                        is_copy_click = state.on_copy_btn;
                        is_edit_click = state.on_edit_btn;
                        is_undo_click = state.on_undo_btn;
                        is_redo_click = state.on_redo_btn;
                        is_markdown_click = state.on_markdown_btn;
                        is_back_click = state.on_back_btn;
                        is_download_click = state.on_download_btn;
                    }
                }
            }
            
            if perform_click {
                 if is_back_click {
                     markdown_view::go_back(hwnd);
                 } else if is_undo_click {
                    let mut prev_text = None;
                    let mut current_text_for_redo = String::new();
                    let mut is_markdown = false;
                    let mut is_hovered = false;
                    {
                        let mut states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                            if let Some(last) = state.text_history.pop() {
                                // Save current text to redo history before replacing
                                current_text_for_redo = state.full_text.clone();
                                prev_text = Some(last.clone());
                                state.full_text = last;
                                // Push current text to redo stack
                                if !current_text_for_redo.is_empty() {
                                    state.redo_history.push(current_text_for_redo);
                                }
                            }
                            is_markdown = state.is_markdown_mode;
                            is_hovered = state.is_hovered;
                        }
                    }
                    if let Some(txt) = prev_text {
                        let wide_text = to_wstring(&txt);
                        SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.font_cache_dirty = true;
                                // Reset browsing state since content changed
                                state.is_browsing = false;
                            }
                        }
                        
                        // Update markdown WebView if in markdown mode
                        if is_markdown {
                            markdown_view::create_markdown_webview(hwnd, &txt, is_hovered);
                        }
                        
                        InvalidateRect(hwnd, None, false);
                    }
                 } else if is_redo_click {
                    // Redo: pop from redo_history, push current to text_history
                    let mut next_text = None;
                    let mut current_text_for_undo = String::new();
                    let mut is_markdown = false;
                    let mut is_hovered = false;
                    {
                        let mut states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                            if let Some(redo_text) = state.redo_history.pop() {
                                // Save current text to undo history before replacing
                                current_text_for_undo = state.full_text.clone();
                                next_text = Some(redo_text.clone());
                                state.full_text = redo_text;
                                // Push current text back to undo stack
                                if !current_text_for_undo.is_empty() {
                                    state.text_history.push(current_text_for_undo);
                                }
                            }
                            is_markdown = state.is_markdown_mode;
                            is_hovered = state.is_hovered;
                        }
                    }
                    if let Some(txt) = next_text {
                        let wide_text = to_wstring(&txt);
                        SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.font_cache_dirty = true;
                                // Reset browsing state since content changed
                                state.is_browsing = false;
                            }
                        }
                        
                        // Update markdown WebView if in markdown mode
                        if is_markdown {
                            markdown_view::create_markdown_webview(hwnd, &txt, is_hovered);
                        }
                        
                        InvalidateRect(hwnd, None, false);
                    }
                 } else if is_edit_click {
                    // Check if we're in markdown mode to decide which input to use
                    let (is_markdown_mode, is_currently_editing, h_edit) = {
                        let states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get(&(hwnd.0 as isize)) {
                            (state.is_markdown_mode, state.is_editing, state.edit_hwnd)
                        } else {
                            (false, false, HWND(0))
                        }
                    };
                    
                    if is_markdown_mode {
                        // Use WebView-based refine input (stays above markdown view)
                        if refine_input::is_refine_input_active(hwnd) {
                            // Toggle off - hide the refine input
                            refine_input::hide_refine_input(hwnd);
                            {
                                let mut states = WINDOW_STATES.lock().unwrap();
                                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                    state.is_editing = false;
                                }
                            }
                            // Resize markdown WebView back to full
                            let is_hovered = {
                                let states = WINDOW_STATES.lock().unwrap();
                                states.get(&(hwnd.0 as isize)).map(|s| s.is_hovered).unwrap_or(false)
                            };
                            markdown_view::resize_markdown_webview(hwnd, is_hovered);
                        } else {
                            // Toggle on - show the refine input
                            let lang = {
                                let app = crate::APP.lock().unwrap();
                                app.config.ui_language.clone()
                            };
                            let locale = crate::gui::locale::LocaleText::get(&lang);
                            let placeholder = locale.text_input_placeholder;
                            
                            if refine_input::show_refine_input(hwnd, placeholder) {
                                {
                                    let mut states = WINDOW_STATES.lock().unwrap();
                                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                        state.is_editing = true;
                                    }
                                }
                                // Resize markdown WebView to leave room for refine input
                                // The refine input is at top, so markdown view shifts down
                                markdown_view::resize_markdown_webview(hwnd, true);
                            }
                        }
                    } else {
                        // Plain text mode: use native EDIT control (existing logic)
                        let show = !is_currently_editing;
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.is_editing = show;
                            }
                        }
                        
                        if show {
                            let mut rect = RECT::default();
                            GetClientRect(hwnd, &mut rect);
                            let w = rect.right - 20;
                            let h = 40;
                            SetWindowPos(h_edit, HWND_TOP, 10, 10, w, h, SWP_SHOWWINDOW);
                            set_rounded_edit_region(h_edit, w, h);
                            SetForegroundWindow(hwnd);
                            SetFocus(h_edit);
                        } else {
                            ShowWindow(h_edit, SW_HIDE);
                        }
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
                 } else if is_markdown_click {
                    // Only allow markdown toggle when NOT refining
                    let can_toggle = {
                        let states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get(&(hwnd.0 as isize)) {
                            !state.is_refining
                        } else {
                            false
                        }
                    };
                    
                    if can_toggle {
                        // Toggle markdown mode
                        let (toggle_on, full_text) = {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.is_markdown_mode = !state.is_markdown_mode;
                                (state.is_markdown_mode, state.full_text.clone())
                            } else {
                                (false, String::new())
                            }
                        };
                        
                        if toggle_on {
                            // DEFER WebView creation to after this handler returns
                            // Using PostMessage allows the handler to return first.
                            PostMessageW(hwnd, WM_CREATE_WEBVIEW, WPARAM(0), LPARAM(0));
                            // Start hover polling timer (ID 2, 30ms interval)
                            SetTimer(hwnd, 2, 30, None);
                        } else {
                            // Hide markdown webview, show plain text
                            markdown_view::hide_markdown_webview(hwnd);
                            // Stop hover polling timer
                            KillTimer(hwnd, 2);
                            
                            // Re-establish TrackMouseEvent for plain text mode
                            // This is needed because Timer 2 was handling hover state,
                            // but now we need WM_MOUSELEAVE to fire again
                            let mut tme = TRACKMOUSEEVENT { 
                                cbSize: size_of::<TRACKMOUSEEVENT>() as u32, 
                                dwFlags: TME_LEAVE, 
                                hwndTrack: hwnd, 
                                dwHoverTime: 0 
                            };
                            TrackMouseEvent(&mut tme);
                        }
                        InvalidateRect(hwnd, None, false);
                    }
                 } else if is_download_click {
                    // Download as HTML file
                    let full_text = {
                        let states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get(&(hwnd.0 as isize)) {
                            state.full_text.clone()
                        } else {
                            String::new()
                        }
                    };
                    
                    if !full_text.is_empty() {
                        // Call save_html_file which opens the file save dialog
                        markdown_view::save_html_file(&full_text);
                    }
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
            ReleaseCapture();
            let mut perform_action = false;
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                     match &state.interaction_mode {
                         InteractionMode::DraggingGroup(_) => {
                             if !state.has_moved_significantly {
                                 perform_action = true;
                             }
                         }
                         _ => {
                             perform_action = true; 
                         }
                     }
                     state.interaction_mode = InteractionMode::None;
                }
            }
            
            if perform_action {
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
            }
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
            let timer_id = wparam.0;
            
            // Timer ID 2: Markdown hover polling (The Authority on WebView Sizing)
            if timer_id == 2 {
                let mut cursor_pos = POINT::default();
                GetCursorPos(&mut cursor_pos);
                let mut window_rect = RECT::default();
                GetWindowRect(hwnd, &mut window_rect);
                
                // Check if cursor is geometrically inside the window rect
                let cursor_inside = cursor_pos.x >= window_rect.left && cursor_pos.x < window_rect.right
                                 && cursor_pos.y >= window_rect.top && cursor_pos.y < window_rect.bottom;
                
                let (is_markdown_mode, current_hover_state) = {
                    let states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get(&(hwnd.0 as isize)) {
                        (state.is_markdown_mode, state.is_hovered)
                    } else {
                        (false, false)
                    }
                };
                
                if is_markdown_mode {
                    // State change detection
                    if cursor_inside && !current_hover_state {
                        // Enter: Mark hovered -> Shrink WebView -> Buttons visible
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.is_hovered = true;
                            }
                        }
                        markdown_view::resize_markdown_webview(hwnd, true);
                        InvalidateRect(hwnd, None, false);
                    } else if !cursor_inside && current_hover_state {
                        // Leave: Mark unhovered -> Expand WebView -> Clean look
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.is_hovered = false;
                                state.on_copy_btn = false;
                                state.on_undo_btn = false;
                                state.on_markdown_btn = false;
                                state.on_download_btn = false;
                            }
                        }
                        markdown_view::resize_markdown_webview(hwnd, false);
                        InvalidateRect(hwnd, None, false);
                    }
                }
                
                return LRESULT(0);
            }
            
            // Timer ID 1 and other timers: existing logic
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
                      
                      // Handle native EDIT control input (plain text mode)
                      if state.is_editing && !state.is_markdown_mode && GetFocus() == state.edit_hwnd {
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
                                   // Clear redo history when new action is performed
                                   state.redo_history.clear();
                                   
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
                       
                       // Handle WebView-based refine input (markdown mode)
                       // Poll for IPC submit/cancel messages
                       if state.is_editing && state.is_markdown_mode {
                           // Note: IPC polling happens outside the lock to avoid deadlock
                           // We just mark that we need to check it
                       }
                }
            }
            
            // Poll WebView-based refine input outside of lock (IPC handler may need lock)
            {
                let is_refine_active = refine_input::is_refine_input_active(hwnd);
                if is_refine_active {
                    let (submitted, cancelled, input_text) = refine_input::poll_refine_input(hwnd);
                    
                    if submitted && !input_text.trim().is_empty() {
                        // User submitted from WebView refine input
                        user_input = input_text;
                        
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                text_to_refine = state.full_text.clone();
                                state.text_history.push(text_to_refine.clone());
                                // Clear redo history when new action is performed
                                state.redo_history.clear();
                                state.is_editing = false;
                                state.is_refining = true;
                                state.full_text = String::new();
                                state.pending_text = Some(String::new());
                            }
                        }
                        
                        // Hide the refine input
                        refine_input::hide_refine_input(hwnd);
                        
                        // Resize markdown WebView back to normal
                        let is_hovered = {
                            let states = WINDOW_STATES.lock().unwrap();
                            states.get(&(hwnd.0 as isize)).map(|s| s.is_hovered).unwrap_or(false)
                        };
                        markdown_view::resize_markdown_webview(hwnd, is_hovered);
                        
                        trigger_refine = true;
                    } else if cancelled {
                        // User cancelled - just hide the input
                        {
                            let mut states = WINDOW_STATES.lock().unwrap();
                            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                                state.is_editing = false;
                            }
                        }
                        refine_input::hide_refine_input(hwnd);
                        
                        // Resize markdown WebView back to normal
                        let is_hovered = {
                            let states = WINDOW_STATES.lock().unwrap();
                            states.get(&(hwnd.0 as isize)).map(|s| s.is_hovered).unwrap_or(false)
                        };
                        markdown_view::resize_markdown_webview(hwnd, is_hovered);
                    }
                }
            }

            if let Some(txt) = pending_update {
                let wide_text = to_wstring(&txt);
                SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
                
                let (maybe_markdown_update, is_hovered) = {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.font_cache_dirty = true;
                        state.full_text = txt.clone();
                        
                        if state.is_markdown_mode {
                            (Some(state.full_text.clone()), state.is_hovered)
                        } else {
                            (None, false)
                        }
                    } else {
                        (None, false)
                    }
                };

                if let Some(md_text) = maybe_markdown_update {
                    markdown_view::create_markdown_webview(hwnd, &md_text, is_hovered);
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
                           {
                               let app = crate::APP.lock().unwrap();
                               &app.config.ui_language.clone()
                           },
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
                              let (lang, model_full_name) = {
                                  let app = crate::APP.lock().unwrap();
                                  let full_name = crate::model_config::get_model_by_id(&model_id)
                                      .map(|m| m.full_name)
                                      .unwrap_or_else(|| model_id.clone());
                                  (app.config.ui_language.clone(), full_name)
                              };
                              let err_msg = crate::overlay::utils::get_error_message(&e.to_string(), &lang, Some(&model_full_name));
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
            // Collect windows to close (those sharing the same cancellation token)
            let windows_to_close: Vec<HWND>;
            let token_to_signal: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>;
            
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.remove(&(hwnd.0 as isize)) {
                    // Get the cancellation token from this window
                    token_to_signal = state.cancellation_token.clone();
                    
                    // Find all other windows with the same cancellation token
                    if let Some(ref token) = token_to_signal {
                        // Signal cancellation first
                        token.store(true, std::sync::atomic::Ordering::Relaxed);
                        
                        // Collect windows to close (can't close while iterating with lock held)
                        windows_to_close = states.iter()
                            .filter(|(_, s)| {
                                if let Some(ref other_token) = s.cancellation_token {
                                    std::sync::Arc::ptr_eq(token, other_token)
                                } else {
                                    false
                                }
                            })
                            .map(|(k, _)| HWND(*k as isize))
                            .collect();
                    } else {
                        windows_to_close = Vec::new();
                    }
                    
                    // Cleanup this window's resources
                    if state.content_bitmap.0 != 0 {
                        DeleteObject(state.content_bitmap);
                    }
                    if state.bg_bitmap.0 != 0 {
                        DeleteObject(state.bg_bitmap);
                    }
                    if state.edit_font.0 != 0 {
                        DeleteObject(state.edit_font);
                    }
                    
                    // Cleanup markdown webview and timer
                    KillTimer(hwnd, 2);
                    markdown_view::destroy_markdown_webview(hwnd);
                    
                    // Cleanup refine input if active
                    refine_input::hide_refine_input(hwnd);
                } else {
                    windows_to_close = Vec::new();
                    token_to_signal = None;
                }
            }
            
            // Close all other windows in the same chain (after dropping the lock)
            for other_hwnd in windows_to_close {
                if other_hwnd != hwnd {
                    PostMessageW(other_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
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
        
        // Deferred WebView2 creation - handles the WM_CREATE_WEBVIEW we posted
        msg if msg == WM_CREATE_WEBVIEW => {
            // Get the text to render
            let (full_text, is_hovered) = {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    (state.full_text.clone(), state.is_hovered)
                } else {
                    (String::new(), false)
                }
            };
            
            if markdown_view::has_markdown_webview(hwnd) {
                // WebView was pre-created, just show and update it
                markdown_view::update_markdown_content(hwnd, &full_text);
                markdown_view::show_markdown_webview(hwnd);
            } else {
                // Try to create WebView
                let result = markdown_view::create_markdown_webview(hwnd, &full_text, is_hovered);
                if !result {
                    // Failed to create - revert markdown mode
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.is_markdown_mode = false;
                    }
                }
            }
            
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
