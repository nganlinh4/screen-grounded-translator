//! Parakeet local transcription using ONNX models

use crate::api::realtime_audio::SharedRealtimeState;
use crate::config::Preset;
use anyhow::Result;
use parakeet_rs::ParakeetEOU;
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

pub fn run_parakeet_transcription(
    _preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    state: SharedRealtimeState,
) -> Result<()> {
    // 1. Check/Download Model
    if !super::model_loader::is_model_downloaded() {
        match super::model_loader::download_parakeet_model(stop_signal.clone()) {
            Ok(_) => {}
            Err(e) => {
                // Check if this was a user-initiated cancellation (not an actual error)
                let err_msg = e.to_string();
                if err_msg.contains("cancelled") || stop_signal.load(Ordering::Relaxed) {
                    // Download was cancelled by user - this is not an error, just exit gracefully
                    println!("Parakeet download was cancelled by user");
                    return Ok(());
                }
                // Real error - propagate it
                return Err(e);
            }
        }
        if stop_signal.load(Ordering::Relaxed) {
            return Ok(());
        }
    }

    // 2. Load Model
    let model_dir = super::model_loader::get_parakeet_model_dir();
    println!("Loading Parakeet model from: {:?}", model_dir);

    let mut parakeet = ParakeetEOU::from_pretrained(&model_dir, None)
        .map_err(|e| anyhow::anyhow!("Failed to load Parakeet model: {:?}", e))?;

    println!("Parakeet model loaded successfully!");

    // 3. Audio Setup - use the same capture functions as Gemini transcription
    let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));

    let audio_source = {
        let app = crate::APP.lock().unwrap();
        app.config.realtime_audio_source.clone()
    };

    println!(
        "Parakeet: Setting up audio capture (source: {})...",
        audio_source
    );
    let _stream = if audio_source == "mic" {
        Some(super::capture::start_mic_capture(
            audio_buffer.clone(),
            stop_signal.clone(),
        )?)
    } else {
        Some(super::capture::start_device_loopback_capture(
            audio_buffer.clone(),
            stop_signal.clone(),
        )?)
    };
    println!("Parakeet: Audio capture started, entering processing loop...");

    // Buffer for accumulating samples to reach chunk size
    let mut sample_accumulator: Vec<f32> = Vec::with_capacity(CHUNK_SIZE * 2);

    // 4. Processing Loop
    while !stop_signal.load(Ordering::Relaxed) {
        if AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst)
            || crate::overlay::realtime_webview::TRANSCRIPTION_MODEL_CHANGE.load(Ordering::SeqCst)
        {
            break;
        }

        // Read from buffer and convert i16 to f32
        let new_samples: Vec<f32> = {
            let mut buf = audio_buffer.lock().unwrap();
            if !buf.is_empty() {
                // Convert i16 to f32 normalized (-1.0 to 1.0)
                let samples: Vec<f32> = buf.drain(..).map(|s| s as f32 / 32768.0).collect();
                samples
            } else {
                Vec::new()
            }
        };

        if !new_samples.is_empty() {
            // Calculate RMS for volume visualization
            let sum_sq: f64 = new_samples.iter().map(|&s| (s as f64).powi(2)).sum();
            let rms = (sum_sq / new_samples.len() as f64).sqrt() as f32;
            REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);

            unsafe {
                if !overlay_hwnd.is_invalid() {
                    let _ =
                        PostMessageW(Some(overlay_hwnd), WM_VOLUME_UPDATE, WPARAM(0), LPARAM(0));
                }
            }

            // Accumulate samples
            sample_accumulator.extend(new_samples);
        }

        // Process when we have enough samples for a chunk
        while sample_accumulator.len() >= CHUNK_SIZE {
            let chunk: Vec<f32> = sample_accumulator.drain(..CHUNK_SIZE).collect();

            // Transcribe the chunk
            match parakeet.transcribe(&chunk, false) {
                Ok(text) => {
                    if !text.is_empty() {
                        // SentencePiece tokenizer uses ▁ (U+2581) to mark WORD START
                        // - Token "▁hello" means "start of word 'hello'" → append " hello"
                        // - Token "ing" means "continuation of previous word" → append "ing"

                        // Process the text: check if it starts with ▁
                        let starts_with_word =
                            text.starts_with('\u{2581}') || text.starts_with('▁');

                        // Replace all ▁ with spaces (handles "▁word▁another" cases)
                        let processed = text.replace('\u{2581}', " ").replace('▁', " ");
                        let processed = processed.trim(); // Remove leading/trailing spaces

                        if !processed.is_empty() {
                            // If original started with ▁, prepend a space (new word)
                            // Otherwise, append directly (continuation token)
                            let to_append = if starts_with_word {
                                format!(" {}", processed)
                            } else {
                                processed.to_string()
                            };

                            // Append transcription to state
                            if let Ok(mut s) = state.lock() {
                                s.append_transcript(&to_append);
                            }

                            // Update overlay
                            unsafe {
                                if !overlay_hwnd.is_invalid() {
                                    let _ = PostMessageW(
                                        Some(overlay_hwnd),
                                        WM_REALTIME_UPDATE,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                }
                            }
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

    // Flush: send silence chunks to get any remaining text
    println!("Flushing Parakeet decoder...");
    let silence = vec![0.0f32; CHUNK_SIZE];
    for _ in 0..3 {
        if let Ok(text) = parakeet.transcribe(&silence, false) {
            if !text.is_empty() {
                // Same SentencePiece processing as main loop
                let starts_with_word = text.starts_with('\u{2581}') || text.starts_with('▁');
                let processed = text.replace('\u{2581}', " ").replace('▁', " ");
                let processed = processed.trim();

                if !processed.is_empty() {
                    let to_append = if starts_with_word {
                        format!(" {}", processed)
                    } else {
                        processed.to_string()
                    };

                    if let Ok(mut s) = state.lock() {
                        s.append_transcript(&to_append);
                    }
                    unsafe {
                        if !overlay_hwnd.is_invalid() {
                            let _ = PostMessageW(
                                Some(overlay_hwnd),
                                WM_REALTIME_UPDATE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
