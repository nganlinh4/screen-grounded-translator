use super::state::{RealtimeState, TranscriptionMethod};
use crate::config::Preset;
use anyhow::Result;
use parakeet_rs::{ExecutionConfig, ExecutionProvider, ParakeetEOU};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::{REALTIME_RMS, WM_REALTIME_UPDATE, WM_VOLUME_UPDATE};
use crate::overlay::realtime_webview::AUDIO_SOURCE_CHANGE;

/// 160ms chunk at 16kHz = 2560 samples (recommended by parakeet-rs)
const CHUNK_SIZE: usize = 2560;

/// Wrapper for the existing Realtime Overlay functionality
pub fn run_parakeet_transcription(
    _preset: Preset,
    stop_signal: Arc<AtomicBool>,
    dummy_pause_signal: Arc<AtomicBool>,
    full_audio_buffer: Option<Arc<Mutex<Vec<i16>>>>,
    hide_recording_ui: bool,
    hwnd_overlay: Option<HWND>,
    state: Arc<Mutex<RealtimeState>>,
) -> Result<()> {
    // Set state early (best effort)
    if let Ok(mut s) = state.lock() {
        s.set_transcription_method(TranscriptionMethod::Parakeet);
    }

    run_parakeet_session(
        stop_signal.clone(),
        dummy_pause_signal,
        full_audio_buffer,
        hwnd_overlay, // Send volume updates to overlay
        hide_recording_ui,
        true, // Don't show download badge (webview handles its own modal)
        None, // Use global config
        true, // auto_stop_enabled
        move |text| {
            // Callback for each text segment
            if let Ok(mut s) = state.lock() {
                s.append_transcript(&text);
            }
            // Notify overlay to update text
            if let Some(h) = hwnd_overlay {
                unsafe {
                    if !h.is_invalid() {
                        let _ = PostMessageW(Some(h), WM_REALTIME_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            }
        },
    )
}

/// Generic Parakeet session that can be used by both Realtime Overlay and Prompt DJ
pub fn run_parakeet_session<F>(
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
    full_audio_buffer: Option<Arc<Mutex<Vec<i16>>>>,
    overlay_hwnd_opt: Option<HWND>,
    hide_recording_ui: bool,
    use_badge: bool,
    audio_source_override: Option<String>,
    auto_stop_recording: bool,
    mut callback: F,
) -> Result<()>
where
    F: FnMut(String),
{
    // 1. Check/Download Model
    if !super::model_loader::is_model_downloaded() {
        // Pass use_badge to download function
        match super::model_loader::download_parakeet_model(stop_signal.clone(), use_badge) {
            Ok(_) => {}
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("cancelled") || stop_signal.load(Ordering::Relaxed) {
                    println!("Parakeet download was cancelled by user");
                    return Ok(());
                }
                return Err(e);
            }
        }
        if stop_signal.load(Ordering::Relaxed) {
            return Ok(());
        }
    }

    // 2. Load Model
    let model_dir = super::model_loader::get_parakeet_model_dir();
    let config = ExecutionConfig::new().with_execution_provider(ExecutionProvider::DirectML);

    let mut parakeet = ParakeetEOU::from_pretrained(&model_dir, Some(config))
        .map_err(|e| anyhow::anyhow!("Failed to load Parakeet model: {:?}", e))?;

    // 3. Audio Setup
    let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));

    let (audio_source, check_per_app) = if let Some(s) = audio_source_override {
        (s, false)
    } else {
        let app = crate::APP.lock().unwrap();
        (app.config.realtime_audio_source.clone(), true)
    };

    use crate::overlay::realtime_webview::{REALTIME_TTS_ENABLED, SELECTED_APP_PID};
    let tts_enabled = REALTIME_TTS_ENABLED.load(Ordering::SeqCst);
    let selected_pid = SELECTED_APP_PID.load(Ordering::SeqCst);

    let using_per_app_capture =
        check_per_app && audio_source == "device" && tts_enabled && selected_pid > 0;

    let _stream = if using_per_app_capture {
        #[cfg(target_os = "windows")]
        {
            super::capture::start_per_app_capture(
                selected_pid,
                audio_buffer.clone(),
                stop_signal.clone(),
                pause_signal.clone(),
            )?;
            None
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    } else if audio_source == "mic" {
        Some(super::capture::start_mic_capture(
            audio_buffer.clone(),
            stop_signal.clone(),
            pause_signal.clone(),
        )?)
    } else if audio_source == "device" && tts_enabled && selected_pid == 0 {
        None
    } else {
        Some(super::capture::start_device_loopback_capture(
            audio_buffer.clone(),
            stop_signal.clone(),
            pause_signal.clone(),
        )?)
    };

    let mut sample_accumulator: Vec<f32> = Vec::with_capacity(CHUNK_SIZE * 2);

    let mut has_spoken = false;
    let mut last_active = std::time::Instant::now();
    let mut first_speech: Option<std::time::Instant> = None;

    // 4. Processing Loop
    while !stop_signal.load(Ordering::Relaxed) {
        if !hide_recording_ui {
            if let Some(hwnd) = overlay_hwnd_opt {
                if unsafe {
                    !windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool()
                } {
                    break;
                }
            }
        }
        if pause_signal.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }
        if AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst)
            || crate::overlay::realtime_webview::TRANSCRIPTION_MODEL_CHANGE.load(Ordering::SeqCst)
        {
            break;
        }

        let new_samples: Vec<f32> = {
            let mut buf = audio_buffer.lock().unwrap();
            if !buf.is_empty() {
                buf.drain(..).map(|s| s as f32 / 32768.0).collect()
            } else {
                Vec::new()
            }
        };

        if !new_samples.is_empty() {
            // Volume Visualization
            let sum_sq: f64 = new_samples.iter().map(|&s| (s as f64).powi(2)).sum();
            let rms = (sum_sq / new_samples.len() as f64).sqrt() as f32;
            REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);
            // Also update recording overlay viz
            crate::overlay::recording::update_audio_viz(rms);
            if rms > 0.001 {
                crate::overlay::recording::AUDIO_WARMUP_COMPLETE.store(true, Ordering::SeqCst);
            }

            if auto_stop_recording {
                if rms > 0.015 {
                    last_active = std::time::Instant::now();
                    if !has_spoken {
                        has_spoken = true;
                        first_speech = Some(std::time::Instant::now());
                    }
                } else if has_spoken {
                    if let Some(start) = first_speech {
                        if last_active.elapsed().as_millis() > 800
                            && start.elapsed().as_millis() > 2000
                        {
                            stop_signal.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            }

            if let Some(hwnd) = overlay_hwnd_opt {
                unsafe {
                    if !hwnd.is_invalid() {
                        let _ = PostMessageW(Some(hwnd), WM_VOLUME_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            }

            if let Some(full_buf) = &full_audio_buffer {
                if let Ok(mut full) = full_buf.lock() {
                    full.extend(new_samples.iter().map(|&s| (s * 32768.0) as i16));
                }
            }

            sample_accumulator.extend(new_samples);
        }

        while sample_accumulator.len() >= CHUNK_SIZE {
            let chunk: Vec<f32> = sample_accumulator.drain(..CHUNK_SIZE).collect();

            match parakeet.transcribe(&chunk, false) {
                Ok(text) => {
                    if !text.is_empty() {
                        let processed = process_sentencepiece_text(&text);
                        if !processed.is_empty() {
                            callback(processed);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Parakeet transcription error: {:?}", e);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Flush
    let silence = vec![0.0f32; CHUNK_SIZE];
    for _ in 0..3 {
        if let Ok(text) = parakeet.transcribe(&silence, false) {
            if !text.is_empty() {
                let processed = process_sentencepiece_text(&text);
                if !processed.is_empty() {
                    callback(processed);
                }
            }
        }
    }

    Ok(())
}

fn process_sentencepiece_text(text: &str) -> String {
    let starts_with_word = text.starts_with('\u{2581}') || text.starts_with('▁');
    let processed = text.replace('\u{2581}', " ").replace('▁', " ");
    let processed = processed.trim();

    if processed.is_empty() {
        return String::new();
    }

    if starts_with_word {
        format!(" {}", processed)
    } else {
        processed.to_string()
    }
}
