use crate::api::realtime_audio::SharedRealtimeState;
use crate::config::Preset;
use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::{REALTIME_RMS, WM_VOLUME_UPDATE};
use crate::overlay::realtime_webview::AUDIO_SOURCE_CHANGE;

pub fn run_parakeet_transcription(
    _preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    _state: SharedRealtimeState,
) -> Result<()> {
    // 1. Check/Download Model
    if !super::model_loader::is_model_downloaded() {
        super::model_loader::download_parakeet_model(stop_signal.clone())?;
        if stop_signal.load(Ordering::Relaxed) {
            return Ok(());
        }
    }

    // 2. Load Model
    let model_dir = super::model_loader::get_parakeet_model_dir();
    let _encoder_path = model_dir.join("encoder.onnx").to_str().unwrap().to_string();
    let _decoder_path = model_dir
        .join("decoder_joint.onnx")
        .to_str()
        .unwrap()
        .to_string();
    let _tokenizer_path = model_dir
        .join("tokenizer.json")
        .to_str()
        .unwrap()
        .to_string();

    // Attempt to instantiate EOU model (Constructor issue confirmed: new() not found)
    // let _model = ParakeetEOU::new(&encoder_path, &decoder_path, &tokenizer_path)
    //    .map_err(|e| anyhow::anyhow!("Failed to load Parakeet EOU model: {}", e))?;

    // 3. Audio Setup (CPAL)
    let host = cpal::default_host();

    // Choose device based on Config/UI
    let device = {
        let app = crate::APP.lock().unwrap();
        let source = &app.config.realtime_audio_source;
        if source == "mic" {
            host.default_input_device()
        } else {
            host.default_output_device()
        }
    }
    .ok_or_else(|| anyhow::anyhow!("No audio device found"))?;

    let config = device.default_input_config()?;
    let _sample_rate = config.sample_rate();

    // Channel for ringbuf
    let (mut producer, mut consumer) = ringbuf::HeapRb::<f32>::new(16384 * 4).split();

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    let overlay_hwnd_ptr = overlay_hwnd.0 as usize;

    macro_rules! build_stream {
        ($sample_type:ty, $converter:expr) => {{
            device.build_input_stream(
                &config.into(),
                move |data: &[$sample_type], _: &_| {
                    let converter = $converter;
                    let mut sum_sq = 0.0;
                    for &sample in data {
                        let s = converter(sample);
                        sum_sq += s * s;
                    }
                    let rms = (sum_sq / data.len() as f32).sqrt();
                    REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);

                    for &sample in data {
                        let _ = producer.push_slice(&[converter(sample)]);
                    }

                    unsafe {
                        let hwnd = HWND(overlay_hwnd_ptr as *mut std::ffi::c_void);
                        if !hwnd.is_invalid() {
                            let _ =
                                PostMessageW(Some(hwnd), WM_VOLUME_UPDATE, WPARAM(0), LPARAM(0));
                        }
                    }
                },
                err_fn,
                None,
            )?
        }};
    }

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_stream!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => build_stream!(i16, |s: i16| (s as f32) / 32768.0),
        cpal::SampleFormat::U16 => build_stream!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        sample_format => {
            return Err(anyhow::anyhow!(
                "Unsupported sample format '{:?}'",
                sample_format
            ))
        }
    };

    stream.play()?;

    // 4. Processing Loop
    let mut audio_buffer: Vec<f32> = Vec::new();
    let _last_partial = String::new();

    while !stop_signal.load(Ordering::Relaxed) {
        if AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst)
            || crate::overlay::realtime_webview::TRANSCRIPTION_MODEL_CHANGE.load(Ordering::SeqCst)
        {
            break;
        }

        let chunk_size = 4096;
        if consumer.occupied_len() >= chunk_size {
            audio_buffer.resize(chunk_size, 0.0);
            consumer.pop_slice(&mut audio_buffer);

            // stream_recognizer.accept_waveform(sample_rate, &audio_buffer);

            // if let Ok(text) = stream_recognizer.get_partial_result() {
            //      if !text.is_empty() {
            //          // Simple diff: if text starts with last_partial, append suffix.
            //          // Else, replace?
            //          // Parakeet partials are usually stable prefixes.
            //          if text.starts_with(&last_partial) {
            //              let new_part = &text[last_partial.len()..];
            //              if !new_part.is_empty() {
            //                  if let Ok(mut s) = state.lock() {
            //                      s.append_transcript(new_part);
            //                  }
            //              }
            //          } else {
            //              // Changed completely? (e.g. correction).
            //              // We can't rewrite `full_transcript`.
            //              // We just append new text relative to last_partial length?
            //              // Or we assume `append_transcript` matches our needs.
            //              // Pragramtic approach: just append diff.
            //              let new_part = if text.len() > last_partial.len() {
            //                  &text[last_partial.len()..]
            //              } else {
            //                  ""
            //              };
            //              if !new_part.is_empty() {
            //                  if let Ok(mut s) = state.lock() {
            //                      s.append_transcript(new_part);
            //                  }
            //              }
            //          }
            //          last_partial = text;

            //          unsafe {
            //              let _ = PostMessageW(
            //                  Some(overlay_hwnd),
            //                  WM_REALTIME_UPDATE,
            //                  WPARAM(0),
            //                  LPARAM(0),
            //              );
            //          }
            //      }
            // }
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    Ok(())
}
