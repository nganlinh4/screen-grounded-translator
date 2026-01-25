pub mod button_canvas;
mod event_handler;
pub mod layout;
mod logic;
pub mod markdown_view;
pub mod paint;
pub mod state;
mod window;

pub use state::{close_windows_with_token, link_windows, RefineContext, WindowType, WINDOW_STATES};
pub use window::{create_result_window, get_chain_color, update_window_text};

// Trigger functions for button canvas IPC
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

// Helper to check if any window is currently refining/editing
pub fn is_any_refine_active() -> bool {
    let states = WINDOW_STATES.lock().unwrap();
    states.values().any(|s| s.is_editing)
}

// Helper to get the parent HWND of the active refine session
pub fn get_active_refine_parent() -> Option<HWND> {
    let states = WINDOW_STATES.lock().unwrap();
    states
        .iter()
        .find(|(_, s)| s.is_editing)
        .map(|(hwnd, _)| HWND(*hwnd as *mut std::ffi::c_void))
}

// Helper to update refine text
pub fn set_refine_text(hwnd: HWND, text: &str, is_insert: bool) {
    button_canvas::send_refine_text_update(hwnd, text, is_insert);

    // Only update internal state if overwriting (for consistency)
    if !is_insert {
        let hwnd_key = hwnd.0 as isize;
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            state.input_text = text.to_string();
        }
    }
}

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

    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            state.is_editing = !state.is_editing;
            if state.is_editing {
                state.input_text.clear();
            }
        }
    }

    // Update button canvas to reflect changes (show/hide refine bar)
    button_canvas::update_window_position(hwnd);

    // Resize markdown view if needed (to make space? actually refine bar is floating now?)
    // If refine bar is in button canvas, it floats NEXT to the window or below it.
    // So we DON'T need to resize the markdown window anymore!
    // But previously we did.
    // If we want it to "join" the button canvas as a bar UNDER the result, it floats outside.
    // So we don't resize the window.
}

pub fn trigger_refine_submit(hwnd: HWND, text: &str) {
    if text.trim().is_empty() {
        return;
    }

    let hwnd_key = hwnd.0 as isize;

    // Save to history
    crate::overlay::input_history::add_to_history(text);

    let mut should_trigger_refine = false;
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            let text_to_refine = state.full_text.clone();
            state.text_history.push(text_to_refine.clone());
            // Clear redo history when new action is performed
            state.redo_history.clear();

            // Set input_text for the prompt logic
            state.input_text = text_to_refine; // The prompt logic uses this as "previous text" if not empty??

            // WAIT, logic in timer_tasks was:
            // if text_to_refine.trim().is_empty() && !preset_prompt.is_empty() { (user_input, preset_prompt) } else { (text_to_refine, user_input) }
            // user_input was the text from refine input.
            // text_to_refine came from state.full_text.

            // So:
            // state.input_text should store the PREVIOUS full text (context).
            // But timer_tasks logic is confusingly named.

            // Let's set a NEW state field or reuse one.
            // We have `state.input_text`.
            // In timer_tasks:
            // state.input_text = text_to_refine.clone();
            // user_input = input_text; (from poll)

            // We need to pass `user_input` (the refine prompt) to the logic.
            // `state.input_text` seems to be used for something else?
            // "NEW: Input text currently being refined/processed" comments says so.

            // I'll add `pending_refine_prompt` to WindowState?
            // Or reuse `pending_text`? No `pending_text` is for output.

            // Let's use `preset_prompt` ??? No.

            // Re-check timer_tasks logic:
            // 341: let (final_prev_text, final_user_prompt) = ...
            // 345: (text_to_refine, user_input)

            // text_to_refine is what we are refining (the current content).
            // user_input is what we typed.

            // So I need to store `user_input` in state so timer_tasks can pick it up.
            // Or I can spawn the thread directly here?
            // But timer_tasks has the logic.

            // Let's just modify `state` to trigger `timer_tasks` logic.
            // `trigger_refine` boolean in timer_tasks is local.

            // I will implement the logic HERE instead of relying on `timer_tasks` to pick it up.
            // It makes more sense.

            state.full_text = String::new(); // Clear output for streaming
            state.pending_text = Some(String::new());

            // We need to store context for the refinement.
            // Let's put the typed text into `state.preset_prompt` TEMPORARILY?
            // No, that's hacky.

            // I'll add a `refine_prompt` field to WindowState if needed, or pass it to a helper.
            // Actually, `state.input_text` WAS storing `text_to_refine`.
            // The `user_input` was local in `timer_tasks`.

            // I will update WindowState to receive the prompt.
            // But wait, `timer_tasks.rs` has the `TYPE MODE PROMPT LOGIC` block (lines 318+).
            // It checks `if trigger_refine && !user_input.empty()`.

            // I'll move that logic into a public function `start_refinement(hwnd, prompt, context_text)` in `logic.rs` or `mod.rs`
            // and call it from here.
            should_trigger_refine = true;
        }
    }

    if should_trigger_refine {
        // Need to invoke the refinement logic.
        // I will implement `start_refinement` in this file below and call it.
        start_refinement(hwnd, text);
    }

    // Hide UI
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            state.is_editing = false;
            state.is_refining = true;
            state.is_streaming_active = true;
            state.was_streaming_active = true;
        }
    }
    button_canvas::update_window_position(hwnd);
}

pub fn trigger_refine_cancel(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd_key) {
            state.is_editing = false;
        }
    }
    button_canvas::update_window_position(hwnd);
}

// Logic extracted from timer_tasks (simplified)
fn start_refinement(hwnd: HWND, user_prompt: &str) {
    let hwnd_key = hwnd.0 as isize;
    let (context_data, model_id, provider, streaming, preset_prompt, prev_text) = {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(s) = states.get_mut(&hwnd_key) {
            let prev = s.full_text.clone();
            // Setup state for processing
            // s.input_text = prev.clone(); // Removed: Don't pollute input UI state with context
            (
                s.context_data.clone(),
                s.model_id.clone(),
                s.provider.clone(),
                s.streaming_enabled,
                s.preset_prompt.clone(),
                prev,
            )
        } else {
            return;
        }
    };

    let user_input = user_prompt.to_string();

    // Logic for what is prompt and what is context
    let (final_prev_text, final_user_prompt) =
        if prev_text.trim().is_empty() && !preset_prompt.is_empty() {
            // If no text to refine, use user input as text and preset as prompt?
            // Logic copied from timer_tasks:
            // (user_input, preset_prompt)
            (user_input, preset_prompt)
        } else {
            (prev_text, user_input)
        };

    let hwnd_val = hwnd.0 as usize;
    std::thread::spawn(move || {
        let capture_hwnd = HWND(hwnd_val as *mut std::ffi::c_void);

        let (groq_key, gemini_key) = {
            let app = crate::APP.lock().unwrap();
            (
                app.config.api_key.clone(),
                app.config.gemini_api_key.clone(),
            )
        };

        let mut acc_text = String::new();
        let mut first_chunk = true;

        let result = crate::api::refine_text_streaming(
            &groq_key,
            &gemini_key,
            context_data,
            final_prev_text,
            final_user_prompt,
            &model_id,
            &provider,
            streaming,
            {
                let app = crate::APP.lock().unwrap();
                &app.config.ui_language.clone()
            },
            move |chunk| {
                // Check cancellation token? refined_text_streaming should handle it if passed.
                // But here we rely on the callback.

                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(capture_hwnd.0 as isize)) {
                    if first_chunk {
                        state.is_refining = false; // Stop animation loop??
                                                   // In timer_tasks: state.is_refining = false;
                        first_chunk = false;
                    }

                    // Handle WIPE_SIGNAL
                    if chunk.starts_with(crate::api::WIPE_SIGNAL) {
                        acc_text.clear();
                        acc_text.push_str(&chunk[crate::api::WIPE_SIGNAL.len()..]);
                    } else {
                        acc_text.push_str(chunk);
                    }
                    state.pending_text = Some(acc_text.clone());
                    state.full_text = acc_text.clone();
                }
            },
        );

        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&(capture_hwnd.0 as isize)) {
            state.is_refining = false;
            state.is_streaming_active = false;
            match result {
                Ok(final_text) => {
                    state.full_text = final_text.clone();
                    state.pending_text = Some(final_text);
                }
                Err(e) => {
                    let (lang, model_full_name) = {
                        let app = crate::APP.lock().unwrap();
                        let full_name = crate::model_config::get_model_by_id(&model_id)
                            .map(|m| m.full_name)
                            .unwrap_or_else(|| model_id.to_string());
                        (app.config.ui_language.clone(), full_name)
                    };
                    let err_msg = crate::overlay::utils::get_error_message(
                        &e.to_string(),
                        &lang,
                        Some(&model_full_name),
                    );
                    state.pending_text = Some(err_msg.clone());
                    state.full_text = err_msg;
                }
            }
        }
    });
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
            // Switching BACK to plain text
            // We must manually update the window text because the optimized streaming path skipped it!
            let full_text = {
                let states = WINDOW_STATES.lock().unwrap();
                states
                    .get(&hwnd_key)
                    .map(|s| s.full_text.clone())
                    .unwrap_or_default()
            };
            let wide_text = crate::overlay::utils::to_wstring(&full_text);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                hwnd,
                windows::core::PCWSTR(wide_text.as_ptr()),
            );

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
        if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_ok() {
            let (nx, ny) = (rect.left + dx, rect.top + dy);
            let (nw, nh) = (rect.right - rect.left, rect.bottom - rect.top);

            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd,
                None,
                nx,
                ny,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );

            // Update canvas with new position WITHOUT calling GetWindowRect again
            button_canvas::update_window_position_direct(hwnd, nx, ny, nw, nh);
        }
    }
}
