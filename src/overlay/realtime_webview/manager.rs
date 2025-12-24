//! Overlay lifecycle management (show/stop/check active)

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::core::w;
use std::sync::atomic::Ordering;
use crate::APP;
use crate::api::realtime_audio::{start_realtime_transcription, RealtimeState};
use super::state::*;
use super::webview::*;
use super::wndproc::*;
pub fn is_realtime_overlay_active() -> bool {
    unsafe { IS_ACTIVE && !REALTIME_HWND.is_invalid() }
}

/// Stop the realtime overlay and close all windows
pub fn stop_realtime_overlay() {
    // Stop any playing TTS immediately
    crate::api::tts::TTS_MANAGER.stop();
    
    unsafe {
        // Close app selection popup if open
        let popup_val = APP_SELECTION_HWND.load(std::sync::atomic::Ordering::SeqCst);
        if popup_val != 0 {
             let popup_hwnd = HWND(popup_val as *mut std::ffi::c_void);
             let _ = PostMessageW(Some(popup_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
             APP_SELECTION_HWND.store(0, std::sync::atomic::Ordering::SeqCst);
        }

        if !REALTIME_HWND.is_invalid() {
            let _ = PostMessageW(Some(REALTIME_HWND), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn show_realtime_overlay(preset_idx: usize) {
    unsafe {
        if IS_ACTIVE { return; }
        
        let mut preset = APP.lock().unwrap().config.presets[preset_idx].clone();
        

        
        // Reset state
        IS_ACTIVE = true;
        REALTIME_STOP_SIGNAL.store(false, Ordering::SeqCst);
        
        // Reset visibility flags
        MIC_VISIBLE.store(true, Ordering::SeqCst);
        TRANS_VISIBLE.store(true, Ordering::SeqCst);
        
        // Reset change signals
        AUDIO_SOURCE_CHANGE.store(false, Ordering::SeqCst);
        LANGUAGE_CHANGE.store(false, Ordering::SeqCst);
        TRANSLATION_MODEL_CHANGE.store(false, Ordering::SeqCst);
        
        // Reset translation state
        {
            let mut state = REALTIME_STATE.lock().unwrap();
            *state = RealtimeState::new();
        }
        
        let instance = GetModuleHandleW(None).unwrap();
        
        // --- Create Main Realtime Overlay ---
        let class_name = w!("RealtimeWebViewOverlay");
        REGISTER_REALTIME_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(realtime_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(std::ptr::null_mut());
            let _ = RegisterClassW(&wc);
        });
        
        // Fetch config
        let (font_size, config_audio_source, config_language, config_translation_model, trans_size, transcription_size) = {
            let app = APP.lock().unwrap();
            (
                app.config.realtime_font_size,
                app.config.realtime_audio_source.clone(),
                app.config.realtime_target_language.clone(),
                app.config.realtime_translation_model.clone(),
                app.config.realtime_translation_size,
                app.config.realtime_transcription_size
            )
        };
        
        // IMPORTANT: Override preset.audio_source with saved config value
        // This ensures the transcription engine uses the user's saved preference
        if !config_audio_source.is_empty() {
            preset.audio_source = config_audio_source.clone();
        }
        
    let target_language = if !config_language.is_empty() {
        config_language
    } else if preset.blocks.len() > 1 {
        // Get from translation block
        let trans_block = &preset.blocks[1];
        if !trans_block.selected_language.is_empty() {
            trans_block.selected_language.clone()
        } else {
            trans_block.language_vars.get("language").cloned()
                .or_else(|| trans_block.language_vars.get("language1").cloned())
                .unwrap_or_else(|| "English".to_string())
        }
    } else {
        "English".to_string()
    };
        
        // Initialize NEW_TARGET_LANGUAGE so translation loop uses saved language from start
        if !target_language.is_empty() {
            if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
                *new_lang = target_language.clone();
            }
            // Trigger a language "change" so translation loop picks it up immediately
            LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
        }
        
        // Calculate positions
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        
        let has_translation = preset.blocks.len() > 1;
        
        // Use configured sizes
        let main_w = transcription_size.0;
        let main_h = transcription_size.1;
        let trans_w = trans_size.0;
        let trans_h = trans_size.1;
        
        let (main_x, main_y) = if has_translation {
            let total_w = main_w + trans_w + GAP;
            ((screen_w - total_w) / 2, (screen_h - main_h) / 2)
        } else {
            ((screen_w - main_w) / 2, (screen_h - main_h) / 2)
        };
        
        // Create popup window with resize support
        let main_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Realtime Transcription"),
            WS_POPUP | WS_VISIBLE,
            main_x, main_y, main_w, main_h,
            None, None, Some(instance.into()), None
        ).unwrap();
        
        // Enable rounded corners (Windows 11+)
        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            main_hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&corner_pref) as u32,
        );
        
        REALTIME_HWND = main_hwnd;
        
        // Create WebView for main overlay
        create_realtime_webview(main_hwnd, false, &config_audio_source, &target_language, &config_translation_model, font_size);
        
        // --- Create Translation Overlay if needed ---
        let translation_hwnd = if has_translation {
            let trans_class = w!("RealtimeTranslationWebViewOverlay");
            REGISTER_TRANSLATION_CLASS.call_once(|| {
                let mut wc = WNDCLASSW::default();
                wc.lpfnWndProc = Some(translation_wnd_proc);
                wc.hInstance = instance.into();
                wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
                wc.lpszClassName = trans_class;
                wc.style = CS_HREDRAW | CS_VREDRAW;
                wc.hbrBackground = HBRUSH(std::ptr::null_mut());
                let _ = RegisterClassW(&wc);
            });
            
            let trans_x = main_x + main_w + GAP;
            let trans_hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                trans_class,
                w!("Translation"),
                WS_POPUP | WS_VISIBLE,
                trans_x, main_y, trans_w, trans_h,
                None, None, Some(instance.into()), None
            ).unwrap();
            
            // Enable rounded corners (Windows 11+)
            let corner_pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                trans_hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&corner_pref) as u32,
            );
            
            TRANSLATION_HWND = trans_hwnd;
            create_realtime_webview(trans_hwnd, true, "mic", &target_language, &config_translation_model, font_size);
            
            Some(trans_hwnd)
        } else {
            TRANSLATION_HWND = HWND::default();
            None
        };
        
        // Start realtime transcription
        start_realtime_transcription(
            preset,
            REALTIME_STOP_SIGNAL.clone(),
            main_hwnd,
            translation_hwnd,
            REALTIME_STATE.clone(),
        );
        
        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_QUIT { break; }
        }
        
        // Cleanup
        destroy_realtime_webview(REALTIME_HWND);
        if !TRANSLATION_HWND.is_invalid() {
            destroy_realtime_webview(TRANSLATION_HWND);
        }
        
        IS_ACTIVE = false;
        REALTIME_HWND = HWND::default();
        TRANSLATION_HWND = HWND::default();
    }
}
