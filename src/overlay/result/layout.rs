use windows::Win32::Foundation::RECT;
use super::state::ResizeEdge;

/// Calculate the next window position based on the previous window's rect.
/// Tries positions in order: Right -> Bottom -> Left -> Top -> Cascaded Offset
pub fn calculate_next_window_rect(prev: RECT, screen_w: i32, screen_h: i32) -> RECT {
    let gap = 15;
    let w = (prev.right - prev.left).abs();
    let h = (prev.bottom - prev.top).abs();

    // 1. Try RIGHT
    if prev.right + gap + w <= screen_w {
        return RECT {
            left: prev.right + gap,
            top: prev.top,
            right: prev.right + gap + w,
            bottom: prev.bottom
        };
    }
    
    // 2. Try BOTTOM
    if prev.bottom + gap + h <= screen_h {
        return RECT {
            left: prev.left,
            top: prev.bottom + gap,
            right: prev.right,
            bottom: prev.bottom + gap + h
        };
    }

    // 3. Try LEFT
    if prev.left - gap - w >= 0 {
        return RECT {
            left: prev.left - gap - w,
            top: prev.top,
            right: prev.left - gap,
            bottom: prev.bottom
        };
    }

    // 4. Try TOP
    if prev.top - gap - h >= 0 {
        return RECT {
            left: prev.left,
            top: prev.top - gap - h,
            right: prev.right,
            bottom: prev.top - gap
        };
    }

    // 5. Fallback: Cascade slightly offset
    RECT {
        left: prev.left + 40,
        top: prev.top + 40,
        right: prev.left + 40 + w,
        bottom: prev.top + 40 + h
    }
}

pub fn get_copy_btn_rect(window_w: i32, window_h: i32) -> RECT {
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

pub fn get_edit_btn_rect(window_w: i32, window_h: i32) -> RECT {
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

pub fn get_undo_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let edit_rect = get_edit_btn_rect(window_w, window_h);
    let gap = 8;
    let width = edit_rect.right - edit_rect.left;
    RECT {
        left: edit_rect.left - width - gap,
        top: edit_rect.top,
        right: edit_rect.left - gap,
        bottom: edit_rect.bottom
    }
}

pub fn get_resize_edge(width: i32, height: i32, x: i32, y: i32) -> ResizeEdge {
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
