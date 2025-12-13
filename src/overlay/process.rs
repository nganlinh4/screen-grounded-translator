use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, Mutex, Once};
use std::collections::HashMap;
use image::{ImageBuffer, Rgba};

use crate::api::{translate_image_streaming, translate_text_streaming};
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
}

// --- ENTRY POINTS ---

pub fn start_text_processing(
    initial_text_content: String,
    screen_rect: RECT,
    config: Config,
    preset: Preset,
    hotkey_name: String
) {
    if preset.text_input_mode == "type" {
        // Use blocks[0].prompt instead of legacy preset.prompt
        let first_block_prompt = preset.blocks.first().map(|b| b.prompt.as_str()).unwrap_or("");
        let guide_text = if first_block_prompt.is_empty() { 
            String::new()
        } else { 
            format!("{}...", preset.name) 
        };

        let config_shared = Arc::new(config.clone());
        let preset_shared = Arc::new(preset.clone());
        let ui_lang = config.ui_language.clone();
        
        text_input::show(guide_text, ui_lang, hotkey_name, move |user_text, input_hwnd| {
            unsafe { PostMessageW(input_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            
            // For Text Preset, Block 0 is the text input processing block
            execute_chain_pipeline(user_text, screen_rect, (*config_shared).clone(), (*preset_shared).clone(), RefineContext::None);
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
) {
    let config = {
        let app = crate::APP.lock().unwrap();
        app.config.clone()
    };
    
    // Audio processing already completed Block 0 (audio recording/transcription).
    // Start at block 0 with skip_execution=true so it can display its overlay (if configured),
    // then the chain naturally continues to block 1, 2, etc.
    run_chain_step(
        0, 
        transcription_text,
        rect, 
        preset.blocks.clone(), 
        config, 
        Arc::new(Mutex::new(None)),
        RefineContext::None,
        true, // skip_execution: audio already done, just display and chain forward
        None
    );
}

pub fn start_processing_pipeline(
    cropped_img: ImageBuffer<Rgba<u8>, Vec<u8>>, 
    screen_rect: RECT, 
    config: Config, 
    preset: Preset
) {
    // If dynamic prompt mode, handle separately (needs immediate window, not processing overlay)
    if preset.prompt_mode == "dynamic" && !preset.blocks.is_empty() {
        // For dynamic mode, we still need to encode PNG first (user will type prompt)
        let mut png_data = Vec::new();
        let _ = cropped_img.write_to(&mut std::io::Cursor::new(&mut png_data), image::ImageFormat::Png);
        let context = RefineContext::Image(png_data);
        
        let block0 = preset.blocks[0].clone();
        let model_id = block0.model.clone();
        let prov = crate::model_config::get_model_by_id(&model_id).map(|m| m.provider).unwrap_or("groq".to_string());
        
        std::thread::spawn(move || {
            let hwnd = create_result_window(
                screen_rect, 
                WindowType::Primary, 
                context, 
                model_id, 
                prov, 
                block0.streaming_enabled, 
                true, // start_editing
                block0.prompt.clone(), 
                None, 
                get_chain_color(0)
            );
            unsafe { ShowWindow(hwnd, SW_SHOW); }
            unsafe { let mut m = MSG::default(); while GetMessageW(&mut m, None, 0, 0).into() { TranslateMessage(&m); DispatchMessageW(&m); if !IsWindow(hwnd).as_bool() { break; } } }
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
    
    std::thread::spawn(move || {
        // Heavy work: PNG encoding happens here, while animation plays
        let mut png_data = Vec::new();
        let _ = cropped_img.write_to(&mut std::io::Cursor::new(&mut png_data), image::ImageFormat::Png);
        let context = RefineContext::Image(png_data);
        
        // Start chain execution with the pre-created processing window
        run_chain_step(
            0, 
            String::new(), 
            screen_rect, 
            blocks, 
            conf_clone, 
            Arc::new(Mutex::new(None)), 
            context, 
            false,
            Some(processing_hwnd) // Pass the handle to be closed later
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
    
    std::thread::spawn(move || {
        run_chain_step(
            0, 
            initial_input, 
            rect, 
            blocks, 
            conf_clone, 
            Arc::new(Mutex::new(None)), 
            context, 
            false,
            Some(processing_hwnd) // Pass the handle to be closed later
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

/// Recursive step to run a block in the chain
fn run_chain_step(
    block_idx: usize,
    input_text: String,
    current_rect: RECT,
    blocks: Vec<ProcessingBlock>,
    config: Config,
    parent_hwnd: Arc<Mutex<Option<HWND>>>,
    context: RefineContext, // Passed to Block 0 (Image context)
    skip_execution: bool,   // If true, we just display result
    mut processing_indicator_hwnd: Option<HWND>, // Handle to the "Processing..." overlay
) {
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
    
    let my_rect = if visible_count_before == 0 {
        current_rect
    } else {
        let s_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let s_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        calculate_next_window_rect(current_rect, s_w, s_h)
    };

    let mut my_hwnd: Option<HWND> = None;

    // 3. Create Window (if visible)
    if block.show_overlay {
        let ctx_clone = if block_idx == 0 { context.clone() } else { RefineContext::None }; 
        let m_id = model_id.clone();
        let prov = provider.clone();
        let prompt_c = final_prompt.clone();
        let stream_en = block.streaming_enabled;
        
        let parent_clone = parent_hwnd.clone();
        let (tx_hwnd, rx_hwnd) = std::sync::mpsc::channel();
        
        // For image blocks, we defer showing the window until first data arrives
        let is_image_block = block.block_type == "image";
        
        std::thread::spawn(move || {
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
                bg_color
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
        
        // Show loading state in the new window
        // For TEXT blocks: use the refining rainbow edge animation
        // For IMAGE blocks: keep using the gradient glow/laser processing window
        if !skip_execution && my_hwnd.is_some() {
            if block.block_type != "image" {
                // Text block: use rainbow edge refining animation
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(my_hwnd.unwrap().0 as isize)) { st.is_refining = true; }
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
        
        let accumulated = Arc::new(Mutex::new(String::new()));
        let acc_clone = accumulated.clone();
        
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
                    block.streaming_enabled, use_json, 
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
            translate_text_streaming(
                &groq_key, &gemini_key, input_text, block.selected_language.clone(), 
                model_full_name, provider, block.streaming_enabled, false,
                |chunk| {
                    let mut t = acc_clone.lock().unwrap(); t.push_str(chunk);
                    if let Some(h) = my_hwnd { 
                        {
                            let mut s = WINDOW_STATES.lock().unwrap();
                            if let Some(st) = s.get_mut(&(h.0 as isize)) { st.is_refining = false; }
                        }
                        update_window_text(h, &t); 
                    }
                }
            )
        };

        if let Some(h) = my_hwnd {
             let mut s = WINDOW_STATES.lock().unwrap();
             if let Some(st) = s.get_mut(&(h.0 as isize)) { st.is_refining = false; }
        }

        match res {
            Ok(txt) => {
                if let Some(h) = my_hwnd { update_window_text(h, &txt); }
                txt
            },
            Err(e) => {
                let err = format!("Error: {}", e);
                if let Some(h) = my_hwnd { update_window_text(h, &err); }
                String::new()
            }
        }
    };

    // 5. Post-Processing (Copy)
    if block.auto_copy && !result_text.trim().is_empty() {
        let txt_c = result_text.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            
            // Get auto_paste_newline setting from active preset
            let (should_add_newline, should_paste, target_window) = {
                let app = crate::APP.lock().unwrap();
                let active_idx = app.config.active_preset_idx;
                if active_idx < app.config.presets.len() {
                    let preset = &app.config.presets[active_idx];
                    (preset.auto_paste_newline, preset.auto_paste, app.last_active_window)
                } else {
                    (false, false, app.last_active_window)
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
                 if let Some(target) = target_window {
                     crate::overlay::utils::force_focus_and_paste(target);
                 }
            }
        });
    }

    // 6. Chain Next Step
    if !result_text.trim().is_empty() {
        let next_parent = if my_hwnd.is_some() {
            Arc::new(Mutex::new(my_hwnd))
        } else {
            parent_hwnd 
        };
        
        // If current window was hidden, next window should probably try to take the same rect
        // unless calculate_next_window_rect handles "previous rect was hidden" logic?
        // Actually, if visible_count_before is used, the layout logic handles position based on VISIBLE windows.
        // So we can pass current_rect or my_rect, layout module decides based on visibility count.
        
        let next_rect = if my_hwnd.is_some() { my_rect } else { current_rect };
        
        run_chain_step(
            block_idx + 1, 
            result_text, 
            next_rect, 
            blocks, 
            config, 
            next_parent, 
            RefineContext::None,
            false,
            processing_indicator_hwnd // Pass it along (might be None or Some)
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

    let hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT, class_name, w!("Processing"), WS_POPUP, rect.left, rect.top, w, h, None, None, instance, None);
    let mut states = PROC_STATES.lock().unwrap();
    states.insert(hwnd.0 as isize, ProcessingState::new(graphics_mode));
    drop(states);
    SetTimer(hwnd, 1, timer_interval, None);
    ShowWindow(hwnd, SW_SHOW);
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
