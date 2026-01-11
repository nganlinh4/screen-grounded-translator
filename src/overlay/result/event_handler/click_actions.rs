use std::mem::size_of;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::*;

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};

use super::misc::WM_CREATE_WEBVIEW;
use crate::overlay::result::refine_input;
use crate::overlay::result::state::{InteractionMode, WINDOW_STATES};
use crate::overlay::result::{button_canvas, markdown_view};
use crate::overlay::utils::to_wstring;

pub unsafe fn handle_lbutton_up(hwnd: HWND) -> LRESULT {
    let _ = ReleaseCapture();
    button_canvas::set_drag_mode(false); // Disable unclipped drag mode
    let mut perform_click = false;
    let mut is_copy_click = false;
    let mut is_edit_click = false;
    let mut is_undo_click = false;
    let mut is_redo_click = false;
    let mut is_markdown_click = false;
    let mut is_back_click = false;
    let mut is_forward_click = false;
    let mut is_download_click = false;
    let mut is_speaker_click = false;
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
            let was_resizing = matches!(state.interaction_mode, InteractionMode::Resizing(_));
            state.interaction_mode = InteractionMode::None;
            if was_resizing && state.is_markdown_mode {
                markdown_view::fit_font_to_window(hwnd);
            }
            if !state.has_moved_significantly {
                perform_click = true;
                is_copy_click = state.on_copy_btn;
                is_edit_click = state.on_edit_btn;
                is_undo_click = state.on_undo_btn;
                is_redo_click = state.on_redo_btn;
                is_markdown_click = state.on_markdown_btn;
                is_back_click = state.on_back_btn;
                is_forward_click = state.on_forward_btn;
                is_download_click = state.on_download_btn;
                is_speaker_click = state.on_speaker_btn;
            }
        }
    }

    if perform_click {
        if is_back_click {
            markdown_view::go_back(hwnd);
        } else if is_forward_click {
            markdown_view::go_forward(hwnd);
        } else if is_undo_click {
            let mut prev_text = None;

            let mut is_markdown = false;
            let mut is_hovered = false;
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    if let Some(last) = state.text_history.pop() {
                        // Save current text to redo history before replacing
                        let current_text_for_redo = state.full_text.clone();
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
                let _ = SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
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

                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        } else if is_redo_click {
            // Redo: pop from redo_history, push current to text_history
            let mut next_text = None;

            let mut is_markdown = false;
            let mut is_hovered = false;
            {
                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    if let Some(redo_text) = state.redo_history.pop() {
                        // Save current text to undo history before replacing
                        let current_text_for_undo = state.full_text.clone();
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
                let _ = SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
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

                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        } else if is_edit_click {
            // Check if we're in markdown mode to decide which input to use
            let (is_markdown_mode, _is_currently_editing, _h_edit) = {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    (state.is_markdown_mode, state.is_editing, state.edit_hwnd)
                } else {
                    (false, false, HWND::default())
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
                        states
                            .get(&(hwnd.0 as isize))
                            .map(|s| s.is_hovered)
                            .unwrap_or(false)
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
                // Plain text mode: now also use WebView-based refine input (same as markdown)
                // This allows the mic button to work in both modes
                if refine_input::is_refine_input_active(hwnd) {
                    // Toggle off - hide the refine input
                    refine_input::hide_refine_input(hwnd);
                    {
                        let mut states = WINDOW_STATES.lock().unwrap();
                        if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                            state.is_editing = false;
                        }
                    }
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
                    }
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
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
            SetTimer(Some(hwnd), 1, 1500, None);
        } else if is_markdown_click {
            // Only allow markdown toggle when NOT refining AND NOT streaming
            let can_toggle = {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    !state.is_refining && !state.is_streaming_active
                } else {
                    false
                }
            };

            if can_toggle {
                // Toggle markdown mode
                let (toggle_on, _full_text) = {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.is_markdown_mode = !state.is_markdown_mode;
                        (state.is_markdown_mode, state.full_text.clone())
                    } else {
                        (false, String::new())
                    }
                };

                if toggle_on {
                    // Switch TO Markdown:
                    // window.rs now creates window without WS_CLIPCHILDREN by default

                    // DEFER WebView creation to after this handler returns
                    let _ = PostMessageW(Some(hwnd), WM_CREATE_WEBVIEW, WPARAM(0), LPARAM(0));
                    SetTimer(Some(hwnd), 2, 30, None);
                } else {
                    // Switch TO Plain Text:
                    // 1. Destroy the WebView completely
                    markdown_view::destroy_markdown_webview(hwnd);

                    // 2. Add WS_CLIPCHILDREN back
                    unsafe {
                        // Force style update (WS_CLIPCHILDREN is permanently off)
                        let _ = SetWindowPos(
                            hwnd,
                            Some(HWND::default()),
                            0,
                            0,
                            0,
                            0,
                            SWP_FRAMECHANGED
                                | SWP_NOMOVE
                                | SWP_NOSIZE
                                | SWP_NOZORDER
                                | SWP_NOACTIVATE,
                        );
                    }

                    // 3. Cleanup timers and restore event tracking
                    let _ = KillTimer(Some(hwnd), 2);

                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
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
        } else if is_speaker_click {
            // TTS - speak the result text
            let (full_text, current_tts_id, is_loading) = {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    (
                        state.full_text.clone(),
                        state.tts_request_id,
                        state.tts_loading,
                    )
                } else {
                    (String::new(), 0, false)
                }
            };

            // Don't allow clicks while loading
            if is_loading {
                // Ignore click during loading state
            } else if current_tts_id != 0
                && crate::api::tts::TTS_MANAGER.is_speaking(current_tts_id)
            {
                // Currently speaking (blue button) - stop immediately
                crate::api::tts::TTS_MANAGER.stop();
                {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.tts_request_id = 0;
                        state.tts_loading = false;
                    }
                }
            } else if !full_text.is_empty() {
                // Start new speech - enter loading state first
                {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.tts_loading = true;
                    }
                }
                let _ = InvalidateRect(Some(hwnd), None, false); // Redraw to show loading

                let request_id = crate::api::tts::TTS_MANAGER.speak(&full_text, hwnd.0 as isize);
                {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.tts_request_id = request_id;
                        // Keep tts_loading = true until audio starts playing
                    }
                }
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
        } else {
            // Clicking "x" (or outside buttons) -> Close window
            let linked_hwnd = {
                let states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get(&(hwnd.0 as isize)) {
                    state.linked_window
                } else {
                    None
                }
            };
            if let Some(linked) = linked_hwnd {
                if IsWindow(Some(linked)).as_bool() {
                    let _ = PostMessageW(Some(linked), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    LRESULT(0)
}

pub unsafe fn handle_rbutton_up(hwnd: HWND) -> LRESULT {
    let _ = ReleaseCapture();
    button_canvas::set_drag_mode(false); // Disable unclipped drag mode
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
        SetTimer(Some(hwnd), 1, 1500, None);
    }
    LRESULT(0)
}

pub unsafe fn handle_mbutton_up() -> LRESULT {
    let mut targets = Vec::new();
    {
        if let Ok(states) = WINDOW_STATES.lock() {
            for (&hwnd_int, _) in states.iter() {
                targets.push(HWND(hwnd_int as *mut std::ffi::c_void));
            }
        }
    }

    for target in targets {
        if IsWindow(Some(target)).as_bool() {
            let _ = PostMessageW(Some(target), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    LRESULT(0)
}
