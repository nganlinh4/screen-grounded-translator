use std::mem::size_of;
use std::sync::Arc;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::overlay::result::button_canvas;
use crate::overlay::result::layout::{
    get_copy_btn_rect, get_download_btn_rect, get_edit_btn_rect, get_markdown_btn_rect,
    get_redo_btn_rect, get_resize_edge, get_speaker_btn_rect, get_undo_btn_rect,
    should_show_buttons,
};
use crate::overlay::result::markdown_view;
use crate::overlay::result::refine_input;
use crate::overlay::result::state::{InteractionMode, ResizeEdge, WINDOW_STATES};

unsafe fn set_rounded_edit_region(h_edit: HWND, w: i32, h: i32) {
    let rgn = CreateRoundRectRgn(0, 0, w, h, 12, 12);
    let _ = SetWindowRgn(h_edit, Some(rgn), true);
}

pub unsafe fn handle_set_cursor(hwnd: HWND) -> LRESULT {
    let mut cursor_id = PCWSTR(std::ptr::null());
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = ScreenToClient(hwnd, &mut pt);

    let mut is_over_edit = false;
    let mut is_streaming_active = false;
    {
        let states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get(&(hwnd.0 as isize)) {
            is_streaming_active = state.is_streaming_active;
            if state.is_editing {
                let edit_left = 10;
                let edit_top = 10;
                let edit_right = rect.right - 10;
                let edit_bottom = 10 + 40;

                is_over_edit = pt.x >= edit_left
                    && pt.x <= edit_right
                    && pt.y >= edit_top
                    && pt.y <= edit_bottom;
            }
        }
    }

    if is_over_edit {
        SetCursor(Some(LoadCursorW(None, IDC_IBEAM).unwrap()));
        return LRESULT(1);
    }

    let edge = get_resize_edge(rect.right, rect.bottom, pt.x, pt.y);

    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => cursor_id = IDC_SIZENS,
        ResizeEdge::Left | ResizeEdge::Right => cursor_id = IDC_SIZEWE,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => cursor_id = IDC_SIZENWSE,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => cursor_id = IDC_SIZENESW,
        ResizeEdge::None => {
            // Only show hand cursor on buttons if overlay is large enough AND not streaming
            if !is_streaming_active && should_show_buttons(rect.right, rect.bottom) {
                let copy_rect = get_copy_btn_rect(rect.right, rect.bottom);
                let edit_rect = get_edit_btn_rect(rect.right, rect.bottom);
                let undo_rect = get_undo_btn_rect(rect.right, rect.bottom);

                let on_copy = pt.x >= copy_rect.left
                    && pt.x <= copy_rect.right
                    && pt.y >= copy_rect.top
                    && pt.y <= copy_rect.bottom;
                let on_edit = pt.x >= edit_rect.left
                    && pt.x <= edit_rect.right
                    && pt.y >= edit_rect.top
                    && pt.y <= edit_rect.bottom;

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
                    left: cx_back - 14,
                    top: cy_back - 14,
                    right: cx_back + 14,
                    bottom: cy_back + 14,
                };

                let on_back = is_browsing
                    && pt.x >= back_rect.left
                    && pt.x <= back_rect.right
                    && pt.y >= back_rect.top
                    && pt.y <= back_rect.bottom;

                let on_undo = has_history
                    && pt.x >= undo_rect.left
                    && pt.x <= undo_rect.right
                    && pt.y >= undo_rect.top
                    && pt.y <= undo_rect.bottom;

                let md_rect = get_markdown_btn_rect(rect.right, rect.bottom);
                let on_md = pt.x >= md_rect.left
                    && pt.x <= md_rect.right
                    && pt.y >= md_rect.top
                    && pt.y <= md_rect.bottom;

                let dl_rect = get_download_btn_rect(rect.right, rect.bottom);
                let on_dl = pt.x >= dl_rect.left
                    && pt.x <= dl_rect.right
                    && pt.y >= dl_rect.top
                    && pt.y <= dl_rect.bottom;

                let speaker_rect = get_speaker_btn_rect(rect.right, rect.bottom);
                let on_speaker = pt.x >= speaker_rect.left
                    && pt.x <= speaker_rect.right
                    && pt.y >= speaker_rect.top
                    && pt.y <= speaker_rect.bottom;

                if on_copy || on_edit || on_undo || on_md || on_back || on_dl || on_speaker {
                    cursor_id = IDC_HAND;
                }
            }
        }
    }

    if !cursor_id.0.is_null() {
        SetCursor(Some(LoadCursorW(None, cursor_id).unwrap()));
        LRESULT(1)
    } else {
        SetCursor(Some(HCURSOR(std::ptr::null_mut())));
        LRESULT(1)
    }
}

pub unsafe fn handle_lbutton_down(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let x = (lparam.0 & 0xFFFF) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);
    let width = rect.right;
    let height = rect.bottom;
    let edge = get_resize_edge(width, height, x, y);
    let mut window_rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut window_rect);
    let mut screen_pt = POINT::default();
    let _ = GetCursorPos(&mut screen_pt);

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
    let is_markdown = if let Some(state) = states.get(&(hwnd.0 as isize)) {
        state.is_markdown_mode
    } else {
        false
    };
    SetCapture(hwnd);
    if is_markdown {
        button_canvas::set_drag_mode(true); // Enable unclipped drag mode for smooth UI
    }
    LRESULT(0)
}

pub unsafe fn handle_rbutton_down(hwnd: HWND, _lparam: LPARAM) -> LRESULT {
    let mut screen_pt = POINT::default();
    let _ = GetCursorPos(&mut screen_pt);

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
                        let h = HWND(h_val as *mut std::ffi::c_void);
                        let mut r = RECT::default();
                        let _ = GetWindowRect(h, &mut r);
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
                let _ = GetWindowRect(current, &mut r);
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
    button_canvas::set_drag_mode(true); // Enable unclipped drag mode for smooth UI
    LRESULT(0)
}

pub unsafe fn handle_mouse_move(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);
    let hover_edge = get_resize_edge(rect.right, rect.bottom, x as i32, y as i32);

    // Defer group moves to avoid deadlocks (holding lock while calling SetWindowPos on other windows)
    let mut group_moves = Vec::new();

    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
            state.current_resize_edge = hover_edge;
            let dx = x - state.physics.x;
            let drag_impulse = if matches!(
                &state.interaction_mode,
                InteractionMode::DraggingWindow | InteractionMode::DraggingGroup(_)
            ) {
                0.0
            } else {
                (dx * 1.5).clamp(-20.0, 20.0)
            };
            state.physics.tilt_velocity -= drag_impulse * 0.2;
            state.physics.current_tilt = state.physics.current_tilt.clamp(-22.5, 22.5);
            state.physics.x = x;
            state.physics.y = y;

            // Only process button hover states if overlay is large enough AND not streaming
            if !state.is_streaming_active && should_show_buttons(rect.right, rect.bottom) {
                let copy_rect = get_copy_btn_rect(rect.right, rect.bottom);
                let edit_rect = get_edit_btn_rect(rect.right, rect.bottom);
                let undo_rect = get_undo_btn_rect(rect.right, rect.bottom);
                let padding = 4;
                state.on_copy_btn = x as i32 >= copy_rect.left - padding
                    && x as i32 <= copy_rect.right + padding
                    && y as i32 >= copy_rect.top - padding
                    && y as i32 <= copy_rect.bottom + padding;
                state.on_edit_btn = x as i32 >= edit_rect.left - padding
                    && x as i32 <= edit_rect.right + padding
                    && y as i32 >= edit_rect.top - padding
                    && y as i32 <= edit_rect.bottom + padding;
                if !state.text_history.is_empty() && !state.is_browsing {
                    state.on_undo_btn = x as i32 >= undo_rect.left - padding
                        && x as i32 <= undo_rect.right + padding
                        && y as i32 >= undo_rect.top - padding
                        && y as i32 <= undo_rect.bottom + padding;
                } else {
                    state.on_undo_btn = false;
                }

                // Redo button hover state
                let redo_rect = get_redo_btn_rect(rect.right, rect.bottom);
                if !state.redo_history.is_empty() && !state.is_browsing {
                    state.on_redo_btn = x as i32 >= redo_rect.left - padding
                        && x as i32 <= redo_rect.right + padding
                        && y as i32 >= redo_rect.top - padding
                        && y as i32 <= redo_rect.bottom + padding;
                } else {
                    state.on_redo_btn = false;
                }

                // Calc Back and Forward Button state (only when browsing)
                if state.is_browsing {
                    let btn_size = 28;
                    let margin = 12;
                    let threshold_h = btn_size + (margin * 2);
                    let cy = if rect.bottom < threshold_h {
                        (rect.bottom as f32) / 2.0
                    } else {
                        (rect.bottom - margin - btn_size / 2) as f32
                    };

                    // Back button (left side)
                    let cx_back = (margin + btn_size / 2) as i32;
                    let cy_back = cy as i32;
                    let l = cx_back - 14 - padding;
                    let r = cx_back + 14 + padding;
                    let t = cy_back - 14 - padding;
                    let b = cy_back + 14 + padding;
                    state.on_back_btn =
                        x as i32 >= l && x as i32 <= r && y as i32 >= t && y as i32 <= b;

                    // Forward button (right side)
                    if state.navigation_depth < state.max_navigation_depth {
                        let cx_forward = (rect.right - margin - btn_size / 2) as i32;
                        let lf = cx_forward - 14 - padding;
                        let rf = cx_forward + 14 + padding;
                        state.on_forward_btn =
                            x as i32 >= lf && x as i32 <= rf && y as i32 >= t && y as i32 <= b;
                    } else {
                        state.on_forward_btn = false;
                    }

                    // Disable all result UI button hovers when browsing
                    state.on_copy_btn = false;
                    state.on_edit_btn = false;
                    state.on_markdown_btn = false;
                    state.on_download_btn = false;
                } else {
                    state.on_back_btn = false;
                    state.on_forward_btn = false;

                    let md_rect = get_markdown_btn_rect(rect.right, rect.bottom);
                    let padding = 4;
                    state.on_markdown_btn = x as i32 >= md_rect.left - padding
                        && x as i32 <= md_rect.right + padding
                        && y as i32 >= md_rect.top - padding
                        && y as i32 <= md_rect.bottom + padding;

                    let dl_rect = get_download_btn_rect(rect.right, rect.bottom);
                    state.on_download_btn = x as i32 >= dl_rect.left - padding
                        && x as i32 <= dl_rect.right + padding
                        && y as i32 >= dl_rect.top - padding
                        && y as i32 <= dl_rect.bottom + padding;

                    let speaker_rect = get_speaker_btn_rect(rect.right, rect.bottom);
                    state.on_speaker_btn = x as i32 >= speaker_rect.left - padding
                        && x as i32 <= speaker_rect.right + padding
                        && y as i32 >= speaker_rect.top - padding
                        && y as i32 <= speaker_rect.bottom + padding;
                }
            } else {
                // Overlay too small - clear all button hover states
                state.on_copy_btn = false;
                state.on_edit_btn = false;
                state.on_undo_btn = false;
                state.on_redo_btn = false;
                state.on_markdown_btn = false;
                state.on_download_btn = false;
                state.on_back_btn = false;
                state.on_forward_btn = false;
                state.on_speaker_btn = false;
            }

            // In markdown mode, let the Timer handle is_hovered state to ensure it syncs with WebView resize
            let handle_hover_in_mousemove = !state.is_markdown_mode;

            if handle_hover_in_mousemove && !state.is_hovered {
                state.is_hovered = true;
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                // Note: WebView resize is now handled by Timer 2 to avoid race conditions
            }

            match &state.interaction_mode {
                InteractionMode::DraggingWindow => {
                    let mut curr_pt = POINT::default();
                    let _ = GetCursorPos(&mut curr_pt);
                    let dx = curr_pt.x - state.drag_start_mouse.x;
                    let dy = curr_pt.y - state.drag_start_mouse.y;
                    if dx.abs() > 3 || dy.abs() > 3 {
                        state.has_moved_significantly = true;
                    }
                    let new_x = state.drag_start_window_rect.left + dx;
                    let new_y = state.drag_start_window_rect.top + dy;
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND::default()),
                        new_x,
                        new_y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    button_canvas::update_window_position(hwnd);
                }
                InteractionMode::DraggingGroup(snapshot) => {
                    let mut curr_pt = POINT::default();
                    let _ = GetCursorPos(&mut curr_pt);
                    let dx = curr_pt.x - state.drag_start_mouse.x;
                    let dy = curr_pt.y - state.drag_start_mouse.y;
                    if dx.abs() > 3 || dy.abs() > 3 {
                        state.has_moved_significantly = true;
                    }

                    for (h, start_rect) in snapshot {
                        let new_x = start_rect.left + dx;
                        let new_y = start_rect.top + dy;
                        group_moves.push((*h, new_x, new_y));
                    }
                }
                InteractionMode::Resizing(edge) => {
                    state.has_moved_significantly = true;
                    let mut curr_pt = POINT::default();
                    let _ = GetCursorPos(&mut curr_pt);
                    let dx = curr_pt.x - state.drag_start_mouse.x;
                    let dy = curr_pt.y - state.drag_start_mouse.y;
                    let mut new_rect = state.drag_start_window_rect;
                    let min_w = super::MIN_WINDOW_WIDTH;
                    let min_h = super::MIN_WINDOW_HEIGHT;
                    match edge {
                        ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight => {
                            new_rect.right = (state.drag_start_window_rect.right + dx)
                                .max(state.drag_start_window_rect.left + min_w);
                        }
                        ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft => {
                            new_rect.left = (state.drag_start_window_rect.left + dx)
                                .min(state.drag_start_window_rect.right - min_w);
                        }
                        _ => {}
                    }
                    match edge {
                        ResizeEdge::Bottom | ResizeEdge::BottomRight | ResizeEdge::BottomLeft => {
                            new_rect.bottom = (state.drag_start_window_rect.bottom + dy)
                                .max(state.drag_start_window_rect.top + min_h);
                        }
                        ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => {
                            new_rect.top = (state.drag_start_window_rect.top + dy)
                                .min(state.drag_start_window_rect.bottom - min_h);
                        }
                        _ => {}
                    }
                    let w = new_rect.right - new_rect.left;
                    let h = new_rect.bottom - new_rect.top;
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND::default()),
                        new_rect.left,
                        new_rect.top,
                        w,
                        h,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    if state.is_editing {
                        let edit_w = w - 20;
                        let edit_h = 40;
                        let _ = SetWindowPos(
                            state.edit_hwnd,
                            Some(HWND_TOP),
                            10,
                            10,
                            edit_w,
                            edit_h,
                            SWP_NOACTIVATE,
                        );
                        set_rounded_edit_region(state.edit_hwnd, edit_w, edit_h);
                    }
                    // Resize markdown webview if in markdown mode
                    if state.is_markdown_mode {
                        markdown_view::resize_markdown_webview(hwnd, state.is_hovered);
                    }
                    // Resize refine input if active
                    if refine_input::is_refine_input_active(hwnd) {
                        refine_input::resize_refine_input(hwnd);
                    }
                    button_canvas::update_window_position(hwnd);
                }
                _ => {}
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    } // Lock released

    // Execute deferred group moves
    for (h, x, y) in group_moves {
        let _ = SetWindowPos(
            h,
            Some(HWND::default()),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        button_canvas::update_window_position(h);
    }

    LRESULT(0)
}

pub unsafe fn handle_mouse_leave(hwnd: HWND) -> LRESULT {
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
        state.on_back_btn = false;
        state.on_forward_btn = false;
        state.on_speaker_btn = false;
        state.current_resize_edge = ResizeEdge::None;

        // For plain text mode, also clear hover state here
        // (markdown mode uses Timer 2 for this since WebView steals focus)
        if !state.is_markdown_mode {
            state.is_hovered = false;
        }

        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    LRESULT(0)
}
