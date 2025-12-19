use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, Mutex, Once, atomic::{AtomicBool, Ordering}};
use std::collections::HashMap;
use image::{ImageBuffer, Rgba};

use crate::api::{translate_image_streaming, translate_text_streaming};
use crate::gui::settings_ui::get_localized_preset_name;
use crate::config::{Config, Preset, ProcessingBlock};
use super::utils::copy_to_clipboard;
use super::result::{create_result_window, update_window_text, WindowType, link_windows, RefineContext, WINDOW_STATES, get_chain_color, layout::calculate_next_window_rect};
use super::text_input;


// --- PROCESSING WINDOW STATIC STATE ---
static REGISTER_PROC_CLASS: Once = Once::new();
const MAX_GLOW_BUFFER_DIM: i32 = 1280;

struct ProcessingState {
    animation_offset: f32,
    is_fading_out: bool,
    alpha: u8,
    cache_hbm: HBITMAP,
    cache_bits: *mut core::ffi::c_void,
    cache_w: i32,
    cache_h: i32,
    scaled_w: i32,
    scaled_h: i32,
    timer_killed: bool,
    graphics_mode: String,
}

unsafe impl Send for ProcessingState {}

impl ProcessingState {
    fn new(graphics_mode: String) -> Self {
        Self {
            animation_offset: 0.0,
            is_fading_out: false,
            alpha: 255,
            cache_hbm: HBITMAP(0),
            cache_bits: std::ptr::null_mut(),
            cache_w: 0,
            cache_h: 0,
            scaled_w: 0,
            scaled_h: 0,
            timer_killed: false,
            graphics_mode,
        }
    }
    
    fn cleanup(&mut self) {
        if self.cache_hbm.0 != 0 {
            unsafe { DeleteObject(self.cache_hbm); }
            self.cache_hbm = HBITMAP(0);
            self.cache_bits = std::ptr::null_mut();
        }
    }
}

lazy_static::lazy_static! {
    static ref PROC_STATES: Mutex<HashMap<isize, ProcessingState>> = Mutex::new(HashMap::new());
    
    // Global sequential window position queue - ensures snake pattern even with parallel processing
    // Stores Option<RECT> where None means reset to initial position
    static ref LAST_WINDOW_RECT: Mutex<Option<RECT>> = Mutex::new(None);
}

/// Reset the window position queue (call at start of new processing chain)
pub fn reset_window_position_queue() {
    let mut last = LAST_WINDOW_RECT.lock().unwrap();
    *last = None;
}

/// Get the next window position using snake algorithm (first-come-first-serve)
/// This is mutex-protected so parallel branches get sequential positions
pub fn get_next_window_position(initial_rect: RECT) -> RECT {
    let mut last = LAST_WINDOW_RECT.lock().unwrap();
    
    let s_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let s_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    
    let next_rect = match *last {
        None => {
            // First window: use initial rect
            initial_rect
        }
        Some(prev) => {
            // Subsequent windows: use snake algorithm from last position
            calculate_next_window_rect(prev, s_w, s_h)
        }
    };
    
    // Update last position for next caller
    *last = Some(next_rect);
    
    next_rect
}



// --- ENTRY POINTS ---

pub fn start_text_processing(
    initial_text_content: String,
    screen_rect: RECT,
    config: Config,
    preset: Preset,
    localized_preset_name: String,  // Already localized by caller
    cancel_hotkey_name: String,     // The actual hotkey name like "Ctrl+Shift+D"
) {

    if preset.text_input_mode == "type" {
        // Use blocks[0].prompt instead of legacy preset.prompt
        let first_block_prompt = preset.blocks.first().map(|b| b.prompt.as_str()).unwrap_or("");
        
        let guide_text = if first_block_prompt.is_empty() { 
            String::new()
        } else { 
            format!("{}...", localized_preset_name) 
        };

        let config_shared = Arc::new(config.clone());
        let preset_shared = Arc::new(preset.clone());
        let ui_lang = config.ui_language.clone();
        // For MASTER presets: always keep window open initially (continuous_mode=true)
        // We'll decide whether to close based on the SELECTED preset after wheel selection
        let continuous_mode = if preset.is_master { true } else { preset.continuous_input };
        
        // For continuous mode: store the previous chain's cancellation token so we can close old windows
        let last_cancel_token: Arc<Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>> = 
            Arc::new(Mutex::new(None));
        let last_cancel_token_clone = last_cancel_token.clone();
        
        // Check if this is a MASTER preset
        let is_master = preset.is_master;
        
        // CRITICAL: For MASTER presets, store the selected preset index after first wheel selection.
        // Subsequent Enter presses will use this stored preset directly (no wheel).
        // The text input window "transfers" to the selected preset.
        let selected_preset_idx: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
        let selected_preset_idx_clone = selected_preset_idx.clone();
        
        text_input::show(guide_text, ui_lang, cancel_hotkey_name, continuous_mode, move |user_text, input_hwnd| {
            // Check if we already selected a preset from the wheel (subsequent submissions)
            let already_selected = selected_preset_idx_clone.lock().unwrap().clone();
            
            let (final_preset, final_config, is_continuous) = if let Some(preset_idx) = already_selected {
                // Already selected from wheel previously - use that preset directly (no wheel)
                let app = crate::APP.lock().unwrap();
                let p = app.config.presets[preset_idx].clone();
                let c = app.config.clone();
                let continuous = p.continuous_input;
                (p, c, continuous)
            } else if is_master {
                // First time MASTER preset - show the preset wheel
                let mut cursor_pos = POINT::default();
                unsafe { GetCursorPos(&mut cursor_pos); }
                
                // Show preset wheel - this blocks until user makes selection
                let selected = super::preset_wheel::show_preset_wheel("text", Some("type"), cursor_pos);
                
                if let Some(idx) = selected {
                    // Store the selected preset index for subsequent submissions
                    *selected_preset_idx_clone.lock().unwrap() = Some(idx);
                    
                    // Refocus the text input window and editor after wheel closes
                    text_input::refocus_editor();
                    
                    // Get the selected preset from config AND update active_preset_idx
                    let mut app = crate::APP.lock().unwrap();
                    // CRITICAL: Update active_preset_idx so auto_paste logic works!
                    app.config.active_preset_idx = idx;
                    let p = app.config.presets[idx].clone();
                    let c = app.config.clone();
                    let continuous = p.continuous_input;

                    // Update UI header with the new preset's name
                    let localized_name = crate::gui::settings_ui::get_localized_preset_name(&p.id, &c.ui_language);
                    // Find first hotkey name for this preset if available
                    let hk_name = p.hotkeys.first().map(|h| h.name.clone()).unwrap_or_default();
                    
                    let new_guide_text = if !hk_name.is_empty() {
                        format!("{} [{}]", localized_name, hk_name)
                    } else {
                        localized_name
                    };
                    text_input::update_ui_text(new_guide_text);

                    (p, c, continuous)
                } else {
                    // User dismissed wheel - refocus and allow retry
                    text_input::refocus_editor();
                    return;
                }
            } else {
                // Normal non-MASTER preset
                let is_continuous = (*preset_shared).continuous_input;
                ((*preset_shared).clone(), (*config_shared).clone(), is_continuous)
            };
            
            if !is_continuous {
                // Normal mode: close input window
                unsafe { PostMessageW(input_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            } else {
                // Continuous mode: close previous result overlays before spawning new ones
                if let Ok(mut token_guard) = last_cancel_token_clone.lock() {
                    if let Some(ref old_token) = *token_guard {
                        // Close windows from previous submission
                        super::result::close_windows_with_token(old_token);
                    }
                }
            }
            
            // Calculate overlay position:
            // - Normal mode: use screen_rect (center of screen or original location)
            // - Continuous mode: spawn below the input window
            let overlay_rect = if is_continuous {
                if let Some(input_rect) = text_input::get_window_rect() {
                    // Position below the input window with a small gap
                    RECT {
                        left: input_rect.left,
                        top: input_rect.bottom + 10, // 10px gap below input window
                        right: input_rect.right,
                        bottom: input_rect.bottom + 10 + (screen_rect.bottom - screen_rect.top),
                    }
                } else {
                    screen_rect
                }
            } else {
                screen_rect
            };
            
            // Start processing and track the new cancellation token for continuous mode
            let config_clone = final_config;
            let preset_clone = final_preset;
            let last_token_update = last_cancel_token_clone.clone();
            
            std::thread::spawn(move || {
                // Create a new cancellation token for this chain
                let new_token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                
                // Store it for later cleanup (in continuous mode)
                if let Ok(mut token_guard) = last_token_update.lock() {
                    *token_guard = Some(new_token.clone());
                }
                
                // Execute the chain
                execute_chain_pipeline_with_token(
                    user_text, 
                    overlay_rect, 
                    config_clone, 
                    preset_clone, 
                    RefineContext::None,
                    new_token
                );
            });
        });
    } else if preset.prompt_mode == "dynamic" {
        // Dynamic prompt mode for text selection: show WebView input for user to type command
        let ui_lang = config.ui_language.clone();
        // Header shows just the localized preset name (hotkey goes to footer via cancel_hotkey_name)
        let guide_text = localized_preset_name.clone();
        
        // Store for use in callback
        let initial_text = Arc::new(initial_text_content);
        let config = Arc::new(config);
        let preset = Arc::new(preset);
        
        text_input::show(guide_text, ui_lang, cancel_hotkey_name, false, move |user_prompt, input_hwnd| {
            // Close the input window
            unsafe { PostMessageW(input_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            
            // Clone preset and modify the first block's prompt with user's input
            let mut modified_preset = (*preset).clone();
            if let Some(block0) = modified_preset.blocks.get_mut(0) {
                if block0.prompt.is_empty() {
                    block0.prompt = user_prompt.clone();
                } else {
                    block0.prompt = format!("{}\n\nUser request: {}", block0.prompt, user_prompt);
                }
            }
            
            let config_clone = (*config).clone();
            let initial_text_clone = (*initial_text).clone();
            
            // Execute the chain with modified preset
            std::thread::spawn(move || {
                execute_chain_pipeline(
                    initial_text_clone, 
                    screen_rect, 
                    config_clone, 
                    modified_preset, 
                    RefineContext::None
                );
            });
        });
    } else {
        execute_chain_pipeline(initial_text_content, screen_rect, config, preset, RefineContext::None);
    }
}

pub fn show_audio_result(
    preset: Preset,
    transcription_text: String,
    rect: RECT,
    _unused_rect: Option<RECT>,
    recording_hwnd: HWND, // Recording overlay window - keep alive until first visible block
) {
    let config = {
        let app = crate::APP.lock().unwrap();
        app.config.clone()
    };
    
    // Audio processing already completed Block 0 (audio recording/transcription).
    // Start at block 0 with skip_execution=true so it can display its overlay (if configured),
    // then the chain naturally continues to block 1, 2, etc.
    // 
    // Pass the recording_hwnd as processing_indicator_hwnd - it will keep animating
    // until the first visible block appears (same behavior as image pipeline).
    let processing_hwnd = if unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(recording_hwnd).as_bool() } {
        Some(recording_hwnd)
    } else {
        None
    };
    
    // Reset position queue for new chain
    reset_window_position_queue();
    
    run_chain_step(
        0, 
        transcription_text,
        rect, 
        preset.blocks.clone(),
        preset.block_connections.clone(), // Graph connections
        config, 
        Arc::new(Mutex::new(None)),
        RefineContext::None,
        true, // skip_execution: audio already done, just display and chain forward
        processing_hwnd, // Pass recording overlay - will close when first visible block appears
        Arc::new(AtomicBool::new(false)), // New chains start with cancellation = false
        preset.id.clone()
    );
}

pub fn start_processing_pipeline(
    cropped_img: ImageBuffer<Rgba<u8>, Vec<u8>>, 
    screen_rect: RECT, 
    config: Config, 
    preset: Preset
) {
    // If dynamic prompt mode, use WebView-based text input
    if preset.prompt_mode == "dynamic" && !preset.blocks.is_empty() {
        // For dynamic mode, encode PNG first (user will type prompt)
        let mut png_data = Vec::new();
        let _ = cropped_img.write_to(&mut std::io::Cursor::new(&mut png_data), image::ImageFormat::Png);
        
        // Get localized UI elements
        let ui_lang = config.ui_language.clone();
        let localized_name = crate::gui::settings_ui::get_localized_preset_name(&preset.id, &ui_lang);
        let guide_text = format!("{}...", localized_name);
        let cancel_hotkey = preset.hotkeys.first().map(|h| h.name.clone()).unwrap_or_default();
        
        // Store for use in callback
        let png_data = Arc::new(png_data);
        let config = Arc::new(config);
        let preset = Arc::new(preset);
        
        // Use WebView-based text input
        text_input::show(guide_text, ui_lang, cancel_hotkey, false, move |user_prompt, input_hwnd| {
            // Close the input window
            unsafe { PostMessageW(input_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            
            // Clone preset and modify the first block's prompt with user's input
            let mut modified_preset = (*preset).clone();
            if let Some(block0) = modified_preset.blocks.get_mut(0) {
                if block0.prompt.is_empty() {
                    block0.prompt = user_prompt.clone();
                } else {
                    block0.prompt = format!("{}\n\nUser request: {}", block0.prompt, user_prompt);
                }
            }
            
            // Context with image data
            let context = RefineContext::Image((*png_data).clone());
            let config_clone = (*config).clone();
            let graphics_mode = config_clone.graphics_mode.clone();
            
            // Create processing window IMMEDIATELY
            let processing_hwnd = unsafe { create_processing_window(screen_rect, graphics_mode) };
            unsafe { SendMessageW(processing_hwnd, WM_TIMER, WPARAM(1), LPARAM(0)); }
            
            // Reset position queue for new chain
            reset_window_position_queue();
            
            // Spawn chain execution - reusing existing run_chain_step!
            let blocks = modified_preset.blocks.clone();
            let connections = modified_preset.block_connections.clone();
            let preset_id = modified_preset.id.clone();
            
            std::thread::spawn(move || {
                run_chain_step(
                    0, 
                    String::new(), 
                    screen_rect, 
                    blocks,
                    connections,
                    config_clone, 
                    Arc::new(Mutex::new(None)), 
                    context, 
                    false,
                    Some(processing_hwnd),
                    Arc::new(AtomicBool::new(false)),
                    preset_id
                );
            });
            
            // Keep processing window alive until closed
            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    if !IsWindow(processing_hwnd).as_bool() { break; }
                }
            }
        });
        return;
    }

    // STANDARD PIPELINE: Create processing window IMMEDIATELY, then encode PNG in background
    // This eliminates the delay between selection and animation appearing
    
    // 1. Create Processing Window FIRST (instant, no delay)
    let graphics_mode = config.graphics_mode.clone();
    let processing_hwnd = unsafe { create_processing_window(screen_rect, graphics_mode) };
    unsafe { SendMessageW(processing_hwnd, WM_TIMER, WPARAM(1), LPARAM(0)); }
    
    // 2. Spawn background thread to encode PNG and start chain execution
    let conf_clone = config.clone();
    let blocks = preset.blocks.clone();
    let connections = preset.block_connections.clone();
    let preset_id = preset.id.clone();
    
    std::thread::spawn(move || {
        // Heavy work: PNG encoding happens here, while animation plays
        let mut png_data = Vec::new();
        let _ = cropped_img.write_to(&mut std::io::Cursor::new(&mut png_data), image::ImageFormat::Png);
        let context = RefineContext::Image(png_data);
        
        // Reset position queue for new chain
        reset_window_position_queue();
        
        // Start chain execution with the pre-created processing window
        run_chain_step(
            0, 
            String::new(), 
            screen_rect, 
            blocks,
            connections, // Graph connections
            conf_clone, 
            Arc::new(Mutex::new(None)), 
            context, 
            false,
            Some(processing_hwnd), // Pass the handle to be closed later
            Arc::new(AtomicBool::new(false)), // New chains start with cancellation = false
            preset_id
        );
    });
    
    // 3. Keep the Processing Window alive on this thread until it is destroyed by the worker
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(processing_hwnd).as_bool() { break; }
        }
    }
}

// --- CORE PIPELINE LOGIC ---

fn execute_chain_pipeline(
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
    unsafe { SendMessageW(processing_hwnd, WM_TIMER, WPARAM(1), LPARAM(0)); }

    // 2. Start the chain execution on a BACKGROUND thread
    // We pass the processing_hwnd so the background thread can close it when appropriate
    let conf_clone = config.clone();
    let blocks = preset.blocks.clone();
    let connections = preset.block_connections.clone();
    let preset_id = preset.id.clone();
    
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
            Some(processing_hwnd), // Pass the handle to be closed later
            Arc::new(AtomicBool::new(false)), // New chains start with cancellation = false
            preset_id
        );
    });
    
    // 3. Keep the Processing Window alive on this thread until it is destroyed by the worker
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(processing_hwnd).as_bool() { break; }
        }
    }
}

/// Execute chain pipeline with a pre-created cancellation token
/// Used for continuous input mode to track and close previous chain windows
/// NOTE: For text presets, we don't create a processing window (gradient glow).
/// Instead, we rely on the refining animation baked into the result window.
fn execute_chain_pipeline_with_token(
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
        preset.id.clone()
    );
}

/// Recursive step to run a block in the chain (now supports graph with connections)
fn run_chain_step(
    block_idx: usize,
    input_text: String,
    current_rect: RECT,
    blocks: Vec<ProcessingBlock>,
    connections: Vec<(usize, usize)>, // Graph edges: (from_idx, to_idx)
    config: Config,
    parent_hwnd: Arc<Mutex<Option<HWND>>>,
    context: RefineContext, // Passed to Block 0 (Image context)
    skip_execution: bool,   // If true, we just display result
    mut processing_indicator_hwnd: Option<HWND>, // Handle to the "Processing..." overlay
    cancel_token: Arc<AtomicBool>, // Cancellation flag - if true, stop processing
    preset_id: String,
) {
    // Check if cancelled before starting
    if cancel_token.load(Ordering::Relaxed) {
        if let Some(h) = processing_indicator_hwnd {
            unsafe { PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
        return;
    }
    
    if block_idx >= blocks.len() { 
        // End of chain. If processing overlay is still active (e.g., all blocks were hidden), close it now.
        if let Some(h) = processing_indicator_hwnd {
             unsafe { PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
        return; 
    }
    
    let block = &blocks[block_idx];
    
    // 1. Resolve Model & Prompt
    let model_id = block.model.clone();
    let model_conf = crate::model_config::get_model_by_id(&model_id);
    let provider = model_conf.clone().map(|m| m.provider).unwrap_or("groq".to_string());
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
    let visible_count_before = blocks.iter().take(block_idx).filter(|b| b.show_overlay).count();
    let bg_color = get_chain_color(visible_count_before);
    
    // For visible windows: use global queue for sequential snake positioning (first-come-first-serve)
    let my_rect = if block.show_overlay {
        get_next_window_position(current_rect)
    } else {
        current_rect // Hidden blocks don't consume a position
    };

    let mut my_hwnd: Option<HWND> = None;

    // 3. Create Window (if visible)
    if block.show_overlay {
        let ctx_clone = if block_idx == 0 { context.clone() } else { RefineContext::None }; 
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
                None, 
                bg_color,
                &render_md
            );
            
            if let Ok(p_guard) = parent_clone.lock() {
                if let Some(ph) = *p_guard {
                    link_windows(ph, hwnd);
                }
            }
            
            // For image blocks: DON'T show window yet - keep it hidden
            // It will be shown when first data arrives (in the streaming callback)
            // For text blocks: show immediately with refining animation
            if !is_image_block {
                unsafe { ShowWindow(hwnd, SW_SHOW); }
            }
            let _ = tx_hwnd.send(hwnd);
            
            unsafe { 
                let mut m = MSG::default(); 
                while GetMessageW(&mut m, None, 0, 0).into() { 
                    TranslateMessage(&m); 
                    DispatchMessageW(&m); 
                    if !IsWindow(hwnd).as_bool() { break; } 
                } 
            }
        });
        
        my_hwnd = rx_hwnd.recv().ok();
        
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
                    st.font_cache_dirty = true;
                }
            }
        }

        // CRITICAL: Close the old "Processing..." overlay ONLY for text blocks
        // For image blocks, we want to keep the beautiful gradient glow animation alive
        if block.block_type != "image" {
            if let Some(h) = processing_indicator_hwnd {
                unsafe { PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0)); }
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
    let result_text = if skip_execution {
        if let Some(h) = my_hwnd { update_window_text(h, &input_text); }
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
                let img = image::load_from_memory(&img_data).expect("Failed to load png").to_rgba8();
                translate_image_streaming(
                    &groq_key, &gemini_key, final_prompt, model_full_name, provider, img, 
                    actual_streaming_enabled, use_json, 
                    |chunk| {
                        let mut t = acc_clone.lock().unwrap(); t.push_str(chunk);
                        if let Some(h) = my_hwnd { 
                            // On first chunk for image blocks: show window and close processing indicator
                            {
                                let mut shown = window_shown_clone.lock().unwrap();
                                if !*shown {
                                    *shown = true;
                                    unsafe { ShowWindow(h, SW_SHOW); }
                                    // Close processing indicator
                                    let mut proc_hwnd = processing_hwnd_clone.lock().unwrap();
                                    if let Some(ph) = proc_hwnd.take() {
                                        unsafe { PostMessageW(ph, WM_CLOSE, WPARAM(0), LPARAM(0)); }
                                    }
                                }
                            }
                            {
                                let mut s = WINDOW_STATES.lock().unwrap();
                                if let Some(st) = s.get_mut(&(h.0 as isize)) { st.is_refining = false; }
                            }
                            update_window_text(h, &t); 
                        }
                    }
                )
            } else {
                Err(anyhow::anyhow!("Missing image context"))
            }
        } else {
            // Text Block
            // Compute search label for compound models
            let search_label = Some(get_localized_preset_name(&preset_id, &config.ui_language));
            translate_text_streaming(
                &groq_key, &gemini_key, input_text, final_prompt, // CHANGED: Pass final_prompt instead of selected_language
                model_full_name, provider, actual_streaming_enabled, false, search_label, &config.ui_language,
                |chunk| {
                    let mut t = acc_clone.lock().unwrap(); t.push_str(chunk);
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
                }
            )
        };

        if let Some(h) = my_hwnd {
             let mut s = WINDOW_STATES.lock().unwrap();
             if let Some(st) = s.get_mut(&(h.0 as isize)) { 
                 st.is_refining = false; 
                 st.font_cache_dirty = true;
             }
        }

        match res {
            Ok(txt) => {
                if let Some(h) = my_hwnd { update_window_text(h, &txt); }
                txt
            },
            Err(e) => {
                let lang = config.ui_language.clone();
                let err = crate::overlay::utils::get_error_message(&e.to_string(), &lang, Some(&model_name_for_error));
                if let Some(h) = my_hwnd { update_window_text(h, &err); }
                String::new()
            }
        }
    };

    // 5. Post-Processing (Copy)
    if block.auto_copy && !result_text.trim().is_empty() {
        let txt_c = result_text.clone();
        let preset_id_clone = preset_id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            
            // Get auto_paste settings from the RUNNING preset (by ID), not active_preset_idx
            let (should_add_newline, should_paste, target_window) = {
                let app = crate::APP.lock().unwrap();
                // Find the preset that's actually running this chain
                if let Some(preset) = app.config.presets.iter().find(|p| p.id == preset_id_clone) {
                    (preset.auto_paste_newline, preset.auto_paste, app.last_active_window)
                } else {
                    // Fallback to active preset if not found (shouldn't happen)
                    let active_idx = app.config.active_preset_idx;
                    if active_idx < app.config.presets.len() {
                        let preset = &app.config.presets[active_idx];
                        (preset.auto_paste_newline, preset.auto_paste, app.last_active_window)
                    } else {
                        (false, false, app.last_active_window)
                    }
                }
            };
            
            // Append newline if setting is enabled
            let final_text = if should_add_newline {
                format!("{}\n", txt_c)
            } else {
                txt_c
            };
            
            copy_to_clipboard(&final_text, HWND(0));
            
            if should_paste {
                // Check if text input window is active - if so, set text directly
                if text_input::is_active() {
                    // Use set_editor_text to inject text into the webview editor
                    text_input::set_editor_text(&final_text);
                    text_input::refocus_editor();
                } 
                // Check if refine input is active - if so, set text there
                else if crate::overlay::result::refine_input::is_any_refine_active() {
                    if let Some(parent) = crate::overlay::result::refine_input::get_active_refine_parent() {
                        crate::overlay::result::refine_input::set_refine_text(parent, &final_text);
                    }
                }
                else if let Some(target) = target_window {
                    // Normal paste to last active window
                    crate::overlay::utils::force_focus_and_paste(target);
                }
            }
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
            unsafe { PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
        return;
    }
    
    if !result_text.trim().is_empty() {
        // Find all downstream blocks from connections
        let downstream_indices: Vec<usize> = connections.iter()
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
                unsafe { PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            }
            return;
        }
        
        let next_parent = if my_hwnd.is_some() {
            Arc::new(Mutex::new(my_hwnd))
        } else {
            parent_hwnd 
        };
        
        let base_rect = if my_hwnd.is_some() { my_rect } else { current_rect };
        
        // For the first downstream block, pass the processing indicator (if any)
        // For additional parallel branches, spawn new threads without the indicator
        let first_next = next_blocks[0];
        let parallel_branches: Vec<usize> = next_blocks.into_iter().skip(1).collect();
        
        // Spawn parallel threads for additional branches FIRST
        let s_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let s_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        
        for (branch_index, next_idx) in parallel_branches.iter().enumerate() {
            let result_clone = result_text.clone();
            let blocks_clone = blocks.clone();
            let conns_clone = connections.clone();
            let config_clone = config.clone();
            let cancel_clone = cancel_token.clone();
            let parent_clone = next_parent.clone();
            let preset_id_clone = preset_id.clone();
            let next_idx_copy = *next_idx;
            
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
                    RefineContext::None,
                    false,
                    None, // No processing indicator for parallel branches
                    cancel_clone,
                    preset_id_clone
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
            RefineContext::None,
            false,
            processing_indicator_hwnd, // Pass it along (might be None or Some)
            cancel_token, // Pass the same token through the chain
            preset_id
        );
    } else {
        // Chain stopped unexpectedly (empty result or error)
        // Ensure processing overlay is closed
        if let Some(h) = processing_indicator_hwnd {
             unsafe { PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        }
    }
}

// --- WINDOW PROC FOR OVERLAY (Unchanged) ---
unsafe fn create_processing_window(rect: RECT, graphics_mode: String) -> HWND {
    let instance = GetModuleHandleW(None).unwrap();
    let class_name = w!("SGTProcessingOverlay");

    REGISTER_PROC_CLASS.call_once(|| {
        let mut wc = WNDCLASSW::default();
        wc.lpfnWndProc = Some(processing_wnd_proc);
        wc.hInstance = instance;
        wc.hCursor = LoadCursorW(None, IDC_WAIT).unwrap();
        wc.lpszClassName = class_name;
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.hbrBackground = HBRUSH(0); 
        RegisterClassW(&wc);
    });

    let w = (rect.right - rect.left).abs();
    let h = (rect.bottom - rect.top).abs();
    let pixels = (w as i64) * (h as i64);
    let timer_interval = if pixels > 5_000_000 { 50 } else if pixels > 2_000_000 { 32 } else { 16 };

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, 
        class_name, w!("Processing"), WS_POPUP, rect.left, rect.top, w, h, None, None, instance, None
    );
    let mut states = PROC_STATES.lock().unwrap();
    states.insert(hwnd.0 as isize, ProcessingState::new(graphics_mode));
    drop(states);
    SetTimer(hwnd, 1, timer_interval, None);
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    hwnd
}

unsafe extern "system" fn processing_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            let mut states = PROC_STATES.lock().unwrap();
            let state = states.entry(hwnd.0 as isize).or_insert(ProcessingState::new("standard".to_string()));
            if !state.is_fading_out {
                state.is_fading_out = true;
                if !state.timer_killed {
                    KillTimer(hwnd, 1); state.timer_killed = true;
                    SetTimer(hwnd, 2, 25, None);
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let (should_destroy, anim_offset, alpha, is_fading) = {
                let mut states = PROC_STATES.lock().unwrap();
                let state = states.entry(hwnd.0 as isize).or_insert(ProcessingState::new("standard".to_string()));
                let mut destroy_flag = false;
                if state.is_fading_out {
                    if state.alpha > 20 { state.alpha -= 20; } else { state.alpha = 0; destroy_flag = true; }
                } else {
                    state.animation_offset += 5.0; if state.animation_offset > 360.0 { state.animation_offset -= 360.0; }
                }
                (destroy_flag, state.animation_offset, state.alpha, state.is_fading_out)
            };
            if should_destroy { 
                KillTimer(hwnd, 1); KillTimer(hwnd, 2); 
                DestroyWindow(hwnd); 
                return LRESULT(0); 
            }
            
            let mut rect = RECT::default(); GetWindowRect(hwnd, &mut rect);
            let w = (rect.right - rect.left).abs(); let h = (rect.bottom - rect.top).abs();
            if w > 0 && h > 0 {
                let mut states = PROC_STATES.lock().unwrap();
                let state = states.get_mut(&(hwnd.0 as isize)).unwrap();
                let scale_factor = if w > MAX_GLOW_BUFFER_DIM || h > MAX_GLOW_BUFFER_DIM {
                    (MAX_GLOW_BUFFER_DIM as f32 / w as f32).min(MAX_GLOW_BUFFER_DIM as f32 / h as f32).min(1.0)
                } else { 1.0 };
                let buf_w = ((w as f32) * scale_factor).ceil() as i32;
                let buf_h = ((h as f32) * scale_factor).ceil() as i32;
                if state.cache_hbm.0 == 0 || state.scaled_w != buf_w || state.scaled_h != buf_h {
                    state.cleanup();
                    let screen_dc = GetDC(None);
                    let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: buf_w, biHeight: -buf_h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default() }, ..Default::default() };
                    let res = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut state.cache_bits, None, 0);
                    ReleaseDC(None, screen_dc);
                    if let Ok(hbm) = res { if !hbm.is_invalid() && !state.cache_bits.is_null() { state.cache_hbm = hbm; state.scaled_w = buf_w; state.scaled_h = buf_h; } else { return LRESULT(0); } } else { return LRESULT(0); }
                }
                if !is_fading && !state.cache_bits.is_null() {
                    if state.graphics_mode == "minimal" { crate::overlay::paint_utils::draw_minimal_glow(state.cache_bits as *mut u32, state.scaled_w, state.scaled_h, anim_offset, 1.0, true); }
                    else { crate::overlay::paint_utils::draw_direct_sdf_glow(state.cache_bits as *mut u32, state.scaled_w, state.scaled_h, anim_offset, 1.0, true); }
                }
                let screen_dc = GetDC(None);
                let needs_scaling = state.scaled_w != w || state.scaled_h != h;
                let (final_hbm, final_w, final_h) = if needs_scaling {
                    let scaled_dc = CreateCompatibleDC(screen_dc); SelectObject(scaled_dc, state.cache_hbm);
                    let dest_bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default() }, ..Default::default() };
                    let mut dest_bits: *mut core::ffi::c_void = std::ptr::null_mut();
                    let dest_hbm = CreateDIBSection(screen_dc, &dest_bmi, DIB_RGB_COLORS, &mut dest_bits, None, 0);
                    if let Ok(hbm) = dest_hbm {
                        if !hbm.is_invalid() {
                            let dest_dc = CreateCompatibleDC(screen_dc); SelectObject(dest_dc, hbm);
                            SetStretchBltMode(dest_dc, HALFTONE); StretchBlt(dest_dc, 0, 0, w, h, scaled_dc, 0, 0, state.scaled_w, state.scaled_h, SRCCOPY);
                            DeleteDC(scaled_dc); (Some((dest_dc, hbm)), w, h)
                        } else { DeleteDC(scaled_dc); (None, state.scaled_w, state.scaled_h) }
                    } else { DeleteDC(scaled_dc); (None, state.scaled_w, state.scaled_h) }
                } else { (None, w, h) };
                
                let (mem_dc, old_hbm, temp_res) = if let Some((dc, hbm)) = final_hbm { (dc, HGDIOBJ::default(), Some(hbm)) } else { let dc = CreateCompatibleDC(screen_dc); let old = SelectObject(dc, state.cache_hbm); (dc, old, None) };
                let pt_src = POINT { x: 0, y: 0 }; let size = SIZE { cx: final_w, cy: final_h };
                let mut blend = BLENDFUNCTION::default(); blend.BlendOp = AC_SRC_OVER as u8; blend.SourceConstantAlpha = alpha; blend.AlphaFormat = AC_SRC_ALPHA as u8;
                UpdateLayeredWindow(hwnd, None, None, Some(&size), mem_dc, Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA);
                
                if temp_res.is_some() { DeleteDC(mem_dc); if let Some(hbm) = temp_res { DeleteObject(hbm); } } else { SelectObject(mem_dc, old_hbm); DeleteDC(mem_dc); }
                ReleaseDC(None, screen_dc);
            }
            LRESULT(0)
        }
        WM_PAINT => { let mut ps = PAINTSTRUCT::default(); BeginPaint(hwnd, &mut ps); EndPaint(hwnd, &mut ps); LRESULT(0) }
        WM_DESTROY => { let mut states = PROC_STATES.lock().unwrap(); if let Some(mut state) = states.remove(&(hwnd.0 as isize)) { state.cleanup(); } LRESULT(0) }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
