use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub mod click_actions;
pub mod misc;
pub mod mouse_input;
pub mod timer_tasks;

/// Minimum window size to prevent rendering issues when resizing too small.
/// Below these dimensions, GDI operations can fail or cause system errors.
pub const MIN_WINDOW_WIDTH: i32 = 40;
pub const MIN_WINDOW_HEIGHT: i32 = 40;

pub unsafe extern "system" fn result_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => misc::handle_erase_bkgnd(hwnd, wparam),

        WM_CTLCOLOREDIT => misc::handle_ctl_color_edit(wparam),

        WM_SETCURSOR => mouse_input::handle_set_cursor(hwnd),

        WM_LBUTTONDOWN => mouse_input::handle_lbutton_down(hwnd, lparam),

        WM_RBUTTONDOWN => mouse_input::handle_rbutton_down(hwnd, lparam),

        WM_MOUSEMOVE => mouse_input::handle_mouse_move(hwnd, lparam),

        0x02A3 => mouse_input::handle_mouse_leave(hwnd), // WM_MOUSELEAVE

        WM_LBUTTONUP => click_actions::handle_lbutton_up(hwnd),

        WM_RBUTTONUP => click_actions::handle_rbutton_up(hwnd),

        WM_MBUTTONUP => click_actions::handle_mbutton_up(),

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

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
