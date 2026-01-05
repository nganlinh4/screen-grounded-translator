//! Overlay lifecycle management (show/stop/check active)

use super::state::*;
use super::webview::*;
use super::wndproc::*;
use crate::api::realtime_audio::{start_realtime_transcription, RealtimeState};
use crate::APP;
use std::sync::atomic::Ordering;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn is_realtime_overlay_active() -> bool {
    unsafe { IS_ACTIVE && !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() }
}

/// Stop the realtime overlay and hide windows
pub fn stop_realtime_overlay() {
    // Stop any playing TTS immediately
    crate::api::tts::TTS_MANAGER.stop();

    // Stop Minimal Mode if active
    crate::overlay::realtime_egui::MINIMAL_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    REALTIME_STOP_SIGNAL.store(true, Ordering::SeqCst);

    unsafe {
        // Close app selection popup if open
        let popup_val = APP_SELECTION_HWND.load(std::sync::atomic::Ordering::SeqCst);
        if popup_val != 0 {
            let popup_hwnd = HWND(popup_val as *mut std::ffi::c_void);
            let _ = PostMessageW(Some(popup_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            APP_SELECTION_HWND.store(0, std::sync::atomic::Ordering::SeqCst);
        }

        if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
            let _ = PostMessageW(
                Some(REALTIME_HWND),
                WM_APP_REALTIME_HIDE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn warmup() {
    std::thread::spawn(|| unsafe {
        internal_create_realtime_loop();
    });
}

pub fn show_realtime_overlay(preset_idx: usize) {
    unsafe {
        // Check if warmed up
        if !IS_WARMED_UP {
            // Show localized message that feature is not ready yet
            let ui_lang = crate::APP.lock().unwrap().config.ui_language.clone();
            let locale = crate::gui::locale::LocaleText::get(&ui_lang);
            crate::overlay::auto_copy_badge::show_notification(locale.live_translate_loading);
            return;
        }

        if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
            let _ = PostMessageW(
                Some(REALTIME_HWND),
                WM_APP_REALTIME_START,
                WPARAM(preset_idx),
                LPARAM(0),
            );
        }
    }
}

unsafe fn internal_create_realtime_loop() {
    let instance = GetModuleHandleW(None).unwrap();

    // --- Register Classes ---
    let class_name = w!("RealtimeWebViewOverlay");
    REGISTER_REALTIME_CLASS.call_once(|| {
        let mut wc = WNDCLASSW::default();
        wc.lpfnWndProc = Some(realtime_wnd_proc_internal);
        wc.hInstance = instance.into();
        wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
        wc.lpszClassName = class_name;
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.hbrBackground = HBRUSH(std::ptr::null_mut());
        let _ = RegisterClassW(&wc);
    });

    let trans_class = w!("RealtimeTranslationWebViewOverlay");
    REGISTER_TRANSLATION_CLASS.call_once(|| {
        let mut wc = WNDCLASSW::default();
        wc.lpfnWndProc = Some(translation_wnd_proc_internal);
        wc.hInstance = instance.into();
        wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
        wc.lpszClassName = trans_class;
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.hbrBackground = HBRUSH(std::ptr::null_mut());
        let _ = RegisterClassW(&wc);
    });

    // Create windows hidden
    let main_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class_name,
        w!("Realtime Transcription"),
        WS_POPUP, // Hidden initially
        0,
        0,
        100,
        100,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .unwrap();

    let trans_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        trans_class,
        w!("Translation"),
        WS_POPUP, // Hidden initially
        0,
        0,
        100,
        100,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .unwrap();

    // Enable rounded corners (Windows 11+)
    let corner_pref = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
        main_hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner_pref as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&corner_pref) as u32,
    );
    let _ = DwmSetWindowAttribute(
        trans_hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner_pref as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&corner_pref) as u32,
    );

    REALTIME_HWND = main_hwnd;
    TRANSLATION_HWND = trans_hwnd;

    // Create WebViews
    create_realtime_webview(
        main_hwnd,
        false,
        "device",
        "English",
        "google-gtx",
        "gemini",
        16,
    );
    create_realtime_webview(
        trans_hwnd,
        true,
        "device",
        "English",
        "google-gtx",
        "gemini",
        16,
    );

    // Mark as warmed up and ready
    IS_WARMED_UP = true;

    // Message loop
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).into() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
        if msg.message == WM_QUIT {
            break;
        }
    }

    // Cleanup
    destroy_realtime_webview(REALTIME_HWND);
    destroy_realtime_webview(TRANSLATION_HWND);
    IS_ACTIVE = false;
    REALTIME_HWND = HWND::default();
    TRANSLATION_HWND = HWND::default();
}

unsafe extern "system" fn realtime_wnd_proc_internal(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_APP_REALTIME_START {
        let preset_idx = wparam.0;
        handle_start_overlay(preset_idx);
        return LRESULT(0);
    }
    realtime_wnd_proc(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn translation_wnd_proc_internal(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    translation_wnd_proc(hwnd, msg, wparam, lparam)
}

unsafe fn handle_start_overlay(preset_idx: usize) {
    if IS_ACTIVE {
        return;
    }

    let mut preset = APP.lock().unwrap().config.presets[preset_idx].clone();

    // Check if Minimal Mode
    if preset.realtime_window_mode == "minimal" {
        crate::overlay::realtime_egui::show_realtime_egui_overlay(preset_idx);
        return;
    }

    // Reset state
    IS_ACTIVE = true;
    REALTIME_STOP_SIGNAL.store(false, Ordering::SeqCst);
    MIC_VISIBLE.store(true, Ordering::SeqCst);
    TRANS_VISIBLE.store(true, Ordering::SeqCst);
    AUDIO_SOURCE_CHANGE.store(false, Ordering::SeqCst);
    LANGUAGE_CHANGE.store(false, Ordering::SeqCst);
    TRANSLATION_MODEL_CHANGE.store(false, Ordering::SeqCst);

    {
        let mut state = REALTIME_STATE.lock().unwrap();
        *state = RealtimeState::new();
    }

    // Fetch config
    let (
        font_size,
        config_audio_source,
        config_language,
        config_translation_model,
        config_transcription_model,
        trans_size,
        transcription_size,
    ) = {
        let app = APP.lock().unwrap();
        (
            app.config.realtime_font_size,
            app.config.realtime_audio_source.clone(),
            app.config.realtime_target_language.clone(),
            app.config.realtime_translation_model.clone(),
            app.config.realtime_transcription_model.clone(),
            app.config.realtime_translation_size,
            app.config.realtime_transcription_size,
        )
    };

    // Default to "device" if no audio source is saved
    let effective_audio_source = if config_audio_source.is_empty() {
        "device".to_string()
    } else {
        config_audio_source.clone()
    };

    preset.audio_source = effective_audio_source.clone();
    if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
        *new_source = effective_audio_source.clone();
    }

    let target_language = if !config_language.is_empty() {
        config_language
    } else if preset.blocks.len() > 1 {
        let trans_block = &preset.blocks[1];
        if !trans_block.selected_language.is_empty() {
            trans_block.selected_language.clone()
        } else {
            trans_block
                .language_vars
                .get("language")
                .cloned()
                .or_else(|| trans_block.language_vars.get("language1").cloned())
                .unwrap_or_else(|| "English".to_string())
        }
    } else {
        "English".to_string()
    };

    if !target_language.is_empty() {
        if let Ok(mut new_lang) = NEW_TARGET_LANGUAGE.lock() {
            *new_lang = target_language.clone();
        }
        LANGUAGE_CHANGE.store(true, Ordering::SeqCst);
    }

    // Calculate positions
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let has_translation = preset.blocks.len() > 1;
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

    // Update window positions and sizes
    let _ = SetWindowPos(
        REALTIME_HWND,
        Some(HWND_TOPMOST),
        main_x,
        main_y,
        main_w,
        main_h,
        SWP_SHOWWINDOW,
    );
    if has_translation {
        let trans_x = main_x + main_w + GAP;
        let _ = SetWindowPos(
            TRANSLATION_HWND,
            Some(HWND_TOPMOST),
            trans_x,
            main_y,
            trans_w,
            trans_h,
            SWP_SHOWWINDOW,
        );
    } else {
        let _ = ShowWindow(TRANSLATION_HWND, SW_HIDE);
    }

    // Notify WebViews of new settings
    notify_webview_settings(
        REALTIME_HWND,
        &effective_audio_source,
        &target_language,
        &config_translation_model,
        &config_transcription_model,
        font_size,
    );

    // Explicitly resize WebViews to match window sizes
    resize_webview(REALTIME_HWND, main_w, main_h);

    // Clear text to start fresh
    clear_webview_text(REALTIME_HWND);

    if has_translation {
        notify_webview_settings(
            TRANSLATION_HWND,
            "mic",
            &target_language,
            &config_translation_model,
            &config_transcription_model,
            font_size,
        );
        resize_webview(TRANSLATION_HWND, trans_w, trans_h);
        clear_webview_text(TRANSLATION_HWND);
    }

    // Sync visibility state to webviews (fixes toggled->hidden state on re-show)
    sync_visibility_to_webviews();

    // Start transcription
    let trans_hwnd_opt = if has_translation {
        Some(TRANSLATION_HWND)
    } else {
        None
    };
    start_realtime_transcription(
        preset,
        REALTIME_STOP_SIGNAL.clone(),
        REALTIME_HWND,
        trans_hwnd_opt,
        REALTIME_STATE.clone(),
    );
}

fn notify_webview_settings(
    hwnd: HWND,
    source: &str,
    lang: &str,
    model: &str,
    trans_model: &str,
    font_size: u32,
) {
    let hwnd_key = hwnd.0 as isize;
    let script = format!(
        "if(window.updateSettings) window.updateSettings({{ audioSource: '{}', targetLanguage: '{}', translationModel: '{}', transcriptionModel: '{}', fontSize: {} }});",
        source, lang, model, trans_model, font_size
    );
    REALTIME_WEBVIEWS.with(|wvs| {
        if let Some(webview) = wvs.borrow().get(&hwnd_key) {
            let _ = webview.evaluate_script(&script);
        }
    });
}

fn resize_webview(hwnd: HWND, width: i32, height: i32) {
    let hwnd_key = hwnd.0 as isize;
    REALTIME_WEBVIEWS.with(|wvs| {
        if let Some(webview) = wvs.borrow().get(&hwnd_key) {
            let _ = webview.set_bounds(wry::Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    width as u32,
                    height as u32,
                )),
            });
        }
    });
}
