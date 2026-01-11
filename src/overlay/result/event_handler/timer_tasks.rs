use super::super::logic;
use crate::overlay::result::markdown_view;
use crate::overlay::result::refine_input;
use crate::overlay::result::state::{RefineContext, WINDOW_STATES};
use crate::overlay::utils::to_wstring;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::*;

pub unsafe fn handle_timer(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let timer_id = wparam.0;

    // Timer ID 2: Markdown hover polling (The Authority on WebView Sizing)
    if timer_id == 2 {
        let mut cursor_pos = POINT::default();
        let _ = GetCursorPos(&mut cursor_pos);
        let mut window_rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut window_rect);

        // Check if cursor is geometrically inside the window rect
        let cursor_inside = cursor_pos.x >= window_rect.left
            && cursor_pos.x < window_rect.right
            && cursor_pos.y >= window_rect.top
            && cursor_pos.y < window_rect.bottom;

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
                markdown_view::fit_font_to_window(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
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
                        state.on_back_btn = false;
                        state.on_forward_btn = false;
                    }
                }
                markdown_view::resize_markdown_webview(hwnd, false);
                markdown_view::fit_font_to_window(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
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
                if state.animation_offset < -3600.0 {
                    state.animation_offset += 3600.0;
                }

                // Refresh markdown WebView when refinement starts to show the context quote
                if state.is_markdown_mode && state.font_cache_dirty {
                    state.font_cache_dirty = false;
                    markdown_view::update_markdown_content_ex(
                        hwnd,
                        &state.full_text,
                        true,
                        &state.preset_prompt,
                        &state.input_text,
                    );
                }

                need_repaint = true;
            }

            // Detect streaming end transition to force flush remaining text
            // When streaming was active but is now inactive, we must render any leftover text
            let streaming_just_ended = state.was_streaming_active && !state.is_streaming_active;
            if streaming_just_ended {
                state.was_streaming_active = false;
            }

            // Safety: If streaming is NOT active, always process pending text immediately
            // This ensures any leftover text is rendered even if streaming_just_ended was missed
            let not_streaming = !state.is_streaming_active;

            // Track if we need to force font cache dirty (bypass 200ms throttle)
            // This is critical for rendering the final text after streaming ends

            // Throttle - but bypass if:
            // 1. streaming_just_ended (transition detection)
            // 2. not_streaming (any pending text after streaming should render immediately)
            // 3. first update (last_text_update_time == 0)
            // 4. throttle expired (>16ms since last update)
            if state.pending_text.is_some()
                && (streaming_just_ended
                    || not_streaming
                    || state.last_text_update_time == 0
                    || now.wrapping_sub(state.last_text_update_time) > 16)
            {
                pending_update = state.pending_text.take();
                state.last_text_update_time = now;

                // CRITICAL: When streaming ends, force font recalculation
                // to ensure the final text is properly rendered (bypass 200ms throttle)
                if not_streaming {
                    state.font_cache_dirty = true;
                }
            }

            // Note: Native EDIT control handling removed - both plain text and markdown modes
            // now use WebView-based refine input. Polling happens outside the lock below.
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
                        state.input_text = text_to_refine.clone();
                        state.is_editing = false;
                        state.is_refining = true;
                        state.is_streaming_active = true; // Hide buttons during refinement
                        state.was_streaming_active = true; // Track for end-of-stream flush
                        state.full_text = String::new();
                        state.pending_text = Some(String::new());
                    }
                }

                // Hide the refine input
                refine_input::hide_refine_input(hwnd);

                // Resize markdown WebView back to normal
                let is_hovered = {
                    let states = WINDOW_STATES.lock().unwrap();
                    states
                        .get(&(hwnd.0 as isize))
                        .map(|s| s.is_hovered)
                        .unwrap_or(false)
                };
                markdown_view::resize_markdown_webview(hwnd, is_hovered);
                markdown_view::fit_font_to_window(hwnd);

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
                    states
                        .get(&(hwnd.0 as isize))
                        .map(|s| s.is_hovered)
                        .unwrap_or(false)
                };
                markdown_view::resize_markdown_webview(hwnd, is_hovered);
                markdown_view::fit_font_to_window(hwnd);
            }
        }
    }

    if let Some(txt) = pending_update {
        let (maybe_markdown_update, is_hovered, is_markdown_streaming, is_streaming) = {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                // 200ms font recalc throttling during streaming/text updates
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u32)
                    .unwrap_or(0);
                let time_since_last_calc = now.wrapping_sub(state.last_font_calc_time);
                if time_since_last_calc >= 200 || state.last_font_calc_time == 0 {
                    state.font_cache_dirty = true;
                    state.last_font_calc_time = now;
                }
                state.full_text = txt.clone();

                if state.is_markdown_mode && !state.is_refining {
                    (
                        Some(state.full_text.clone()),
                        state.is_hovered,
                        state.is_markdown_streaming,
                        state.is_streaming_active,
                    )
                } else {
                    (None, false, false, false)
                }
            } else {
                (None, false, false, false)
            }
        };

        if let Some(md_text) = maybe_markdown_update {
            // MARKDOWN MODE - OPTIMIZED PATH
            // Skip SetWindowTextW and InvalidateRect to prevent double-rendering lag behind WebView

            // Use streaming-optimized update for markdown_stream mode during active streaming
            if is_markdown_streaming && is_streaming {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u32)
                    .unwrap_or(0);

                let mut should_update_webview = false;
                {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        let time_since_last_webview =
                            now.wrapping_sub(state.last_webview_update_time);
                        if time_since_last_webview >= 80 || state.last_webview_update_time == 0 {
                            state.last_webview_update_time = now;
                            should_update_webview = true;
                        }
                    }
                }

                if should_update_webview {
                    markdown_view::stream_markdown_content(hwnd, &md_text);
                    // Register with button canvas (may already be registered, that's fine)
                    crate::overlay::result::button_canvas::register_markdown_window(hwnd);
                }
            } else if is_markdown_streaming && !is_streaming {
                // Streaming just ended in markdown_stream mode
                // Render the FINAL content first (in case last chunks weren't rendered due to throttling)
                markdown_view::stream_markdown_content(hwnd, &md_text);
                // Initialize Grid.js on any tables
                markdown_view::init_gridjs(hwnd);
                // Fit font to fill any unfilled space
                markdown_view::fit_font_to_window(hwnd);
                // Now reset for next session
                markdown_view::reset_stream_counter(hwnd);
                // Reset throttle for next time
                {
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                        state.last_webview_update_time = 0;
                    }
                }
                // Register with button canvas
                crate::overlay::result::button_canvas::register_markdown_window(hwnd);
            } else {
                // Regular markdown mode (not streaming) - full render
                markdown_view::reset_stream_counter(hwnd);
                markdown_view::create_markdown_webview(hwnd, &md_text, is_hovered);
                // Fit font to fill any unfilled space
                markdown_view::fit_font_to_window(hwnd);
                // Register with button canvas
                crate::overlay::result::button_canvas::register_markdown_window(hwnd);
            }

            // Do NOT set need_repaint = true here.
            // The WebView handles the display. Repainting parent window is wasteful and causes lag.
        } else {
            // PLAIN TEXT MODE (or Refining) - LEGACY PATH
            // Must update window text and trigger GDI repaint
            let wide_text = to_wstring(&txt);
            let _ = SetWindowTextW(hwnd, PCWSTR(wide_text.as_ptr()));
            need_repaint = true;
        }
    }

    // --- TYPE MODE PROMPT LOGIC ---
    if trigger_refine && !user_input.trim().is_empty() {
        let (context_data, model_id, provider, streaming, preset_prompt) = {
            let states = WINDOW_STATES.lock().unwrap();
            if let Some(s) = states.get(&(hwnd.0 as isize)) {
                (
                    s.context_data.clone(),
                    s.model_id.clone(),
                    s.provider.clone(),
                    s.streaming_enabled,
                    s.preset_prompt.clone(),
                )
            } else {
                (
                    RefineContext::None,
                    "scout".to_string(),
                    "groq".to_string(),
                    false,
                    "".to_string(),
                )
            }
        };

        let (final_prev_text, final_user_prompt) =
            if text_to_refine.trim().is_empty() && !preset_prompt.is_empty() {
                (user_input, preset_prompt)
            } else {
                (text_to_refine, user_input)
            };

        let hwnd_val = hwnd.0 as usize;
        std::thread::spawn(move || {
            // let hwnd = HWND(hwnd_val as *mut std::ffi::c_void); // Unused in this closure's scope, removed to silence warning.
            // Actually it IS used in the callback closure below, which captures hwnd_val implicitly if I'm not careful.
            // But the closure passed to refine_text_streaming captures `hwnd`?
            // Wait, the callback `move |chunk|` captures `hwnd` if I refer to it.

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
                    let mut states = WINDOW_STATES.lock().unwrap();
                    if let Some(state) = states.get_mut(&(capture_hwnd.0 as isize)) {
                        if first_chunk {
                            state.is_refining = false;
                            first_chunk = false;
                        }

                        // Handle WIPE_SIGNAL - clear accumulator and use content after signal
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

            // Refinement should ONLY update the current window
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(capture_hwnd.0 as isize)) {
                state.is_refining = false;
                state.is_streaming_active = false; // Refinement complete, show buttons
                match result {
                    Ok(final_text) => {
                        // SUCCESS
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

    logic::handle_timer(hwnd, wparam);
    if need_repaint {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    LRESULT(0)
}
