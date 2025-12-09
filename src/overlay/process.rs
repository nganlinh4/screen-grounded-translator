use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, Mutex, Once};
use std::collections::HashMap;
use image::{ImageBuffer, Rgba};

use crate::api::{translate_image_streaming, translate_text_streaming};
use crate::config::{Config, Preset};
use super::utils::{copy_to_clipboard, get_error_message};
use super::result::{create_result_window, update_window_text, WindowType, link_windows, RefineContext};

// --- PROCESSING WINDOW STATIC STATE ---
static REGISTER_PROC_CLASS: Once = Once::new();

struct ProcessingState {
    animation_offset: f32,
    is_fading_out: bool,
    alpha: u8,
}

lazy_static::lazy_static! {
    static ref PROC_STATES: Mutex<HashMap<isize, ProcessingState>> = Mutex::new(HashMap::new());
}

// --- MAIN ENTRY POINT FOR PROCESSING ---
pub fn start_processing_pipeline(
    cropped_img: ImageBuffer<Rgba<u8>, Vec<u8>>, 
    screen_rect: RECT, 
    config: Config, 
    preset: Preset
) {
    println!("[DEBUG] start_processing_pipeline: Initializing...");
    let hide_overlay = preset.hide_overlay;
    println!("[DEBUG] hide_overlay = {}", hide_overlay);

    // Data for Result Window
    let model_id = preset.model.clone();
    let model_config = crate::model_config::get_model_by_id(&model_id);
    let model_config = model_config.expect("Model config not found for preset model");
    let provider = model_config.provider.clone();

    // Prepare Refine Context (Image)
    let mut png_data = Vec::new();
    let _ = cropped_img.write_to(&mut std::io::Cursor::new(&mut png_data), image::ImageFormat::Png);
    let refine_context = RefineContext::Image(png_data);

    // NEW LOGIC: Dynamic Prompt Mode
    if preset.prompt_mode == "dynamic" {
        // Skip processing overlay, skip API thread. Open Result Window directly in Edit Mode.
        std::thread::spawn(move || {
            let hwnd = create_result_window(
                screen_rect,
                WindowType::Primary,
                refine_context,
                model_id,
                provider,
                preset.streaming_enabled,
                true // start_editing = true
            );
            
            unsafe { ShowWindow(hwnd, SW_SHOW); }
            
            // Run message loop
            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    if !IsWindow(hwnd).as_bool() { break; }
                }
            }
        });
        return;
    }

    // --- STANDARD PROCESSING (Fixed Prompt) ---

    // 1. Create the Processing Overlay Window (The glowing rainbow box)
    let processing_hwnd = unsafe { create_processing_window(screen_rect) };

    // 2. Prepare Data for API Thread (model_id, provider, refine_context already prepared above)
    let model_config = crate::model_config::get_model_by_id(&model_id);
    let model_config = model_config.expect("Model config not found for preset model");
    let model_name = model_config.full_name.clone();
    
    // API Config
    let groq_api_key = config.api_key.clone();
    let gemini_api_key = config.gemini_api_key.clone();
    let ui_language = config.ui_language.clone();
    
    // Prepare Prompt
    let mut final_prompt = preset.prompt.clone();
    for (key, value) in &preset.language_vars {
        final_prompt = final_prompt.replace(&format!("{{{}}}", key), value);
    }
    final_prompt = final_prompt.replace("{language}", &preset.selected_language);
    
    let streaming_enabled = preset.streaming_enabled;
    let use_json_format = preset.id == "preset_translate";
    let auto_copy = preset.auto_copy;
    let do_retranslate = preset.retranslate;
    let retranslate_to = preset.retranslate_to.clone();
    let retranslate_model_id = preset.retranslate_model.clone();
    let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
    let retranslate_auto_copy = preset.retranslate_auto_copy;
    let cropped_for_history = cropped_img.clone();

    // NEW: Capture the target window we stored in main.rs
    let target_window_for_paste = if let Ok(app) = crate::APP.lock() {
        app.last_active_window
    } else {
        None
    };
    println!("[DEBUG] Process Pipeline: Captured Target Window = {:?}", target_window_for_paste);

    // 3. Spawn API Worker Thread
    std::thread::spawn(move || {
        let accumulated_vision = Arc::new(Mutex::new(String::new()));
        let acc_vis_clone = accumulated_vision.clone();
        let mut first_chunk_received = false;
        
        let (tx_hwnd, rx_hwnd) = std::sync::mpsc::channel();

        let api_res = translate_image_streaming(
            &groq_api_key, 
            &gemini_api_key, 
            final_prompt, 
            model_name, 
            provider.clone(), 
            cropped_img, 
            streaming_enabled, 
            use_json_format,
            |chunk| {
                let mut text = acc_vis_clone.lock().unwrap();
                text.push_str(chunk);
                
                if !first_chunk_received {
                    first_chunk_received = true;
                    
                    // Signal Processing Overlay to Fade Out
                    if processing_hwnd.0 != 0 {
                        unsafe { PostMessageW(processing_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
                    }

                    // Spawn the Result Window Thread
                    let rect_copy = screen_rect;
                    let refine_ctx_copy = refine_context.clone();
                    let mid_copy = model_id.clone();
                    let prov_copy = provider.clone();
                    let stream_copy = streaming_enabled;
                    let hide_copy = hide_overlay;
                    let tx_hwnd_clone = tx_hwnd.clone();
                    
                    std::thread::spawn(move || {
                        let hwnd = create_result_window(
                            rect_copy,
                            WindowType::Primary,
                            refine_ctx_copy,
                            mid_copy,
                            prov_copy,
                            stream_copy,
                            false
                        );
                        
                        // Only show the text result if NOT hidden
                        if !hide_copy {
                            unsafe { ShowWindow(hwnd, SW_SHOW); }
                        }
                        let _ = tx_hwnd_clone.send(hwnd);
                        
                        unsafe {
                            let mut msg = MSG::default();
                            while GetMessageW(&mut msg, None, 0, 0).into() {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                                if !IsWindow(hwnd).as_bool() { break; }
                            }
                        }
                    });
                }
            }
        );

        let result_hwnd = if first_chunk_received {
            rx_hwnd.recv().ok()
        } else {
             if processing_hwnd.0 != 0 {
                unsafe { PostMessageW(processing_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            }
            
            let rect_copy = screen_rect;
            let refine_ctx_copy = refine_context.clone();
            let mid_copy = model_id.clone();
            let prov_copy = provider.clone();
            let stream_copy = streaming_enabled;
            let hide_copy = hide_overlay;
            let tx_hwnd_clone = tx_hwnd.clone();

            std::thread::spawn(move || {
                let hwnd = create_result_window(
                    rect_copy, WindowType::Primary, refine_ctx_copy, mid_copy, prov_copy, stream_copy, false
                );
                if !hide_copy { unsafe { ShowWindow(hwnd, SW_SHOW); } }
                let _ = tx_hwnd_clone.send(hwnd);
                unsafe {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).into() {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                        if !IsWindow(hwnd).as_bool() { break; }
                    }
                }
            });
            rx_hwnd.recv().ok()
        };

        if let Some(r_hwnd) = result_hwnd {
            match api_res {
                Ok(full_text) => {
                    println!("[DEBUG] API Success. Text length: {}", full_text.len());
                    if !hide_overlay { update_window_text(r_hwnd, &full_text); }
                    
                    if let Ok(app_lock) = crate::APP.lock() {
                        app_lock.history.save_image(cropped_for_history, full_text.clone());
                    }

                    // UPDATED: Logic for Auto Copy AND Auto Paste
                    if auto_copy && !full_text.trim().is_empty() {
                         let txt = full_text.clone();
                         // NEW: Check logic - only paste if overlay is hidden and we have a target
                         let should_paste = hide_overlay && target_window_for_paste.is_some();
                         let target_hwnd = target_window_for_paste;
                         
                         println!("[DEBUG] Auto-Copy triggered. should_paste={}, target_hwnd={:?}", should_paste, target_hwnd);

                         std::thread::spawn(move || {
                             std::thread::sleep(std::time::Duration::from_millis(200));
                             println!("[DEBUG] Copying to clipboard...");
                             copy_to_clipboard(&txt, HWND(0));
                             println!("[DEBUG] Text copied to clipboard. Length: {}", txt.len());
                             
                             if should_paste {
                                 if let Some(hwnd) = target_hwnd {
                                     // Ensure we paste into the correct window
                                     println!("[DEBUG] Triggering auto-paste to {:?}", hwnd);
                                     crate::overlay::utils::force_focus_and_paste(hwnd);
                                 }
                             }
                         });
                    }

                    if do_retranslate && !full_text.trim().is_empty() {
                         let text_to_retrans = full_text.clone();
                         let g_key = groq_api_key.clone();
                         let gm_key = gemini_api_key.clone();
                         
                         std::thread::spawn(move || {
                             let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                             let (tm_id, tm_name, tm_provider) = match tm_config {
                                 Some(m) => (m.id, m.full_name, m.provider),
                                 None => ("fast_text".to_string(), "openai/gpt-oss-20b".to_string(), "groq".to_string())
                             };

                             let sec_hwnd = create_result_window(
                                 screen_rect,
                                 WindowType::Secondary,
                                 RefineContext::None,
                                 tm_id,
                                 tm_provider.clone(),
                                 retranslate_streaming_enabled,
                                 false
                             );
                             link_windows(r_hwnd, sec_hwnd);
                             if !hide_overlay {
                                 unsafe { ShowWindow(sec_hwnd, SW_SHOW); }
                                 update_window_text(sec_hwnd, "");
                             }
                             
                             std::thread::spawn(move || {
                                 let acc = Arc::new(Mutex::new(String::new()));
                                 let acc_c = acc.clone();
                                 let res = translate_text_streaming(
                                     &g_key, &gm_key, text_to_retrans, retranslate_to, tm_name, tm_provider, retranslate_streaming_enabled, false,
                                     |chunk| {
                                         let mut t = acc_c.lock().unwrap();
                                         t.push_str(chunk);
                                         if !hide_overlay { update_window_text(sec_hwnd, &t); }
                                     }
                                 );
                                 if let Ok(fin) = res {
                                     if !hide_overlay { update_window_text(sec_hwnd, &fin); }
                                     if retranslate_auto_copy {
                                         std::thread::spawn(move || {
                                             std::thread::sleep(std::time::Duration::from_millis(100));
                                             copy_to_clipboard(&fin, HWND(0));
                                         });
                                     }
                                 } else if let Err(e) = res {
                                     if !hide_overlay { update_window_text(sec_hwnd, &format!("Error: {}", e)); }
                                 }
                             });

                             unsafe {
                                let mut msg = MSG::default();
                                while GetMessageW(&mut msg, None, 0, 0).into() {
                                    TranslateMessage(&msg);
                                    DispatchMessageW(&msg);
                                    if !IsWindow(sec_hwnd).as_bool() { break; }
                                }
                             }
                         });
                    }
                },
                Err(e) => {
                    let err_msg = get_error_message(&e.to_string(), &ui_language);
                    update_window_text(r_hwnd, &err_msg);
                }
            }
        }
    });

    if processing_hwnd.0 != 0 {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                // Standard loop exit conditions
                if msg.message == WM_QUIT { break; }
                if !IsWindow(processing_hwnd).as_bool() { break; }
            }
        }
    }
}


// --- PROCESSING OVERLAY WINDOW IMPLEMENTATION ---

unsafe fn create_processing_window(rect: RECT) -> HWND {
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

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
        class_name,
        w!("Processing"),
        WS_POPUP,
        rect.left, rect.top, w, h,
        None, None, instance, None
    );

    let mut states = PROC_STATES.lock().unwrap();
    states.insert(hwnd.0 as isize, ProcessingState {
        animation_offset: 0.0,
        is_fading_out: false,
        alpha: 255
    });
    drop(states);
    
    SetTimer(hwnd, 1, 16, None);
    ShowWindow(hwnd, SW_SHOW);

    hwnd
}

unsafe extern "system" fn processing_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            // When closed (via worker thread), start fade out instead of destroying immediately
            let mut states = PROC_STATES.lock().unwrap();
            let state = states.entry(hwnd.0 as isize).or_insert(ProcessingState { 
                animation_offset: 0.0,
                is_fading_out: false,
                alpha: 255
            });
            if !state.is_fading_out {
                state.is_fading_out = true;
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let (should_destroy, anim_offset, alpha) = {
                let mut states = PROC_STATES.lock().unwrap();
                let state = states.entry(hwnd.0 as isize).or_insert(ProcessingState { 
                    animation_offset: 0.0,
                    is_fading_out: false,
                    alpha: 255
                });
                
                // Handle Fade Out
                let mut destroy_flag = false;
                if state.is_fading_out {
                    if state.alpha > 30 {
                        state.alpha -= 30;
                    } else {
                        state.alpha = 0;
                        destroy_flag = true;
                    }
                }

                state.animation_offset += 5.0;
                if state.animation_offset > 360.0 { state.animation_offset -= 360.0; }
                
                (destroy_flag, state.animation_offset, state.alpha)
            };
            
            if should_destroy {
                DestroyWindow(hwnd);
                PostQuitMessage(0);
                return LRESULT(0);
            }
            
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect);
            let w = (rect.right - rect.left).abs();
            let h = (rect.bottom - rect.top).abs();

            if w > 0 && h > 0 {
                let screen_dc = GetDC(None);
                let mem_dc = CreateCompatibleDC(screen_dc);
                
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0 as u32,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                
                let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
                let hbm = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0).unwrap();
                let old_hbm = SelectObject(mem_dc, hbm);

                let local_rect = RECT { left: 0, top: 0, right: w, bottom: h };
                
                crate::overlay::paint_utils::render_box_sdf(
                    HDC(mem_dc.0),
                    local_rect,
                    w,
                    h,
                    true, 
                    anim_offset
                );

                let pt_src = POINT { x: 0, y: 0 };
                let size = SIZE { cx: w, cy: h };
                let mut blend = BLENDFUNCTION::default();
                blend.BlendOp = AC_SRC_OVER as u8;
                blend.SourceConstantAlpha = alpha; // Use dynamic alpha
                blend.AlphaFormat = AC_SRC_ALPHA as u8;

                UpdateLayeredWindow(
                    hwnd, 
                    None, 
                    None, 
                    Some(&size), 
                    mem_dc, 
                    Some(&pt_src), 
                    COLORREF(0), 
                    Some(&blend), 
                    ULW_ALPHA
                );

                SelectObject(mem_dc, old_hbm);
                DeleteObject(hbm);
                DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            let mut states = PROC_STATES.lock().unwrap();
            states.remove(&(hwnd.0 as isize));
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn show_audio_result(preset: crate::config::Preset, text: String, rect: RECT, retrans_rect: Option<RECT>) {
     let hide_overlay = preset.hide_overlay;
     let auto_copy = preset.auto_copy;
     let retranslate = preset.retranslate && retrans_rect.is_some();
     let retranslate_to = preset.retranslate_to.clone();
     let retranslate_model_id = preset.retranslate_model.clone();
     let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
     let retranslate_auto_copy = preset.retranslate_auto_copy;
     
     let model_id = preset.model.clone();
     let model_config = crate::model_config::get_model_by_id(&model_id);
     let provider = model_config.map(|m| m.provider).unwrap_or("groq".to_string());
     let streaming = preset.streaming_enabled;
     
     std::thread::spawn(move || {
         let primary_hwnd = create_result_window(
             rect,
             WindowType::Primary,
             RefineContext::None,
             model_id,
             provider,
             streaming,
             false
         );
        if !hide_overlay {
            unsafe { ShowWindow(primary_hwnd, SW_SHOW); }
            update_window_text(primary_hwnd, &text);
        }
        
        // --- AUTO PASTE LOGIC FOR AUDIO ---
        if auto_copy && !text.trim().is_empty() {
            let target_window = if let Ok(app) = crate::APP.lock() {
               app.last_active_window
            } else { None };
            println!("[DEBUG] Audio show_audio_result: Target Window = {:?}, hide_overlay = {}", target_window, hide_overlay);
            
            let txt_for_copy = text.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(200));
                println!("[DEBUG] Audio: Copying to clipboard...");
                copy_to_clipboard(&txt_for_copy, HWND(0));
                println!("[DEBUG] Audio: Text copied. Length: {}", txt_for_copy.len());
                
                // Logic: Only paste if Hide Overlay is ON and we have a target window
                if hide_overlay && target_window.is_some() {
                    if let Some(hwnd) = target_window {
                        println!("[DEBUG] Audio: Triggering auto-paste to {:?}", hwnd);
                        crate::overlay::utils::force_focus_and_paste(hwnd);
                    }
                } else {
                    println!("[DEBUG] Audio: Not pasting. hide_overlay={}, has_target={}", hide_overlay, target_window.is_some());
                }
            });
        }
        // ----------------------------------

        if retranslate && !text.trim().is_empty() {
            let rect_sec = retrans_rect.unwrap();
            let text_for_retrans = text.clone();
            let (groq_key, gemini_key) = {
                let app = crate::APP.lock().unwrap();
                (app.config.api_key.clone(), app.config.gemini_api_key.clone())
            };
            
            std::thread::spawn(move || {
                let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                let (tm_id, tm_name, tm_provider) = match tm_config {
                Some(m) => (m.id, m.full_name, m.provider),
                None => ("fast_text".to_string(), "openai/gpt-oss-20b".to_string(), "groq".to_string())
                };
                
                let secondary_hwnd = create_result_window(
                rect_sec,
                WindowType::SecondaryExplicit,
                RefineContext::None,
                tm_id,
                tm_provider.clone(),
                retranslate_streaming_enabled,
                false
                );
                link_windows(primary_hwnd, secondary_hwnd);
                
                if !hide_overlay {
                unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                update_window_text(secondary_hwnd, "");
                }

                std::thread::spawn(move || {
                let acc_text = Arc::new(Mutex::new(String::new()));
                let acc_text_clone = acc_text.clone();

                        let text_res = translate_text_streaming(
                            &groq_key,
                            &gemini_key,
                            text_for_retrans,
                            retranslate_to,
                            tm_name,
                            tm_provider,
                            retranslate_streaming_enabled,
                            false,
                            |chunk| {
                                let mut t = acc_text_clone.lock().unwrap();
                                t.push_str(chunk);
                                if !hide_overlay {
                                    update_window_text(secondary_hwnd, &t);
                                }
                            }
                        );
                        
                        if let Ok(final_text) = text_res {
                            if !hide_overlay {
                                update_window_text(secondary_hwnd, &final_text);
                            }
                            if retranslate_auto_copy {
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                    copy_to_clipboard(&final_text, HWND(0));
                                });
                            }
                        } else if let Err(e) = text_res {
                            if !hide_overlay {
                                update_window_text(secondary_hwnd, &format!("Error: {}", e));
                            }
                        }
                    });

                    unsafe {
                        let mut msg = MSG::default();
                        while GetMessageW(&mut msg, None, 0, 0).into() {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                            if !IsWindow(secondary_hwnd).as_bool() { break; }
                        }
                    }
                });
        }
        
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !IsWindow(primary_hwnd).as_bool() { break; }
            }
        }
    });
}
