//! Window procedures for realtime overlay windows

use super::state::*;
use super::webview::update_webview_text;
use crate::api::realtime_audio::{
    REALTIME_RMS, WM_CLOSE_TTS_MODAL, WM_COPY_TEXT, WM_DOWNLOAD_PROGRESS, WM_EXEC_SCRIPT,
    WM_MODEL_SWITCH, WM_REALTIME_UPDATE, WM_START_DRAG, WM_TOGGLE_MIC, WM_TOGGLE_TRANS,
    WM_TRANSLATION_UPDATE, WM_UPDATE_TTS_SPEED, WM_VOLUME_UPDATE,
};
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::Rect;
pub unsafe extern "system" fn realtime_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_START_DRAG => {
            let _ = ReleaseCapture();
            let _ = SendMessageW(
                hwnd,
                WM_NCLBUTTONDOWN,
                Some(WPARAM(HTCAPTION as usize)),
                Some(LPARAM(0)),
            );
            LRESULT(0)
        }
        WM_TOGGLE_MIC => {
            let val = wparam.0 != 0;
            MIC_VISIBLE.store(val, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_TOGGLE_TRANS => {
            let val = wparam.0 != 0;
            TRANS_VISIBLE.store(val, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_COPY_TEXT => {
            let ptr = lparam.0 as *mut String;
            if !ptr.is_null() {
                let text = Box::from_raw(ptr);
                crate::overlay::utils::copy_to_clipboard(&text, hwnd);
            }
            LRESULT(0)
        }
        WM_EXEC_SCRIPT => {
            let ptr = lparam.0 as *mut String;
            if !ptr.is_null() {
                let script_box = Box::from_raw(ptr);
                let script = *script_box;
                let hwnd_key = hwnd.0 as isize;
                REALTIME_WEBVIEWS.with(|wvs| {
                    if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                        let _ = webview.evaluate_script(&script);
                    }
                });
            }
            LRESULT(0)
        }
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
        WM_DOWNLOAD_PROGRESS => {
            let (is_downloading, title, message, progress) = {
                if let Ok(state) = REALTIME_STATE.lock() {
                    (
                        state.is_downloading,
                        state.download_title.clone(),
                        state.download_message.clone(),
                        state.download_progress,
                    )
                } else {
                    (false, String::new(), String::new(), 0.0)
                }
            };

            if is_downloading {
                let script = format!(
                    "if(window.showDownloadModal) window.showDownloadModal('{}', '{}', {});",
                    title.replace("'", "\\'"),
                    message.replace("'", "\\'"),
                    progress
                );
                let hwnd_key = hwnd.0 as isize;
                REALTIME_WEBVIEWS.with(|wvs| {
                    if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                        let _ = webview.evaluate_script(&script);
                    }
                });
            } else {
                let script = "if(window.hideDownloadModal) window.hideDownloadModal();";
                let hwnd_key = hwnd.0 as isize;
                REALTIME_WEBVIEWS.with(|wvs| {
                    if let Some(webview) = wvs.borrow().get(&hwnd_key) {
                        let _ = webview.evaluate_script(&script);
                    }
                });
            }

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
            let script = format!(
                "if(window.updateTtsSpeed) window.updateTtsSpeed({});",
                speed
            );

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
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width, height)),
                    });
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = PostMessageW(Some(hwnd), WM_APP_REALTIME_HIDE, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_APP_REALTIME_HIDE => {
            // Stop transcription and TTS
            REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
            crate::api::tts::TTS_MANAGER.stop();

            // Hide windows
            let _ = ShowWindow(hwnd, SW_HIDE);
            if !std::ptr::addr_of!(TRANSLATION_HWND).read().is_invalid() {
                let _ = ShowWindow(TRANSLATION_HWND, SW_HIDE);
            }

            // Reset active state so it can be shown again
            IS_ACTIVE = false;

            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = DestroyWindow(hwnd);
            if !std::ptr::addr_of!(TRANSLATION_HWND).read().is_invalid() {
                let _ = DestroyWindow(TRANSLATION_HWND);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub unsafe extern "system" fn translation_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRANSLATION_UPDATE => {
            // Get old (committed) and new (uncommitted) translation from state
            let (old_text, new_text): (String, String) = {
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
            let is_mic_mode = NEW_AUDIO_SOURCE
                .lock()
                .map(|s| s.is_empty() || s.as_str() == "mic")
                .unwrap_or(true);
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
                        let last_boundary = text[..search_limit]
                            .rfind(|c| c == '.' || c == '?' || c == '!' || c == '\n');

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
                _ => "groq-llama",
            };
            let hwnd_key = hwnd.0 as isize;
            let script = format!(
                "if(window.switchModel) window.switchModel('{}');",
                model_name
            );

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
            let script = format!(
                "if(window.updateTtsSpeed) window.updateTtsSpeed({});",
                speed
            );

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
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
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
            let _ = PostMessageW(
                Some(REALTIME_HWND),
                WM_APP_REALTIME_HIDE,
                WPARAM(0),
                LPARAM(0),
            );
            LRESULT(0)
        }
        WM_APP_REALTIME_HIDE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
