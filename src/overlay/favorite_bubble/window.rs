use super::panel::{
    close_panel, destroy_panel, ensure_panel_created, move_panel_to_bubble, show_panel,
};
use super::render::update_bubble_visual;
use super::state::*;
use crate::APP;
use std::sync::atomic::Ordering;
use windows::core::w;
use windows::Win32::Foundation::*;

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

// Show the favorite bubble overlay
pub fn show_favorite_bubble() {
    // Prevent duplicates
    if BUBBLE_ACTIVE.swap(true, Ordering::SeqCst) {
        return; // Already active
    }

    std::thread::spawn(|| {
        create_bubble_window();
    });
}

// Hide the favorite bubble overlay
pub fn hide_favorite_bubble() {
    if !BUBBLE_ACTIVE.load(Ordering::SeqCst) {
        return;
    }

    let hwnd_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn trigger_blink_animation() {
    let hwnd_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        BLINK_STATE.store(1, Ordering::SeqCst); // Start Blink Phase 1
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            // Force timer start
            let _ = SetTimer(Some(hwnd), OPACITY_TIMER_ID, 16, None);
        }
    }
}

fn create_bubble_window() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTFavoriteBubble");

        REGISTER_BUBBLE_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(bubble_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        // Get saved position or use default
        let (initial_x, initial_y) = if let Ok(app) = APP.lock() {
            app.config.favorite_bubble_position.unwrap_or_else(|| {
                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                let screen_h = GetSystemMetrics(SM_CYSCREEN);
                (screen_w - BUBBLE_SIZE - 30, screen_h - BUBBLE_SIZE - 150)
            })
        } else {
            (100, 100)
        };

        // Create layered window for transparency
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("FavBubble"),
            WS_POPUP,
            initial_x,
            initial_y,
            BUBBLE_SIZE,
            BUBBLE_SIZE,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if hwnd.is_invalid() {
            BUBBLE_ACTIVE.store(false, Ordering::SeqCst);
            return;
        }

        BUBBLE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // Paint the bubble
        update_bubble_visual(hwnd);

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        // Warmup: Create panel hidden immediately
        ensure_panel_created(hwnd);

        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup
        destroy_panel();
        BUBBLE_ACTIVE.store(false, Ordering::SeqCst);
        BUBBLE_HWND.store(0, Ordering::SeqCst);
    }
}

unsafe extern "system" fn bubble_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    const WM_MOUSELEAVE: u32 = 0x02A3;

    match msg {
        WM_LBUTTONDOWN => {
            // Stop any ongoing physics
            let _ = KillTimer(Some(hwnd), PHYSICS_TIMER_ID);
            PHYSICS_STATE.with(|p| *p.borrow_mut() = (0.0, 0.0));

            IS_DRAGGING.store(true, Ordering::SeqCst);
            IS_DRAGGING_MOVED.store(false, Ordering::SeqCst);

            // Store initial click position for threshold check
            let x = (lparam.0 as i32) & 0xFFFF;
            let y = ((lparam.0 as i32) >> 16) & 0xFFFF;
            DRAG_START_X.store(x as isize, Ordering::SeqCst);
            DRAG_START_Y.store(y as isize, Ordering::SeqCst);

            let _ = SetCapture(hwnd);
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            let was_dragging_moved = IS_DRAGGING_MOVED.load(Ordering::SeqCst);
            IS_DRAGGING.store(false, Ordering::SeqCst);
            let _ = ReleaseCapture();

            // Only toggle if we didn't drag/move the bubble
            if !was_dragging_moved {
                if IS_EXPANDED.load(Ordering::SeqCst) {
                    close_panel();
                } else {
                    show_panel(hwnd);
                }
            } else {
                // Start physics inertia if we were moving
                let _ = SetTimer(Some(hwnd), PHYSICS_TIMER_ID, 16, None);
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            if IS_DRAGGING.load(Ordering::SeqCst) && (wparam.0 & 0x0001) != 0 {
                // Left button held - check for drag
                let x = (lparam.0 as i32) & 0xFFFF;
                let y = ((lparam.0 as i32) >> 16) & 0xFFFF;

                // Convert to signed 16-bit to handle negative coordinates properly
                let x = x as i16 as i32;
                let y = y as i16 as i32;

                // Check if we've exceeded the drag threshold
                if !IS_DRAGGING_MOVED.load(Ordering::SeqCst) {
                    let start_x = DRAG_START_X.load(Ordering::SeqCst) as i32;
                    let start_y = DRAG_START_Y.load(Ordering::SeqCst) as i32;
                    let dx = (x - start_x).abs();
                    let dy = (y - start_y).abs();

                    if dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD {
                        IS_DRAGGING_MOVED.store(true, Ordering::SeqCst);
                    }
                }

                // Only actually move the window if threshold was exceeded
                if IS_DRAGGING_MOVED.load(Ordering::SeqCst) {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);

                    // Use Work Area (exclude taskbar) for boundaries
                    let mut work_area = RECT::default();
                    unsafe {
                        let _ = SystemParametersInfoW(
                            SPI_GETWORKAREA,
                            0,
                            Some(&mut work_area as *mut _ as *mut std::ffi::c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                    }

                    let new_x = (rect.left + x - BUBBLE_SIZE / 2)
                        .clamp(work_area.left, work_area.right - BUBBLE_SIZE);
                    let new_y = (rect.top + y - BUBBLE_SIZE / 2)
                        .clamp(work_area.top, work_area.bottom - BUBBLE_SIZE);

                    // Track velocity (instantaneous delta) with smoothing and boost
                    let raw_vx = (new_x - rect.left) as f32;
                    let raw_vy = (new_y - rect.top) as f32;

                    // Boost factor allows "throwing" to feel more powerful
                    // Smoothing helps filter out jitter from high polling rates
                    const THROW_BOOST: f32 = 2.5;
                    const SMOOTHING: f32 = 0.6; // Weight for new value

                    PHYSICS_STATE.with(|p| {
                        let (old_vx, old_vy) = *p.borrow();
                        let target_vx = raw_vx * THROW_BOOST;
                        let target_vy = raw_vy * THROW_BOOST;

                        let final_vx = old_vx * (1.0 - SMOOTHING) + target_vx * SMOOTHING;
                        let final_vy = old_vy * (1.0 - SMOOTHING) + target_vy * SMOOTHING;

                        *p.borrow_mut() = (final_vx, final_vy);
                    });

                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        new_x,
                        new_y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );

                    // Move panel if open
                    if IS_EXPANDED.load(Ordering::SeqCst) {
                        move_panel_to_bubble(new_x, new_y);
                    }
                }
            }

            if !IS_HOVERED.load(Ordering::SeqCst) {
                IS_HOVERED.store(true, Ordering::SeqCst);

                // Start animation timer
                let _ = SetTimer(Some(hwnd), OPACITY_TIMER_ID, 16, None); // ~60 FPS

                // Track mouse leave
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            IS_HOVERED.store(false, Ordering::SeqCst);
            // Start animation timer to fade out (unless expanded)
            let _ = SetTimer(Some(hwnd), OPACITY_TIMER_ID, 16, None);
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == OPACITY_TIMER_ID {
                let is_hovered = IS_HOVERED.load(Ordering::SeqCst);
                let is_expanded = IS_EXPANDED.load(Ordering::SeqCst);
                let blink_state = BLINK_STATE.load(Ordering::SeqCst);

                let mut target = if is_hovered || is_expanded {
                    OPACITY_ACTIVE
                } else {
                    OPACITY_INACTIVE
                };

                // Blink override
                if blink_state > 0 {
                    // Odd state = Active (255), Even state = Low (50) for visibility
                    if blink_state % 2 != 0 {
                        target = OPACITY_ACTIVE;
                    } else {
                        target = 50; // Drop lower than inactive to be distinct
                    }
                }

                let current = CURRENT_OPACITY.load(Ordering::SeqCst);

                if current != target {
                    // Faster step for blinking
                    let step = if blink_state > 0 { 45 } else { OPACITY_STEP };

                    let new_opacity = if current < target {
                        (current as u16 + step as u16).min(target as u16) as u8
                    } else {
                        (current as i16 - step as i16).max(target as i16) as u8
                    };
                    CURRENT_OPACITY.store(new_opacity, Ordering::SeqCst);
                    update_bubble_visual(hwnd);
                } else {
                    if blink_state > 0 {
                        // Transition to next blink state
                        if blink_state >= 4 {
                            BLINK_STATE.store(0, Ordering::SeqCst);
                        } else {
                            BLINK_STATE.fetch_add(1, Ordering::SeqCst);
                        }
                        // Keep timer running for next phase (no KillTimer)
                    } else {
                        let _ = KillTimer(Some(hwnd), OPACITY_TIMER_ID);
                    }
                }
            } else if wparam.0 == PHYSICS_TIMER_ID {
                PHYSICS_STATE.with(|p| {
                    let (mut vx, mut vy) = *p.borrow();

                    // Lower friction for longer travel (was 0.92)
                    vx *= 0.95;
                    vy *= 0.95;

                    // Stop if slow
                    if vx.abs() < 0.2 && vy.abs() < 0.2 {
                        // Lower threshold for smoother stop
                        let _ = KillTimer(Some(hwnd), PHYSICS_TIMER_ID);
                        *p.borrow_mut() = (0.0, 0.0);
                        return;
                    }

                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);

                    let mut next_x = rect.left as f32 + vx;
                    let mut next_y = rect.top as f32 + vy;

                    // Use Work Area (exclude taskbar) for physics collision logic
                    let mut work_area = RECT::default();
                    unsafe {
                        let _ = SystemParametersInfoW(
                            SPI_GETWORKAREA,
                            0,
                            Some(&mut work_area as *mut _ as *mut std::ffi::c_void),
                            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                        );
                    }

                    let min_x = work_area.left as f32;
                    let max_x = (work_area.right - BUBBLE_SIZE) as f32;
                    let min_y = work_area.top as f32;
                    let max_y = (work_area.bottom - BUBBLE_SIZE) as f32;

                    let bounce_factor = 0.75; // Rubbery bounce

                    // Bounce off edges
                    if next_x < min_x {
                        next_x = min_x;
                        vx = -vx * bounce_factor;
                    } else if next_x > max_x {
                        next_x = max_x;
                        vx = -vx * bounce_factor;
                    }

                    if next_y < min_y {
                        next_y = min_y;
                        vy = -vy * bounce_factor;
                    } else if next_y > max_y {
                        next_y = max_y;
                        vy = -vy * bounce_factor;
                    }

                    *p.borrow_mut() = (vx, vy);

                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        next_x as i32,
                        next_y as i32,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );

                    if IS_EXPANDED.load(Ordering::SeqCst) {
                        move_panel_to_bubble(next_x as i32, next_y as i32);
                    }
                });
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            close_panel();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
