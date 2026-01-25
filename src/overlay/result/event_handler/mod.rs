use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub mod click_actions;
pub mod misc;
pub mod mouse_input;
pub mod timer_tasks;

/// Minimum window size to prevent rendering issues when resizing too small.
/// Below these dimensions, GDI operations can fail or cause system errors.
pub const MIN_WINDOW_WIDTH: i32 = 40;
pub const MIN_WINDOW_HEIGHT: i32 = 30;

pub unsafe extern "system" fn result_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => misc::handle_erase_bkgnd(hwnd, wparam),

        // WM_CTLCOLOREDIT removed (native edit control deprecated)
        WM_SETCURSOR => mouse_input::handle_set_cursor(hwnd),

        WM_LBUTTONDOWN => mouse_input::handle_lbutton_down(hwnd, lparam),

        WM_RBUTTONDOWN => mouse_input::handle_rbutton_down(hwnd, lparam),

        WM_MOUSEMOVE => mouse_input::handle_mouse_move(hwnd, lparam),
        WM_MBUTTONDOWN => mouse_input::handle_mbutton_down(hwnd, lparam),

        0x02A3 => mouse_input::handle_mouse_leave(hwnd), // WM_MOUSELEAVE

        WM_LBUTTONUP => click_actions::handle_lbutton_up(hwnd),

        WM_RBUTTONUP => click_actions::handle_rbutton_up(hwnd),

        WM_MBUTTONUP => click_actions::handle_mbutton_up(hwnd),

        WM_TIMER => timer_tasks::handle_timer(hwnd, wparam),

        WM_DESTROY => misc::handle_destroy(hwnd),

        WM_PAINT => misc::handle_paint(hwnd),

        WM_KEYDOWN => misc::handle_keydown(),

        // Enforce minimum window size to prevent rendering issues
        WM_GETMINMAXINFO => {
            let mmi = lparam.0 as *mut MINMAXINFO;
            if !mmi.is_null() {
                (*mmi).ptMinTrackSize.x = MIN_WINDOW_WIDTH;
                (*mmi).ptMinTrackSize.y = MIN_WINDOW_HEIGHT;
            }
            LRESULT(0)
        }

        // Deferred WebView2 creation - handles the WM_CREATE_WEBVIEW we posted
        msg if msg == misc::WM_CREATE_WEBVIEW => misc::handle_create_webview(hwnd),
        msg if msg == misc::WM_SHOW_MARKDOWN => misc::handle_show_markdown(hwnd),
        msg if msg == misc::WM_HIDE_MARKDOWN => misc::handle_hide_markdown(hwnd),
        msg if msg == misc::WM_RESIZE_MARKDOWN => misc::handle_resize_markdown(hwnd),

        msg if msg == misc::WM_UNDO_CLICK => {
            crate::overlay::result::trigger_undo(hwnd);
            LRESULT(0)
        }
        msg if msg == misc::WM_REDO_CLICK => {
            crate::overlay::result::trigger_redo(hwnd);
            LRESULT(0)
        }
        msg if msg == misc::WM_COPY_CLICK => {
            crate::overlay::result::trigger_copy(hwnd);
            LRESULT(0)
        }
        msg if msg == misc::WM_EDIT_CLICK => {
            crate::overlay::result::trigger_edit(hwnd);
            LRESULT(0)
        }
        msg if msg == misc::WM_BACK_CLICK => misc::handle_back_click(hwnd),
        msg if msg == misc::WM_FORWARD_CLICK => misc::handle_forward_click(hwnd),
        msg if msg == misc::WM_SPEAKER_CLICK => {
            crate::overlay::result::trigger_speaker(hwnd);
            LRESULT(0)
        }
        msg if msg == misc::WM_DOWNLOAD_CLICK => misc::handle_download_click(hwnd),

        WM_WINDOWPOSCHANGED => {
            // Update button canvas position when window moves/resizes
            crate::overlay::result::button_canvas::register_markdown_window(hwnd);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_ENTERSIZEMOVE => {
            // Set interaction mode to Resizing to triggering "Hide All Buttons" logic
            crate::overlay::result::state::set_window_interaction_mode(
                hwnd,
                crate::overlay::result::state::InteractionMode::Resizing(
                    crate::overlay::result::state::ResizeEdge::None,
                ),
            );
            crate::overlay::result::button_canvas::update_canvas();
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_EXITSIZEMOVE => {
            // Reset interaction mode to show buttons again
            crate::overlay::result::state::set_window_interaction_mode(
                hwnd,
                crate::overlay::result::state::InteractionMode::None,
            );

            // Re-trigger markdown view fitting after native resize ends
            let is_markdown = {
                let states = crate::overlay::result::state::WINDOW_STATES.lock().unwrap();
                states
                    .get(&(hwnd.0 as isize))
                    .map(|s| s.is_markdown_mode)
                    .unwrap_or(false)
            };
            if is_markdown {
                crate::overlay::result::markdown_view::fit_font_to_window(hwnd);
            }

            crate::overlay::result::button_canvas::update_canvas();
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
