use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use std::sync::{Arc, Mutex};
use image::GenericImageView;

use crate::{AppState, api::{translate_image_streaming, translate_text_streaming}};
use super::utils::{copy_to_clipboard, get_error_message};
use super::result::{create_result_window, update_window_text, WindowType, link_windows};

pub fn process_and_close(app: Arc<Mutex<AppState>>, rect: RECT, overlay_hwnd: HWND, preset_idx: usize) {
    // 1. Snapshot and Configuration Retrieval
    let (img, config, preset) = {
        let guard = app.lock().unwrap();
        if preset_idx >= guard.config.presets.len() {
            // Should not happen, but safety check
            unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            return;
        }
        (
            guard.original_screenshot.clone().unwrap(), 
            guard.config.clone(),
            guard.config.presets[preset_idx].clone()
        )
    };

    let model_id = &preset.model;
    let model_config = crate::model_config::get_model_by_id(model_id);
    let model_config = model_config.expect("Model config not found for preset model");
    let model_name = model_config.full_name.clone();
    let provider = model_config.provider.clone();

    let x_virt = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y_virt = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    
    let crop_x = (rect.left - x_virt).max(0) as u32;
    let crop_y = (rect.top - y_virt).max(0) as u32;
    let crop_w = (rect.right - rect.left).abs() as u32;
    let crop_h = (rect.bottom - rect.top).abs() as u32;
    
    let img_w = img.width();
    let img_h = img.height();
    let crop_w = crop_w.min(img_w.saturating_sub(crop_x));
    let crop_h = crop_h.min(img_h.saturating_sub(crop_y));

    if crop_w > 0 && crop_h > 0 {
        let cropped = img.view(crop_x, crop_y, crop_w, crop_h).to_image();
        
        let groq_api_key = config.api_key.clone();
        let gemini_api_key = config.gemini_api_key.clone();
        let ui_language = config.ui_language.clone();
        
        // Prepare Prompt - replace all {languageN} with actual languages
        let mut final_prompt = preset.prompt.clone();
        
        // Replace numbered language tags
        for (key, value) in &preset.language_vars {
            let pattern = format!("{{{}}}", key); // e.g., "{language1}"
            final_prompt = final_prompt.replace(&pattern, value);
        }
        
        // Backward compatibility: also replace old {language} tag
        final_prompt = final_prompt.replace("{language}", &preset.selected_language);
        
        // Settings for thread
        let streaming_enabled = preset.streaming_enabled;
        let retranslate_streaming_enabled = preset.retranslate_streaming_enabled;
        let auto_copy = preset.auto_copy;
        let retranslate_auto_copy = preset.retranslate_auto_copy;
        let do_retranslate = preset.retranslate;
        let retranslate_to = preset.retranslate_to.clone();
        let retranslate_model_id = preset.retranslate_model.clone();
        let use_json_format = preset.id == "preset_translate";
        let hide_overlay = preset.hide_overlay;
        
        // Spawn UI Thread for Results
        std::thread::spawn(move || {
            // Create Primary Window (Hidden initially)
            let primary_hwnd = create_result_window(rect, WindowType::Primary);
            
            // Worker thread for API calls
            std::thread::spawn(move || {
                let accumulated_vision = Arc::new(Mutex::new(String::new()));
                let acc_vis_clone = accumulated_vision.clone();
                let mut first_chunk_received = false;

                // --- STEP 1: VISION API ---
                let vision_res = translate_image_streaming(
                    &groq_api_key, 
                    &gemini_api_key, 
                    final_prompt, 
                    model_name, 
                    provider, 
                    cropped, 
                    streaming_enabled, 
                    use_json_format,
                    |chunk| {
                        let mut text = acc_vis_clone.lock().unwrap();
                        text.push_str(chunk);
                        
                        if !first_chunk_received {
                            first_chunk_received = true;
                            unsafe {
                                PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                                if !hide_overlay {
                                    ShowWindow(primary_hwnd, SW_SHOW);
                                }
                            }
                        }
                        if !hide_overlay {
                            update_window_text(primary_hwnd, &text);
                        }
                    }
                );

                match vision_res {
                    Ok(vision_text) => {
                        // Ensure window is shown if it wasn't already (non-streaming or fast response)
                        if !first_chunk_received {
                             unsafe {
                                PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                                if !hide_overlay {
                                    ShowWindow(primary_hwnd, SW_SHOW);
                                }
                            }
                            if !hide_overlay {
                                update_window_text(primary_hwnd, &vision_text);
                            }
                        }

                        // --- STEP 1.5: MAIN AUTO COPY ---
                        if auto_copy && !vision_text.trim().is_empty() {
                            let vt = vision_text.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                copy_to_clipboard(&vt, HWND(0));
                            });
                        }

                        // --- STEP 2: RETRANSLATE (Optional) ---
                        if do_retranslate && !vision_text.trim().is_empty() {
                            // Create Secondary Window
                            // We need to do this on the UI thread? No, create_result_window handles it?
                            // Actually create_result_window creates a window on the CURRENT thread.
                            // The current thread is this worker thread? 
                            // NO. `create_result_window` creates a window. Windows must be pumped on the thread they are created.
                            // This worker thread DOES NOT pump messages. The PARENT thread (spawned above) pumps messages for `primary_hwnd`.
                            // So `primary_hwnd` was created on the parent thread.
                            // If we want a secondary window, it ALSO needs to be created on the parent thread to share the message loop.
                            // Solution: We cannot easily create the secondary window from THIS worker thread if we want the parent loop to handle it.
                            // However, we can use `PostMessage` to signal the parent thread to create it? 
                            // Or, simplified: Just spawn a NEW thread/loop for the secondary window?
                            // Yes, spawning a new thread for the secondary window is easiest and isolates it.
                            
                            let vision_text_for_retrans = vision_text.clone();
                            let groq_key_for_retrans = groq_api_key.clone();
                            
                            // Spawn Secondary UI Thread
                            std::thread::spawn(move || {
                                let secondary_hwnd = create_result_window(rect, WindowType::Secondary);
                                super::result::link_windows(primary_hwnd, secondary_hwnd);
                                if !hide_overlay {
                                    unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                                    update_window_text(secondary_hwnd, "");
                                }

                                // API Call for Retranslation (Blocking in this UI thread? No, need another worker or just block since it's simple text?)
                                // Better to block here? If we block, the window won't repaint.
                                // So spawn a worker for text API too.
                                
                                std::thread::spawn(move || {
                                    let acc_text = Arc::new(Mutex::new(String::new()));
                                    let acc_text_clone = acc_text.clone();
                                    
                                    // Resolve text model
                                    let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                                    let tm_name = tm_config.map(|m| m.full_name).expect("Retranslate model not found");

                                    let text_res = translate_text_streaming(
                                        &groq_key_for_retrans,
                                        vision_text_for_retrans,
                                        retranslate_to,
                                        tm_name,
                                        retranslate_streaming_enabled,
                                        false, // Disable JSON format for text retranslation to avoid parsing errors
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

                                // Message Loop for Secondary
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
                    }
                    Err(e) => {
                        if !first_chunk_received {
                            unsafe {
                                PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                                ShowWindow(primary_hwnd, SW_SHOW);
                            }
                        }
                        let error_msg = get_error_message(&e.to_string(), &ui_language);
                        update_window_text(primary_hwnd, &error_msg);
                    }
                }
            });

            // Message Loop for Primary
            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    if !IsWindow(primary_hwnd).as_bool() { break; }
                }
            }
        });

    } else {
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
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
    
    std::thread::spawn(move || {
        let primary_hwnd = create_result_window(rect, WindowType::Primary);
        if !hide_overlay {
            unsafe { ShowWindow(primary_hwnd, SW_SHOW); }
            update_window_text(primary_hwnd, &text);
        }
        
        if auto_copy {
            copy_to_clipboard(&text, HWND(0));
        }

        if retranslate && !text.trim().is_empty() {
            let rect_sec = retrans_rect.unwrap();
            let text_for_retrans = text.clone();
            let groq_key = {
                let app = crate::APP.lock().unwrap();
                app.config.api_key.clone()
            };
            
            std::thread::spawn(move || {
                let secondary_hwnd = create_result_window(rect_sec, WindowType::Secondary);
                link_windows(primary_hwnd, secondary_hwnd);
                
                if !hide_overlay {
                    unsafe { ShowWindow(secondary_hwnd, SW_SHOW); }
                    update_window_text(secondary_hwnd, "");
                }

                // API Call for Retranslation
                std::thread::spawn(move || {
                    let acc_text = Arc::new(Mutex::new(String::new()));
                    let acc_text_clone = acc_text.clone();
                    
                    let tm_config = crate::model_config::get_model_by_id(&retranslate_model_id);
                    let tm_name = tm_config.map(|m| m.full_name).unwrap_or_else(|| "openai/gpt-oss-20b".to_string());

                    let text_res = translate_text_streaming(
                        &groq_key,
                        text_for_retrans,
                        retranslate_to,
                        tm_name,
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

                // Message Loop for Secondary
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
