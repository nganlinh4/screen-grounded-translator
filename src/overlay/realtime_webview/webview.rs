//! WebView creation and IPC handling for realtime overlay

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use std::sync::atomic::Ordering;
use wry::{WebViewBuilder, Rect};
use crate::APP;
use crate::gui::locale::LocaleText;
use crate::config::get_all_languages;
use crate::api::realtime_audio::{WM_REALTIME_UPDATE, WM_TRANSLATION_UPDATE};
use super::state::*;
use crate::overlay::realtime_html::get_realtime_html;
pub fn create_realtime_webview(hwnd: HWND, is_translation: bool, audio_source: &str, current_language: &str, translation_model: &str, font_size: u32) {
    let hwnd_key = hwnd.0 as isize;
    
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect); }
    
    // Use full language list from isolang crate
    let languages = get_all_languages();
    
    // Fetch locale text
    let locale_text = {
        let app = APP.lock().unwrap();
        let lang = app.config.ui_language.clone();
        LocaleText::get(&lang)
    };
    
    let html = get_realtime_html(is_translation, audio_source, &languages, current_language, translation_model, font_size, &locale_text);
    let wrapper = HwndWrapper(hwnd);
    
    // Capture hwnd for the IPC handler closure
    let hwnd_for_ipc = hwnd;
    
    let result = WebViewBuilder::new()
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32
            )),
        })
        .with_html(&html)
        .with_transparent(false)
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            let body = msg.body();
            if body == "startDrag" {
                // Initiate window drag
                unsafe {
                    let _ = ReleaseCapture();
                    SendMessageW(
                        hwnd_for_ipc,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION as usize)),
                        Some(LPARAM(0))
                    );
                }
            } else if body == "startGroupDrag" {
                // Start group drag - nothing special needed, just mark drag started
                // The actual movement is handled by groupDragMove
            } else if body.starts_with("groupDragMove:") {
                // Move both windows together by delta
                let coords = &body[14..];
                if let Some((dx_str, dy_str)) = coords.split_once(',') {
                    if let (Ok(dx), Ok(dy)) = (dx_str.parse::<i32>(), dy_str.parse::<i32>()) {
                        unsafe {
                            // Move realtime window
                            if !REALTIME_HWND.is_invalid() {
                                let mut rect = RECT::default();
                                GetWindowRect(REALTIME_HWND, &mut rect);
                                SetWindowPos(
                                    REALTIME_HWND,
                                    None,
                                    rect.left + dx,
                                    rect.top + dy,
                                    0, 0,
                                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
                                );
                            }
                            
                            // Move translation window
                            if !TRANSLATION_HWND.is_invalid() {
                                let mut rect = RECT::default();
                                GetWindowRect(TRANSLATION_HWND, &mut rect);
                                SetWindowPos(
                                    TRANSLATION_HWND,
                                    None,
                                    rect.left + dx,
                                    rect.top + dy,
                                    0, 0,
                                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
                                );
                            }
                        }
                    }
                }
            } else if body.starts_with("copyText:") {
                // Copy text to clipboard
                let text = &body[9..];
                crate::overlay::utils::copy_to_clipboard(text, hwnd_for_ipc);
            } else if body == "close" {
                unsafe {
                    let _ = PostMessageW(
                        Some(hwnd_for_ipc),
                        WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0)
                    );
                }
            } else if body == "saveResize" {
                unsafe {
                    let mut rect = RECT::default();
                    GetWindowRect(hwnd_for_ipc, &mut rect);
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;
                    
                    let mut app = APP.lock().unwrap();
                    if hwnd_for_ipc == REALTIME_HWND {
                        app.config.realtime_transcription_size = (w, h);
                    } else {
                        app.config.realtime_translation_size = (w, h);
                    }
                    crate::config::save_config(&app.config);
                }
            } else if body.starts_with("fontSize:") {
                // Font size change - store for future use
                if let Ok(size) = body[9..].parse::<u32>() {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_font_size = size;
                    crate::config::save_config(&app.config);
                }
            } else if body.starts_with("audioSource:") {
                // Audio source change
                let source = body[12..].to_string();
                if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
                    *new_source = source.clone();
                }
                
                if source == "mic" {
                    // Clear app selection when switching to mic
                    SELECTED_APP_PID.store(0, Ordering::SeqCst);
                    if let Ok(mut name) = SELECTED_APP_NAME.lock() {
                        name.clear();
                    }
                } else if source == "device" {
                    // Check if TTS is enabled - if so, show app selection popup
                    let tts_enabled = REALTIME_TTS_ENABLED.load(Ordering::SeqCst);
                    if tts_enabled {
                        // Show app selection popup for user to choose which app to capture
                        show_app_selection_popup();
                    } else {
                        // TTS is off, use normal device loopback (clear any app selection)
                        SELECTED_APP_PID.store(0, Ordering::SeqCst);
                        if let Ok(mut name) = SELECTED_APP_NAME.lock() {
                            name.clear();
                        }
                    }
                }
                
                // Save to config
                {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_audio_source = source;
                    crate::config::save_config(&app.config);
                }
                AUDIO_SOURCE_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("language:") {
                // Target language change - signal update
                let lang = body[9..].to_string();
                if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
                    *new_lang = lang.clone();
                }
                
                // Save to config
                {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_target_language = lang;
                    crate::config::save_config(&app.config);
                }
                LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("translationModel:") {
                // Translation model change - signal update
                let model = body[17..].to_string();
                if let Ok(mut new_model) = NEW_TRANSLATION_MODEL.lock() {
                    *new_model = model.clone();
                }
                
                // Save to config
                {
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_translation_model = model;
                    crate::config::save_config(&app.config);
                }
                TRANSLATION_MODEL_CHANGE.store(true, Ordering::SeqCst);
            } else if body.starts_with("resize:") {
                // Resize window by delta
                let coords = &body[7..];
                if let Some((dx_str, dy_str)) = coords.split_once(',') {
                    if let (Ok(dx), Ok(dy)) = (dx_str.parse::<i32>(), dy_str.parse::<i32>()) {
                        unsafe {
                            let mut rect = RECT::default();
                            GetWindowRect(hwnd_for_ipc, &mut rect);
                            let new_width = (rect.right - rect.left + dx).max(200);
                            let new_height = (rect.bottom - rect.top + dy).max(100);
                            SetWindowPos(
                                hwnd_for_ipc,
                                None,
                                rect.left,
                                rect.top,
                                new_width,
                                new_height,
                                SWP_NOZORDER | SWP_NOACTIVATE
                            );
                        }
                    }
                }
            } else if body.starts_with("toggleMic:") {
                // Toggle transcription window visibility
                let visible = &body[10..] == "1";
                MIC_VISIBLE.store(visible, Ordering::SeqCst);
                unsafe {
                    if !REALTIME_HWND.is_invalid() {
                        ShowWindow(REALTIME_HWND, if visible { SW_SHOW } else { SW_HIDE });
                    }
                    // Sync to other webview
                    sync_visibility_to_webviews();
                    
                    // If both windows are now off, completely stop everything
                    if !MIC_VISIBLE.load(Ordering::SeqCst) && !TRANS_VISIBLE.load(Ordering::SeqCst) {
                        REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
                        PostQuitMessage(0);
                    } else if visible {
                        // Force update since we suppressed them while hidden
                        let _ = PostMessageW(Some(REALTIME_HWND), WM_REALTIME_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            } else if body.starts_with("toggleTrans:") {
                // Toggle translation window visibility
                let visible = &body[12..] == "1";
                TRANS_VISIBLE.store(visible, Ordering::SeqCst);
                
                // Stop TTS when translation window is hidden
                if !visible {
                    crate::api::tts::TTS_MANAGER.stop();
                }
                
                unsafe {
                    if !TRANSLATION_HWND.is_invalid() {
                        ShowWindow(TRANSLATION_HWND, if visible { SW_SHOW } else { SW_HIDE });
                    }
                    // Sync to other webview
                    sync_visibility_to_webviews();
                    
                    // If both windows are now off, completely stop everything
                    if !MIC_VISIBLE.load(Ordering::SeqCst) && !TRANS_VISIBLE.load(Ordering::SeqCst) {
                        REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);
                        PostQuitMessage(0);
                    } else if visible {
                        // Force update since we suppressed them while hidden
                        let _ = PostMessageW(Some(TRANSLATION_HWND), WM_TRANSLATION_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            } else if body.starts_with("ttsEnabled:") {
                // TTS toggle for realtime translations
                let enabled = &body[11..] == "1";
                REALTIME_TTS_ENABLED.store(enabled, Ordering::SeqCst);
                
                // Reset spoken length when disabling so we start fresh next time
                if !enabled {
                    // IMMEDIATELY stop TTS (cut off mid-sentence to prevent capture)
                    crate::api::tts::TTS_MANAGER.stop();
                    
                    // Close app selection popup if open
                    let popup_hwnd_val = APP_SELECTION_HWND.load(Ordering::SeqCst);
                    if popup_hwnd_val != 0 {
                        let popup_hwnd = windows::Win32::Foundation::HWND(popup_hwnd_val as *mut std::ffi::c_void);
                        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(popup_hwnd), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0)) };
                        APP_SELECTION_HWND.store(0, Ordering::SeqCst);
                    }
                    
                    LAST_SPOKEN_LENGTH.store(0, Ordering::SeqCst);
                    // Clear any queued translations
                    if let Ok(mut queue) = COMMITTED_TRANSLATION_QUEUE.lock() {
                        queue.clear();
                    }
                    
                    // Clear app selection (but do NOT restart audio capture - 
                    // that only happens when an app is explicitly selected)
                    SELECTED_APP_PID.store(0, Ordering::SeqCst);
                    if let Ok(mut name) = SELECTED_APP_NAME.lock() {
                        name.clear();
                    }
                } else {
                    // TTS enabled - if in device mode, show app selection popup
                    // Note: We DON'T change audio mode here - only when user selects an app
                    let current_source = {
                        let app = APP.lock().unwrap();
                        app.config.realtime_audio_source.clone()
                    };
                    if current_source == "device" {
                        // Show app selection popup (no audio change yet - happens when app is selected)
                        show_app_selection_popup();
                    }
                }
            } else if body.starts_with("ttsSpeed:") {
                // TTS playback speed adjustment (50-200, where 100 = 1.0x)
                if let Ok(speed) = body[9..].parse::<u32>() {
                    REALTIME_TTS_SPEED.store(speed, Ordering::SeqCst);
                    // Turn off auto-speed when user manually adjusts slider
                    REALTIME_TTS_AUTO_SPEED.store(false, Ordering::SeqCst);
                }
            } else if body.starts_with("ttsAutoSpeed:") {
                // TTS auto-speed toggle
                let enabled = &body[13..] == "1";
                REALTIME_TTS_AUTO_SPEED.store(enabled, Ordering::SeqCst);
            }
        })
        .build_as_child(&wrapper);
    
    if let Ok(webview) = result {
        REALTIME_WEBVIEWS.with(|wvs| {
            wvs.borrow_mut().insert(hwnd_key, webview);
        });
    }
}

pub fn destroy_realtime_webview(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;
    REALTIME_WEBVIEWS.with(|wvs| {
        wvs.borrow_mut().remove(&hwnd_key);
    });
}

/// Sync visibility toggle state to all webviews
fn sync_visibility_to_webviews() {
    let mic_vis = MIC_VISIBLE.load(Ordering::SeqCst);
    let trans_vis = TRANS_VISIBLE.load(Ordering::SeqCst);
    let script = format!("if(window.setVisibility) window.setVisibility({}, {});", mic_vis, trans_vis);
    
    REALTIME_WEBVIEWS.with(|wvs| {
        for webview in wvs.borrow().values() {
            let _ = webview.evaluate_script(&script);
        }
    });
}

pub fn update_webview_text(hwnd: HWND, old_text: &str, new_text: &str) {
    let hwnd_key = hwnd.0 as isize;
    
    // Escape the text for JavaScript
    fn escape_js(text: &str) -> String {
        text.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "")
    }
    
    let escaped_old = escape_js(old_text);
    let escaped_new = escape_js(new_text);
    
    let script = format!("window.updateText('{}', '{}');", escaped_old, escaped_new);
    
    REALTIME_WEBVIEWS.with(|wvs| {
        if let Some(webview) = wvs.borrow().get(&hwnd_key) {
            let _ = webview.evaluate_script(&script);
        }
    });
}

use super::app_selection::show_app_selection_popup;
