pub mod button_canvas;
mod event_handler;
pub mod layout;
mod logic;
pub mod markdown_view;
pub mod paint;
pub mod refine_input;
pub mod state;
mod window;

pub use state::{close_windows_with_token, link_windows, RefineContext, WindowType, WINDOW_STATES};
pub use window::{create_result_window, get_chain_color, update_window_text};

// Trigger functions for button canvas IPC
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

/// Trigger copy action on a result window
pub fn trigger_copy(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    // Get text and copy to clipboard
    let text = {
        let states = WINDOW_STATES.lock().unwrap();
        states
            .get(&hwnd_key)
            .map(|s| s.full_text.clone())
            .unwrap_or_default()
    };

    if !text.is_empty() {
        crate::overlay::utils::copy_to_clipboard(&text, hwnd);

        // Set copy success flag
        {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&hwnd_key) {
                state.copy_success = true;
            }
        }

        // Update canvas to show success state
        button_canvas::update_window_position(hwnd);

        // Reset success flag after delay
        let hwnd_val = hwnd.0 as usize;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd_val as isize)) {
                    state.copy_success = false;
                }
            }
            // Update canvas after dropping lock
            button_canvas::update_window_position(HWND(hwnd_val as *mut std::ffi::c_void));
        });
    }
}

/// Trigger undo action on a result window
pub fn trigger_undo(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    let (prev_text, is_markdown) = {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            if let Some(last) = state.text_history.pop() {
                let current = state.full_text.clone();
                state.redo_history.push(current);
                state.full_text = last.clone();
                (Some(last), state.is_markdown_mode)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        }
    };

    if let Some(txt) = prev_text {
        // Update window text
        let wide_text = crate::overlay::utils::to_wstring(&txt);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                hwnd,
                windows::core::PCWSTR(wide_text.as_ptr()),
            );
        }

        if is_markdown {
            unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    event_handler::misc::WM_CREATE_WEBVIEW,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }

        // Update canvas
        button_canvas::update_window_position(hwnd);
    }
}

/// Trigger redo action on a result window
pub fn trigger_redo(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    let (next_text, is_markdown) = {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            if let Some(redo) = state.redo_history.pop() {
                let current = state.full_text.clone();
                state.text_history.push(current);
                state.full_text = redo.clone();
                (Some(redo), state.is_markdown_mode)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        }
    };

    if let Some(txt) = next_text {
        let wide_text = crate::overlay::utils::to_wstring(&txt);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                hwnd,
                windows::core::PCWSTR(wide_text.as_ptr()),
            );
        }

        if is_markdown {
            unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    event_handler::misc::WM_CREATE_WEBVIEW,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }

        button_canvas::update_window_position(hwnd);
    }
}

/// Trigger edit/refine action
pub fn trigger_edit(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    let is_active = refine_input::is_refine_input_active(hwnd);

    if is_active {
        refine_input::hide_refine_input(hwnd);
        {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&hwnd_key) {
                state.is_editing = false;
            }
        }
    } else {
        let lang = crate::APP.lock().unwrap().config.ui_language.clone();
        let locale = crate::gui::locale::LocaleText::get(&lang);

        if refine_input::show_refine_input(hwnd, locale.text_input_placeholder) {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&hwnd_key) {
                state.is_editing = true;
            }
        }
    }

    // Resize markdown view if needed
    unsafe {
        let _ = PostMessageW(
            Some(hwnd),
            event_handler::misc::WM_RESIZE_MARKDOWN,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

/// Trigger markdown toggle (switch back to plain text)
pub fn trigger_markdown_toggle(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    // Check if we can toggle
    let can_toggle = {
        let states = WINDOW_STATES.lock().unwrap();
        states
            .get(&hwnd_key)
            .map(|s| !s.is_refining && !s.is_streaming_active)
            .unwrap_or(false)
    };

    if !can_toggle {
        return;
    }

    // Toggle the mode in state
    let is_now_markdown = {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            state.is_markdown_mode = !state.is_markdown_mode;
            state.is_markdown_mode
        } else {
            return;
        }
    };

    // Use message passing to update UI on the correct thread
    unsafe {
        if is_now_markdown {
            let _ = PostMessageW(
                Some(hwnd),
                event_handler::misc::WM_CREATE_WEBVIEW,
                WPARAM(0),
                LPARAM(0),
            );
        } else {
            let _ = PostMessageW(
                Some(hwnd),
                event_handler::misc::WM_HIDE_MARKDOWN,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    // Update canvas to reflect the new state (e.g., active icon state)
    button_canvas::update_window_position(hwnd);
}

/// Trigger download HTML
pub fn trigger_download(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    let full_text = {
        let states = WINDOW_STATES.lock().unwrap();
        states
            .get(&hwnd_key)
            .map(|s| s.full_text.clone())
            .unwrap_or_default()
    };

    if !full_text.is_empty() {
        markdown_view::save_html_file(&full_text);
    }
}

/// Trigger speaker/TTS
pub fn trigger_speaker(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    let (full_text, current_tts_id, is_loading) = {
        let states = WINDOW_STATES.lock().unwrap();
        states
            .get(&hwnd_key)
            .map(|s| (s.full_text.clone(), s.tts_request_id, s.tts_loading))
            .unwrap_or_default()
    };

    if is_loading {
        return; // Ignore during loading
    }

    if current_tts_id != 0 && crate::api::tts::TTS_MANAGER.is_speaking(current_tts_id) {
        // Stop speaking
        crate::api::tts::TTS_MANAGER.stop();
        {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&hwnd_key) {
                state.tts_request_id = 0;
                state.tts_loading = false;
            }
        }
    } else if !full_text.is_empty() {
        // Start speaking
        {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&hwnd_key) {
                state.tts_loading = true;
            }
        }

        let request_id = crate::api::tts::TTS_MANAGER.speak(&full_text, hwnd_key);
        {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&hwnd_key) {
                state.tts_request_id = request_id;
            }
        }
    }

    button_canvas::update_window_position(hwnd);
}

/// Trigger close all windows
pub fn trigger_close_all() {
    let targets: Vec<HWND> = {
        let states = WINDOW_STATES.lock().unwrap();
        states
            .keys()
            .map(|&k| HWND(k as *mut std::ffi::c_void))
            .collect()
    };

    for hwnd in targets {
        unsafe {
            if windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Trigger drag window (move by delta)
pub fn trigger_drag_window(hwnd: HWND, dx: i32, dy: i32) {
    unsafe {
        let mut rect = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect);

        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            hwnd,
            None,
            rect.left + dx,
            rect.top + dy,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
        );
    }

    // Update canvas with new position
    button_canvas::update_window_position(hwnd);
}
