//! Window procedures for realtime overlay windows

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use std::sync::atomic::Ordering;
use wry::Rect;
use crate::api::realtime_audio::{WM_REALTIME_UPDATE, WM_TRANSLATION_UPDATE, WM_VOLUME_UPDATE, WM_MODEL_SWITCH, REALTIME_RMS};
use super::state::*;
use super::webview::update_webview_text;
pub unsafe extern "system" fn realtime_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_REALTIME_UPDATE => {
            // Get old (committed) and new (current sentence) text from state
            let (old_text, new_text) = {
                if let Ok(state) = REALTIME_STATE.lock() {
                    // Everything before last_committed_pos is "old"
                    // Everything after is "new" (current sentence)
                    let full = &state.full_transcript;
                    let pos = state.last_committed_pos.min(full.len());
                    let old_raw = &full[..pos];
                    let new_raw = &full[pos..];
                    
                    let old = old_raw.trim_end();
                    let new = new_raw.trim_start();
                    if !old.is_empty() && !new.is_empty() {
                        (old.to_string(), format!(" {}", new))
                    } else {
                        (old.to_string(), new.to_string())
                    }
                } else {
                    (String::new(), String::new())
                }
            };
            update_webview_text(hwnd, &old_text, &new_text);
            LRESULT(0)
        }
        WM_VOLUME_UPDATE => {
            // Read RMS from shared atomic and update visualizer
            let rms_bits = REALTIME_RMS.load(Ordering::Relaxed);
            let rms = f32::from_bits(rms_bits);
            
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.updateVolume) window.updateVolume({});", rms);
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_UPDATE_TTS_SPEED => {
            let speed = wparam.0 as u32;
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.updateTtsSpeed) window.updateTtsSpeed({});", speed);
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_SIZE => {
            // Resize WebView to match window size
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            let hwnd_key = hwnd.0 as isize;
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width, height)),
                    });
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
            DestroyWindow(hwnd);
            
            if !TRANSLATION_HWND.is_invalid() {
                DestroyWindow(TRANSLATION_HWND);
            }
            
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub unsafe extern "system" fn translation_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRANSLATION_UPDATE => {
            // Get old (committed) and new (uncommitted) translation from state
            let (old_text, new_text) = {
                if let Ok(state) = REALTIME_STATE.lock() {
                    let old = state.committed_translation.trim_end();
                    let new = state.uncommitted_translation.trim_start();
                    if !old.is_empty() && !new.is_empty() {
                        (old.to_string(), format!(" {}", new))
                    } else {
                        (old.to_string(), new.to_string())
                    }
                } else {
                    (String::new(), String::new())
                }
            };
            
            
            // TTS: Check if we have new committed text to speak
            // For Mic mode: TTS always works (no feedback loop concern)
            // For Device mode: Only speak if an app is selected (per-app capture isolates TTS from loopback)
            let app_selected = SELECTED_APP_PID.load(Ordering::SeqCst) > 0;
            let is_mic_mode = NEW_AUDIO_SOURCE.lock().map(|s| s.is_empty() || s.as_str() == "mic").unwrap_or(true);
            let tts_allowed = is_mic_mode || app_selected;
            if REALTIME_TTS_ENABLED.load(Ordering::SeqCst) && tts_allowed && !old_text.is_empty() {
                let old_len = old_text.len();
                
                // Smart catch-up: If starting fresh (0) with existing text, skip to last sentence
                // This prevents reading the entire history when toggling TTS on
                if LAST_SPOKEN_LENGTH.load(Ordering::SeqCst) == 0 && old_len > 50 {
                    let text = old_text.trim_end();
                    // Ignore the very last char if it's punctuation, to find the PREVIOUS sentence boundary
                    let search_limit = text.len().saturating_sub(1);
                    if search_limit > 0 {
                        // Find last sentence terminator (. ? ! or newline)
                        let last_boundary = text[..search_limit].rfind(|c| c == '.' || c == '?' || c == '!' || c == '\n');
                        
                        if let Some(idx) = last_boundary {
                            // Mark everything up to (and including) this punctuation as "spoken"
                            // So we only read what follows
                            LAST_SPOKEN_LENGTH.store(idx + 1, Ordering::SeqCst);
                        }
                    }
                }

                let last_spoken = LAST_SPOKEN_LENGTH.load(Ordering::SeqCst);
                
                if old_len > last_spoken {
                    // We have new committed text since last spoken
                    let new_committed = old_text[last_spoken..].to_string();
                    
                    // Only queue non-empty, non-whitespace segments
                    if !new_committed.trim().is_empty() {
                        // Queue this text for TTS
                        if let Ok(mut queue) = COMMITTED_TRANSLATION_QUEUE.lock() {
                            queue.push_back(new_committed.clone());
                        }
                        
                        // Speak using TTS manager (non-blocking)
                        // This uses the existing parallel TTS infrastructure
                        let hwnd_val = hwnd.0 as isize;
                        std::thread::spawn(move || {
                            crate::api::tts::TTS_MANAGER.speak_realtime(&new_committed, hwnd_val);
                        });
                    }
                    
                    LAST_SPOKEN_LENGTH.store(old_len, Ordering::SeqCst);
                }
            }
            
            update_webview_text(hwnd, &old_text, &new_text);
            LRESULT(0)
        }
        WM_MODEL_SWITCH => {
            // Animate the model switch in the UI
            // WPARAM: 0 = groq-llama, 1 = google-gemma, 2 = google-gtx
            let model_name = match wparam.0 {
                1 => "google-gemma",
                2 => "google-gtx",
                _ => "groq-llama"
            };
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.switchModel) window.switchModel('{}');", model_name);
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_UPDATE_TTS_SPEED => {
            let speed = wparam.0 as u32;
            let hwnd_key = hwnd.0 as isize;
            let script = format!("if(window.updateTtsSpeed) window.updateTtsSpeed({});", speed);
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_SIZE => {
            // Resize WebView to match window size
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            let hwnd_key = hwnd.0 as isize;
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width, height)),
                    });
                }
            });
            LRESULT(0)
        }
        WM_CLOSE_TTS_MODAL => {
            // Close the TTS settings modal in the WebView
            let hwnd_key = hwnd.0 as isize;
            let script = "if(document.getElementById('tts-modal')) { document.getElementById('tts-modal').classList.remove('show'); document.getElementById('tts-modal-overlay').classList.remove('show'); }";
            
            REALTIME_WEBVIEWS.with(|wvs| {
                if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                    let _ = webview.evaluate_script(script);
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            // Stop TTS when translation window is closed
            crate::api::tts::TTS_MANAGER.stop();
            
            REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
            DestroyWindow(hwnd);
            
            if !REALTIME_HWND.is_invalid() {
                DestroyWindow(REALTIME_HWND);
            }
            
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
