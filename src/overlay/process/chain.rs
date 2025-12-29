use crate::api::{translate_image_streaming, translate_text_streaming};
use crate::config::{Config, Preset, ProcessingBlock};
use crate::gui::settings_ui::get_localized_preset_name;
use crate::overlay::result::{
    create_result_window, get_chain_color, link_windows, update_window_text, RefineContext,
    WindowType, WINDOW_STATES,
};
use crate::overlay::text_input;
use crate::win_types::SendHwnd;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::types::{get_next_window_position, reset_window_position_queue};
use super::window::create_processing_window;

// --- CORE PIPELINE LOGIC ---

pub fn execute_chain_pipeline(
    initial_input: String,
    rect: RECT,
    config: Config,
    preset: Preset,
    context: RefineContext,
) {
    // 1. Create Processing Window (Gradient Glow)
    // This window stays on the current thread (UI thread context for this operation)
    let graphics_mode = config.graphics_mode.clone();
    let processing_hwnd = unsafe { create_processing_window(rect, graphics_mode) };
    unsafe {
        let _ = SendMessageW(processing_hwnd, WM_TIMER, Some(WPARAM(1)), Some(LPARAM(0)));
    }

    // 2. Start the chain execution on a BACKGROUND thread
    // We pass the processing_hwnd so the background thread can close it when appropriate
    let conf_clone = config.clone();
    let blocks = preset.blocks.clone();
    let connections = preset.block_connections.clone();
    let preset_id = preset.id.clone();

    let processing_hwnd_send = SendHwnd(processing_hwnd);
    std::thread::spawn(move || {
        // Reset position queue for new chain
        reset_window_position_queue();

        run_chain_step(
            0,
            initial_input,
            rect,
            blocks,
            connections, // Graph connections
            conf_clone,
            Arc::new(Mutex::new(None)),
            context,
            false,
            Some(processing_hwnd_send), // Pass the handle to be closed later
            Arc::new(AtomicBool::new(false)), // New chains start with cancellation = false
            preset_id,
        );
    });

    // 3. Keep the Processing Window alive on this thread until it is destroyed by the worker
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(Some(processing_hwnd)).as_bool() {
                break;
            }
        }
    }
}

/// Execute chain pipeline with a pre-created cancellation token
/// Used for continuous input mode to track and close previous chain windows
/// NOTE: For text presets, we don't create a processing window (gradient glow).
/// Instead, we rely on the refining animation baked into the result window.
pub fn execute_chain_pipeline_with_token(
    initial_input: String,
    rect: RECT,
    config: Config,
    preset: Preset,
    context: RefineContext,
    cancel_token: Arc<AtomicBool>,
) {
    // For text presets: NO processing window (gradient glow).
    // The result window itself shows the refining animation.

    let blocks = preset.blocks.clone();
    let connections = preset.block_connections.clone();

    // Reset position queue for new chain
    reset_window_position_queue();

    run_chain_step(
        0,
        initial_input,
        rect,
        blocks,
        connections,
        config,
        Arc::new(Mutex::new(None)),
        context,
        false,
        None, // No processing window for text presets
        cancel_token,
        preset.id.clone(),
    );
}

/// Recursive step to run a block in the chain (now supports graph with connections)
pub fn run_chain_step(
    block_idx: usize,
    input_text: String,
    current_rect: RECT,
    blocks: Vec<ProcessingBlock>,
    connections: Vec<(usize, usize)>, // Graph edges: (from_idx, to_idx)
    config: Config,
    parent_hwnd: Arc<Mutex<Option<SendHwnd>>>,
    context: RefineContext, // Passed to Block 0 (Image context)
    skip_execution: bool,   // If true, we just display result
    mut processing_indicator_hwnd: Option<SendHwnd>, // Handle to the "Processing..." overlay
    cancel_token: Arc<AtomicBool>, // Cancellation flag - if true, stop processing
    preset_id: String,
) {
    // Check if cancelled before starting
    if cancel_token.load(Ordering::Relaxed) {
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    if block_idx >= blocks.len() {
        // End of chain. If processing overlay is still active (e.g., all blocks were hidden), close it now.
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    let block = &blocks[block_idx];

    // 1. Resolve Model & Prompt
    let model_id = block.model.clone();
    let model_conf = crate::model_config::get_model_by_id(&model_id);
    let provider = model_conf
        .clone()
        .map(|m| m.provider)
        .unwrap_or("groq".to_string());
    let model_full_name = model_conf.map(|m| m.full_name).unwrap_or(model_id.clone());

    let mut final_prompt = block.prompt.clone();
    for (key, value) in &block.language_vars {
        final_prompt = final_prompt.replace(&format!("{{{}}}", key), value);
    }
    // Fallback: if {language1} is still in prompt but not in language_vars, use selected_language
    if final_prompt.contains("{language1}") && !block.language_vars.contains_key("language1") {
        final_prompt = final_prompt.replace("{language1}", &block.selected_language);
    }
    final_prompt = final_prompt.replace("{language}", &block.selected_language);

    // 2. Determine Visibility & Position
    let visible_count_before = blocks
        .iter()
        .take(block_idx)
        .filter(|b| b.show_overlay)
        .count();
    let bg_color = get_chain_color(visible_count_before);

    // For visible windows: use global queue for sequential snake positioning (first-come-first-serve)
    let my_rect = if block.show_overlay {
        get_next_window_position(current_rect)
    } else {
        current_rect // Hidden blocks don't consume a position
    };

    let mut my_hwnd: Option<HWND> = None;

    // 3. Create Window (if visible)
    // 3. Create Window (if visible)
    if block.block_type == "input_adapter" {
        // Input adapter is invisible and instant
        // Do nothing here, skipping window creation
    } else if block.show_overlay {
        let ctx_clone = if block_idx == 0 {
            context.clone()
        } else {
            RefineContext::None
        };
        let m_id = model_id.clone();
        let prov = provider.clone();
        let prompt_c = final_prompt.clone();
        // CRITICAL: Override streaming to false if render_mode is markdown
        // Markdown + streaming doesn't work properly (causes missing content)
        let stream_en = if block.render_mode == "markdown" {
            false
        } else {
            block.streaming_enabled
        };
        let render_md = block.render_mode.clone();

        let parent_clone = parent_hwnd.clone();
        let (tx_hwnd, rx_hwnd) = std::sync::mpsc::channel();

        // For image blocks, we defer showing the window until first data arrives
        let is_image_block = block.block_type == "image";

        std::thread::spawn(move || {
            // NOTE: wry handles COM internally, explicit initialization may interfere

            let hwnd = create_result_window(
                my_rect,
                WindowType::Primary,
                ctx_clone,
                m_id,
                prov,
                stream_en,
                false,
                prompt_c,
                bg_color,
                &render_md,
            );

            if let Ok(p_guard) = parent_clone.lock() {
                if let Some(ph) = *p_guard {
                    link_windows(ph.0, hwnd);
                }
            }

            // For image blocks: DON'T show window yet - keep it hidden
            // It will be shown when first data arrives (in the streaming callback)
            // For text blocks: show immediately with refining animation
            if !is_image_block {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                }
            }
            let _ = tx_hwnd.send(SendHwnd(hwnd));

            unsafe {
                let mut m = MSG::default();
                while GetMessageW(&mut m, None, 0, 0).into() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                    if !IsWindow(Some(hwnd)).as_bool() {
                        break;
                    }
                }
            }
        });

        my_hwnd = rx_hwnd.recv().ok().map(|h| h.0);

        // Associate cancellation token with this window so destruction stops the chain
        if let Some(h) = my_hwnd {
            let mut s = WINDOW_STATES.lock().unwrap();
            if let Some(st) = s.get_mut(&(h.0 as isize)) {
                st.cancellation_token = Some(cancel_token.clone());
            }
        }

        // Show loading state in the new window
        // For TEXT blocks: use the refining rainbow edge animation
        // For IMAGE blocks: keep using the gradient glow/laser processing window
        if !skip_execution && my_hwnd.is_some() {
            if block.block_type != "image" {
                // Text block: use rainbow edge refining animation
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(my_hwnd.unwrap().0 as isize)) {
                    st.input_text = input_text.clone();
                    st.is_refining = true;
                    st.is_streaming_active = true; // Hide buttons during streaming
                    st.font_cache_dirty = true;
                }
            } else {
                // Image block: also set streaming active to hide buttons
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(my_hwnd.unwrap().0 as isize)) {
                    st.is_streaming_active = true; // Hide buttons during streaming
                }
            }
        }

        // CRITICAL: Close the old "Processing..." overlay ONLY for text blocks
        // For image blocks, we want to keep the beautiful gradient glow animation alive
        if block.block_type != "image" {
            if let Some(h) = processing_indicator_hwnd {
                unsafe {
                    let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                // Consumed. Don't pass it to next steps.
                processing_indicator_hwnd = None;
            }
        }
    } else {
        // HIDDEN BLOCK:
        // We do NOT close processing_indicator_hwnd.
        // It keeps spinning/glowing while we execute this hidden block.
        // It will be passed to the next block.
    }

    // 4. Execution (API Call)
    // 4. Execution (API Call)
    let result_text = if block.block_type == "input_adapter" {
        // Pass-through: return input as-is
        input_text.clone()
    } else if skip_execution {
        if let Some(h) = my_hwnd {
            update_window_text(h, &input_text);
        }
        input_text
    } else {
        let groq_key = config.api_key.clone();
        let gemini_key = config.gemini_api_key.clone();
        // Use JSON format for single-block image extraction (helps with structured output)
        let use_json = block_idx == 0 && blocks.len() == 1 && blocks[0].block_type == "image";

        // CRITICAL: Override streaming to false if render_mode is markdown
        // Markdown + streaming doesn't work properly (causes missing content)
        let actual_streaming_enabled = if block.render_mode == "markdown" {
            false
        } else {
            block.streaming_enabled
        };

        let accumulated = Arc::new(Mutex::new(String::new()));
        let acc_clone = accumulated.clone();

        // Clone model name for use in error handling (original gets moved to API functions)
        let model_name_for_error = model_full_name.clone();

        // For image blocks: track if window has been shown and share processing_hwnd
        let window_shown = Arc::new(Mutex::new(block.block_type != "image")); // true for text, false for image
        let window_shown_clone = window_shown.clone();
        let processing_hwnd_shared = Arc::new(Mutex::new(processing_indicator_hwnd));
        let processing_hwnd_clone = processing_hwnd_shared.clone();

        let res = if block_idx == 0 && matches!(context, RefineContext::Image(_)) {
            // Image Block
            if let RefineContext::Image(img_data) = context.clone() {
                let img = image::load_from_memory(&img_data)
                    .expect("Failed to load png")
                    .to_rgba8();
                translate_image_streaming(
                    &groq_key,
                    &gemini_key,
                    final_prompt,
                    model_full_name,
                    provider,
                    img,
                    actual_streaming_enabled,
                    use_json,
                    |chunk| {
                        let mut t = acc_clone.lock().unwrap();
                        // Handle WIPE_SIGNAL - clear accumulator and use content after signal
                        if chunk.starts_with(crate::api::WIPE_SIGNAL) {
                            t.clear();
                            t.push_str(&chunk[crate::api::WIPE_SIGNAL.len()..]);
                        } else {
                            t.push_str(chunk);
                        }
                        if let Some(h) = my_hwnd {
                            // On first chunk for image blocks: show window and close processing indicator
                            {
                                let mut shown = window_shown_clone.lock().unwrap();
                                if !*shown {
                                    *shown = true;
                                    unsafe {
                                        let _ = ShowWindow(h, SW_SHOW);
                                    }
                                    // Close processing indicator
                                    let mut proc_hwnd = processing_hwnd_clone.lock().unwrap();
                                    if let Some(ph) = proc_hwnd.take() {
                                        unsafe {
                                            let _ = PostMessageW(
                                                Some(ph.0),
                                                WM_CLOSE,
                                                WPARAM(0),
                                                LPARAM(0),
                                            );
                                        }
                                    }
                                }
                            }
                            {
                                let mut s = WINDOW_STATES.lock().unwrap();
                                if let Some(st) = s.get_mut(&(h.0 as isize)) {
                                    st.is_refining = false;
                                }
                            }
                            update_window_text(h, &t);
                        }
                    },
                )
            } else {
                Err(anyhow::anyhow!("Missing image context"))
            }
        } else {
            // Text Block
            // Compute search label for compound models
            let search_label = Some(get_localized_preset_name(&preset_id, &config.ui_language));
            translate_text_streaming(
                &groq_key,
                &gemini_key,
                input_text,
                final_prompt, // CHANGED: Pass final_prompt instead of selected_language
                model_full_name,
                provider,
                actual_streaming_enabled,
                false,
                search_label,
                &config.ui_language,
                |chunk| {
                    let mut t = acc_clone.lock().unwrap();
                    // Handle WIPE_SIGNAL - clear accumulator and use content after signal
                    if chunk.starts_with(crate::api::WIPE_SIGNAL) {
                        t.clear();
                        t.push_str(&chunk[crate::api::WIPE_SIGNAL.len()..]);
                    } else {
                        t.push_str(chunk);
                    }
                    if let Some(h) = my_hwnd {
                        {
                            let mut s = WINDOW_STATES.lock().unwrap();
                            if let Some(st) = s.get_mut(&(h.0 as isize)) {
                                st.is_refining = false;
                                st.font_cache_dirty = true;
                            }
                        }
                        update_window_text(h, &t);
                    }
                },
            )
        };

        if let Some(h) = my_hwnd {
            let mut s = WINDOW_STATES.lock().unwrap();
            if let Some(st) = s.get_mut(&(h.0 as isize)) {
                st.is_refining = false;
                st.is_streaming_active = false; // Streaming complete, show buttons
                st.font_cache_dirty = true;
            }
        }

        match res {
            Ok(txt) => {
                if let Some(h) = my_hwnd {
                    update_window_text(h, &txt);
                }
                txt
            }
            Err(e) => {
                let lang = config.ui_language.clone();
                let err = crate::overlay::utils::get_error_message(
                    &e.to_string(),
                    &lang,
                    Some(&model_name_for_error),
                );
                if let Some(h) = my_hwnd {
                    update_window_text(h, &err);
                }
                String::new()
            }
        }
    };

    // 5. Post-Processing (Copy)
    // 5. Post-Processing (Copy)
    // Handle Auto-Copy for both Text and Image inputs
    // For input_adapter, we must check if we should copy the SOURCE (Image or Text)
    // result_text is input_text for adapters
    let is_input_adapter = block.block_type == "input_adapter";
    let has_content = !result_text.trim().is_empty();

    if block.auto_copy {
        // CASE 1: Image Input Adapter (Source Copy)
        // If this is an input adapter AND we have image context, copy the image.
        // We do this even if result_text (input_text) is empty, because image source has no text.
        if is_input_adapter {
            if let RefineContext::Image(img_data) = context.clone() {
                let img_data_clone = img_data.clone();
                std::thread::spawn(move || {
                    crate::overlay::utils::copy_image_to_clipboard(&img_data_clone);
                });
            }
        }

        // CASE 2: Text Content (Result or Source Text) OR Image Content (Source Copy)
        // Only copy text if it is NOT empty.
        // For paste logic: we proceed if EITHER we have text content OR we just copied an image (is_input_adapter && image context).
        let image_copied = is_input_adapter && matches!(context, RefineContext::Image(_));

        if has_content {
            let txt_c = result_text.clone();
            let txt_for_badge = result_text.clone();
            std::thread::spawn(move || {
                crate::overlay::utils::copy_to_clipboard(&txt_c, HWND::default());
                // Show auto-copy badge notification with text snippet
                crate::overlay::auto_copy_badge::show_auto_copy_badge_text(&txt_for_badge);
            });
        } else if image_copied {
            // For image-only copy, show the badge with image message
            crate::overlay::auto_copy_badge::show_auto_copy_badge_image();
        }

        if has_content || image_copied {
            // Re-clone for the paste thread
            let txt_c = result_text.clone();
            let preset_id_clone = preset_id.clone();

            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Get auto_paste settings from the RUNNING preset (by ID), not active_preset_idx
                let (should_add_newline, should_paste, target_window) = {
                    let app = crate::APP.lock().unwrap();
                    // Find the preset that's actually running this chain
                    if let Some(preset) =
                        app.config.presets.iter().find(|p| p.id == preset_id_clone)
                    {
                        (
                            preset.auto_paste_newline,
                            preset.auto_paste,
                            app.last_active_window,
                        )
                    } else {
                        // Fallback to active preset if not found (shouldn't happen)
                        let active_idx = app.config.active_preset_idx;
                        if active_idx < app.config.presets.len() {
                            let preset = &app.config.presets[active_idx];
                            (
                                preset.auto_paste_newline,
                                preset.auto_paste,
                                app.last_active_window,
                            )
                        } else {
                            (false, false, app.last_active_window)
                        }
                    }
                };

                // If strictly image copied (no text content), we ignore newline logic and just paste (Ctrl+V)
                // If text content exists, we do the full text logic.
                let final_text = if !txt_c.trim().is_empty() {
                    if should_add_newline {
                        format!("{}\n", txt_c)
                    } else {
                        txt_c.clone()
                    }
                } else {
                    String::new() // No text to modify/inject
                };

                // NOTE: We ALREADY copied to clipboard above (Text or Image).
                // Now we just handle the PASTE action.

                if should_paste {
                    // Special Case: If it's pure image copy (no text), we MUST use generic Ctrl+V paste.
                    // We cannot use text injection or set_editor_text.
                    if txt_c.trim().is_empty() {
                        // Image-only paste path
                        if let Some(target) = target_window {
                            crate::overlay::utils::force_focus_and_paste(target.0);
                        }
                    } else {
                        // Text paste path (supports injection)
                        // Check if text input window is active - if so, set text directly
                        if text_input::is_active() {
                            // Use set_editor_text to inject text into the webview editor
                            text_input::set_editor_text(&final_text);
                            text_input::refocus_editor();
                        }
                        // Check if refine input is active - if so, set text there
                        else if crate::overlay::result::refine_input::is_any_refine_active() {
                            if let Some(parent) =
                                crate::overlay::result::refine_input::get_active_refine_parent()
                            {
                                crate::overlay::result::refine_input::set_refine_text(
                                    parent,
                                    &final_text,
                                );
                            }
                        } else if let Some(target) = target_window {
                            // Normal paste to last active window
                            crate::overlay::utils::force_focus_and_paste(target.0);
                        }
                    }
                }
            });
        }
    }

    // Auto-Speak
    if block.auto_speak && !result_text.trim().is_empty() {
        let txt_s = result_text.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            crate::api::tts::TTS_MANAGER.speak(&txt_s, 0);
        });
    }

    // SAVE TO HISTORY: Handle both Text and Image blocks
    if block.show_overlay && !result_text.trim().is_empty() {
        let text_for_history = result_text.clone();

        if block.block_type == "text" {
            std::thread::spawn(move || {
                if let Ok(app) = crate::APP.lock() {
                    app.history.save_text(text_for_history);
                }
            });
        } else if block.block_type == "image" {
            // For image blocks, we need to grab the image data from the context
            // context is RefineContext::Image(Vec<u8>) for the first block
            if let RefineContext::Image(img_bytes) = context.clone() {
                std::thread::spawn(move || {
                    // Decode PNG bytes back to ImageBuffer for the history saver
                    // (HistoryManager::save_image expects ImageBuffer<Rgba<u8>, ...>)
                    if let Ok(img_dynamic) = image::load_from_memory(&img_bytes) {
                        let img_buffer = img_dynamic.to_rgba8();
                        if let Ok(app) = crate::APP.lock() {
                            app.history.save_image(img_buffer, text_for_history);
                        }
                    }
                });
            }
        }
    }

    // 6. Chain Next Steps (Graph-based: find all downstream blocks)
    // Check cancellation before continuing
    if cancel_token.load(Ordering::Relaxed) {
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    if !result_text.trim().is_empty() {
        // Find all downstream blocks from connections
        let downstream_indices: Vec<usize> = connections
            .iter()
            .filter(|(from, _)| *from == block_idx)
            .map(|(_, to)| *to)
            .collect();

        // Determine next blocks:
        // - If connections vec is completely empty (legacy linear chain), use block_idx + 1 fallback
        // - If connections vec has entries (graph mode), use ONLY explicit connections
        let next_blocks: Vec<usize> = if connections.is_empty() {
            // Legacy mode: no graph connections defined, use linear chain
            if block_idx + 1 < blocks.len() {
                vec![block_idx + 1]
            } else {
                vec![]
            }
        } else {
            // Graph mode: use only explicit connections (no fallback)
            downstream_indices
        };

        if next_blocks.is_empty() {
            // End of chain
            if let Some(h) = processing_indicator_hwnd {
                unsafe {
                    let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            return;
        }

        let next_parent = if my_hwnd.is_some() {
            Arc::new(Mutex::new(my_hwnd.map(|h| SendHwnd(h))))
        } else {
            parent_hwnd
        };

        let base_rect = if my_hwnd.is_some() {
            my_rect
        } else {
            current_rect
        };

        // For the first downstream block, pass the processing indicator (if any)
        // For additional parallel branches, spawn new threads without the indicator
        let first_next = next_blocks[0];
        let parallel_branches: Vec<usize> = next_blocks.into_iter().skip(1).collect();

        // Spawn parallel threads for additional branches FIRST
        let next_context = if block.block_type == "input_adapter" {
            context.clone()
        } else {
            RefineContext::None
        };

        let _s_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let _s_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

        for (branch_index, next_idx) in parallel_branches.iter().enumerate() {
            let result_clone = result_text.clone();
            let blocks_clone = blocks.clone();
            let conns_clone = connections.clone();
            let config_clone = config.clone();
            let cancel_clone = cancel_token.clone();
            let parent_clone = next_parent.clone();
            let preset_id_clone = preset_id.clone();
            let next_idx_copy = *next_idx;

            // Capture next_context for parallel branches
            let branch_context = next_context.clone();

            // Position will be determined individually by get_next_window_position inside run_chain_step
            // We just pass the base_rect as a reference point
            let branch_rect = base_rect;

            // Incremental delay for each branch (300ms, 600ms, 900ms, ...)
            // This naturally staggers WebView2 creation without blocking mutexes
            let delay_ms = (branch_index as u64 + 1) * 300;

            std::thread::spawn(move || {
                // CRITICAL: Initialize COM on this thread - required for WebView2
                unsafe {
                    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }

                // Stagger WebView2 creation across parallel branches
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));

                run_chain_step(
                    next_idx_copy,
                    result_clone,
                    branch_rect,
                    blocks_clone,
                    conns_clone,
                    config_clone,
                    parent_clone,
                    branch_context, // Pass the captured context
                    false,
                    None, // No processing indicator for parallel branches
                    cancel_clone,
                    preset_id_clone,
                );
            });
        }

        // Continue with the first downstream block on current thread
        run_chain_step(
            first_next,
            result_text,
            base_rect,
            blocks,
            connections,
            config,
            next_parent,
            next_context, // Pass the context
            false,
            processing_indicator_hwnd, // Pass it along (might be None or Some)
            cancel_token,              // Pass the same token through the chain
            preset_id,
        );
    } else {
        // Chain stopped unexpectedly (empty result or error)
        // Ensure processing overlay is closed
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}
