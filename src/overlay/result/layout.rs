use windows::Win32::Foundation::{RECT, HWND};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindow};
use super::state::{ResizeEdge, WINDOW_STATES};

/// Check if two RECTs overlap (with a gap margin)
fn rects_overlap(a: &RECT, b: &RECT, gap: i32) -> bool {
    // Expand both rects by gap/2 to account for minimum gap between windows
    let half_gap = gap / 2;
    !(a.right + half_gap <= b.left - half_gap 
        || b.right + half_gap <= a.left - half_gap 
        || a.bottom + half_gap <= b.top - half_gap 
        || b.bottom + half_gap <= a.top - half_gap)
}

/// Get RECTs of all currently visible result overlay windows
/// This provides intelligent detection of existing windows for collision avoidance
fn get_all_active_window_rects() -> Vec<RECT> {
    let mut rects = Vec::new();
    
    // Lock WINDOW_STATES to get all tracked overlay windows
    if let Ok(states) = WINDOW_STATES.lock() {
        for (&hwnd_key, _state) in states.iter() {
            let hwnd = HWND(hwnd_key as isize);
            unsafe {
                // Verify window is still valid
                if IsWindow(hwnd).as_bool() {
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).as_bool() {
                        rects.push(rect);
                    }
                }
            }
        }
    }
    
    rects
}

/// Check if a proposed RECT overlaps with any existing window
fn would_overlap_existing(proposed: &RECT, existing: &[RECT], gap: i32) -> bool {
    existing.iter().any(|r| rects_overlap(proposed, r, gap))
}

/// Calculate the next window position with intelligent collision detection.
/// 
/// This improved algorithm:
/// 1. Collects all active overlay windows from WINDOW_STATES
/// 2. Tries positions in order: Right -> Bottom -> Left -> Top
/// 3. Checks each candidate against ALL existing windows (not just the previous one)
/// 4. Falls back to cascade positioning if all directions are blocked
/// 
/// Similar to the intelligent layout in node_graph.rs blocks_to_snarl()
pub fn calculate_next_window_rect(prev: RECT, screen_w: i32, screen_h: i32) -> RECT {
    let gap = 15;
    let w = (prev.right - prev.left).abs();
    let h = (prev.bottom - prev.top).abs();
    
    // Get all active window RECTs for collision detection
    let existing_windows = get_all_active_window_rects();

    // 1. Try RIGHT
    let right_candidate = RECT {
        left: prev.right + gap,
        top: prev.top,
        right: prev.right + gap + w,
        bottom: prev.bottom
    };
    if right_candidate.right <= screen_w 
        && !would_overlap_existing(&right_candidate, &existing_windows, gap) {
        return right_candidate;
    }
    
    // 2. Try BOTTOM
    let bottom_candidate = RECT {
        left: prev.left,
        top: prev.bottom + gap,
        right: prev.right,
        bottom: prev.bottom + gap + h
    };
    if bottom_candidate.bottom <= screen_h 
        && !would_overlap_existing(&bottom_candidate, &existing_windows, gap) {
        return bottom_candidate;
    }

    // 3. Try LEFT
    let left_candidate = RECT {
        left: prev.left - gap - w,
        top: prev.top,
        right: prev.left - gap,
        bottom: prev.bottom
    };
    if left_candidate.left >= 0 
        && !would_overlap_existing(&left_candidate, &existing_windows, gap) {
        return left_candidate;
    }

    // 4. Try TOP
    let top_candidate = RECT {
        left: prev.left,
        top: prev.top - gap - h,
        right: prev.right,
        bottom: prev.top - gap
    };
    if top_candidate.top >= 0 
        && !would_overlap_existing(&top_candidate, &existing_windows, gap) {
        return top_candidate;
    }
    
    // 5. Try diagonals if cardinal directions are blocked
    let diagonals = [
        // Bottom-Right
        RECT { left: prev.right + gap, top: prev.bottom + gap, right: prev.right + gap + w, bottom: prev.bottom + gap + h },
        // Bottom-Left
        RECT { left: prev.left - gap - w, top: prev.bottom + gap, right: prev.left - gap, bottom: prev.bottom + gap + h },
        // Top-Right
        RECT { left: prev.right + gap, top: prev.top - gap - h, right: prev.right + gap + w, bottom: prev.top - gap },
        // Top-Left
        RECT { left: prev.left - gap - w, top: prev.top - gap - h, right: prev.left - gap, bottom: prev.top - gap },
    ];
    
    for diag in diagonals {
        if diag.left >= 0 && diag.right <= screen_w && diag.top >= 0 && diag.bottom <= screen_h
            && !would_overlap_existing(&diag, &existing_windows, gap) {
            return diag;
        }
    }

    // 6. Cascade fallback: find a non-overlapping cascade position
    // Start with standard offset and increment until we find free space
    for cascade_mult in 1..10 {
        let offset = 40 * cascade_mult;
        let cascade = RECT {
            left: prev.left + offset,
            top: prev.top + offset,
            right: prev.left + offset + w,
            bottom: prev.top + offset + h
        };
        
        // Clamp to screen bounds
        if cascade.right <= screen_w && cascade.bottom <= screen_h 
            && !would_overlap_existing(&cascade, &existing_windows, gap) {
            return cascade;
        }
    }
    
    // 7. Ultimate fallback: just use the simple cascade (may overlap)
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

// Markdown button is between Edit and Copy buttons
pub fn get_markdown_btn_rect(window_w: i32, window_h: i32) -> RECT {
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

// Download HTML button is between Markdown and Undo buttons
pub fn get_download_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let md_rect = get_markdown_btn_rect(window_w, window_h);
    let gap = 8;
    let width = md_rect.right - md_rect.left;
    RECT {
        left: md_rect.left - width - gap,
        top: md_rect.top,
        right: md_rect.left - gap,
        bottom: md_rect.bottom
    }
}

pub fn get_undo_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let dl_rect = get_download_btn_rect(window_w, window_h);
    let gap = 8;
    let width = dl_rect.right - dl_rect.left;
    RECT {
        left: dl_rect.left - width - gap,
        top: dl_rect.top,
        right: dl_rect.left - gap,
        bottom: dl_rect.bottom
    }
}

pub fn get_redo_btn_rect(window_w: i32, window_h: i32) -> RECT {
    let undo_rect = get_undo_btn_rect(window_w, window_h);
    let gap = 8;
    let width = undo_rect.right - undo_rect.left;
    RECT {
        left: undo_rect.left - width - gap,
        top: undo_rect.top,
        right: undo_rect.left - gap,
        bottom: undo_rect.bottom
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
