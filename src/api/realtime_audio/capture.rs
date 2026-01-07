//! Audio capture implementations: per-app WASAPI, device loopback, and microphone

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use super::REALTIME_RMS;

/// Start per-app audio capture using WASAPI process loopback (Windows 10 1903+)
///
/// This function spawns a thread that captures audio from a specific process
/// and pushes samples to the provided buffer.
#[cfg(target_os = "windows")]
pub fn start_per_app_capture(
    process_id: u32,
    audio_buffer: Arc<Mutex<Vec<i16>>>,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
) -> Result<()> {
    use std::collections::VecDeque;
    use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};

    std::thread::spawn(move || {
        // Initialize COM for this thread (required for WASAPI)
        if wasapi::initialize_mta().is_err() {
            eprintln!("Per-app capture: Failed to initialize MTA");
            return;
        }

        // Create loopback capture client for the specified process
        // include_tree=true to include child processes (browsers often use separate audio processes)
        let audio_client = match AudioClient::new_application_loopback_client(process_id, true) {
            Ok(client) => client,
            Err(e) => {
                eprintln!(
                    "Per-app capture: Failed to create loopback client for PID {}: {:?}",
                    process_id, e
                );
                return;
            }
        };

        // Configure desired format: 16kHz mono 16-bit (what Gemini expects)
        // With autoconvert=true, Windows will handle resampling from the app's native format
        let desired_format = WaveFormat::new(
            16, // bits per sample
            16, // valid bits
            &SampleType::Int,
            16000, // 16kHz sample rate
            1,     // mono
            None,
        );

        // Buffer duration: 100ms in 100-nanosecond units
        let buffer_duration_hns = 1_000_000i64; // 100ms

        // Configure stream mode with auto-conversion
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns,
        };

        let mut audio_client = audio_client;
        if let Err(e) = audio_client.initialize_client(&desired_format, &Direction::Capture, &mode)
        {
            eprintln!(
                "Per-app capture: Failed to initialize audio client: {:?}",
                e
            );
            eprintln!("Hint: Per-app capture requires Windows 10 version 1903 or later");
            return;
        }

        // Get the capture client interface
        let capture_client = match audio_client.get_audiocaptureclient() {
            Ok(client) => client,
            Err(e) => {
                eprintln!("Per-app capture: Failed to get capture client: {:?}", e);
                return;
            }
        };

        // Get event handle for efficient waiting
        let event_handle = match audio_client.set_get_eventhandle() {
            Ok(handle) => handle,
            Err(e) => {
                eprintln!("Per-app capture: Failed to get event handle: {:?}", e);
                return;
            }
        };

        // Start the audio stream
        if let Err(e) = audio_client.start_stream() {
            eprintln!("Per-app capture: Failed to start stream: {:?}", e);
            return;
        }

        // Per-app capture started for process_id

        // Buffer for reading audio data
        let mut capture_buffer: VecDeque<u8> = VecDeque::new();

        // Capture loop
        while !stop_signal.load(Ordering::Relaxed) {
            if pause_signal.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            // Wait for buffer to be ready (up to 100ms timeout)
            if event_handle.wait_for_event(100).is_err() {
                continue; // Timeout, check stop signal and try again
            }

            // Read captured data
            match capture_client.read_from_device_to_deque(&mut capture_buffer) {
                Ok(_buffer_info) => {
                    // Check if we received any data
                    if !capture_buffer.is_empty() {
                        // Convert bytes to i16 samples (16-bit = 2 bytes per sample)
                        // Format is 16-bit mono at 16kHz
                        let bytes_per_sample = 2;
                        let sample_count = capture_buffer.len() / bytes_per_sample;

                        if sample_count > 0 {
                            // Drain buffer and convert to i16
                            let mut samples: Vec<i16> = Vec::with_capacity(sample_count);

                            while capture_buffer.len() >= bytes_per_sample {
                                let low = capture_buffer.pop_front().unwrap_or(0);
                                let high = capture_buffer.pop_front().unwrap_or(0);
                                let sample = i16::from_le_bytes([low, high]);
                                samples.push(sample);
                            }

                            // Audio received from per-app capture - add to buffer

                            // Push to shared audio buffer
                            if let Ok(mut buf) = audio_buffer.lock() {
                                buf.extend(&samples);
                            }

                            // Calculate RMS for volume visualization
                            if !samples.is_empty() {
                                let sum_sq: f64 =
                                    samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
                                let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
                                REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);
                            }
                        }
                    }
                }
                Err(e) => {
                    // Check for specific errors that indicate process ended or connection lost
                    eprintln!("Per-app capture: Read error: {:?}", e);
                    // Small delay before retrying
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }

        // Cleanup
        let _ = audio_client.stop_stream();
        // Per-app capture stopped
    });

    Ok(())
}

/// Start device loopback capture (captures all system audio)
/// Returns the cpal Stream that must be kept alive
pub fn start_device_loopback_capture(
    audio_buffer: Arc<Mutex<Vec<i16>>>,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    #[cfg(target_os = "windows")]
    let host = cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host());
    #[cfg(not(target_os = "windows"))]
    let host = cpal::default_host();

    // Use default output device for loopback
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("No output device available"))?;
    let config = device.default_output_config()?;

    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;

    let audio_buffer_clone = audio_buffer.clone();

    // Resample to 16kHz if needed
    let target_rate = 16000u32;
    let resample_ratio = target_rate as f64 / sample_rate as f64;

    let stop_signal_audio = stop_signal.clone();
    let pause_signal_audio = pause_signal.clone();
    let err_fn = |err| eprintln!("Audio stream error: {}", err);

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if stop_signal_audio.load(Ordering::Relaxed)
                    || pause_signal_audio.load(Ordering::Relaxed)
                {
                    return;
                }

                // Convert to mono and i16
                let mono_samples: Vec<i16> = data
                    .chunks(channels)
                    .map(|frame| {
                        let sum: f32 = frame.iter().sum();
                        let avg = sum / channels as f32;
                        (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                    })
                    .collect();

                // Simple resampling (linear interpolation)
                let resampled: Vec<i16> = if resample_ratio < 1.0 {
                    let new_len = (mono_samples.len() as f64 * resample_ratio) as usize;
                    (0..new_len)
                        .map(|i| {
                            let src_idx = i as f64 / resample_ratio;
                            let idx0 = src_idx as usize;
                            let idx1 = (idx0 + 1).min(mono_samples.len() - 1);
                            let frac = src_idx - idx0 as f64;
                            let s0 = mono_samples[idx0] as f64;
                            let s1 = mono_samples[idx1] as f64;
                            (s0 + (s1 - s0) * frac) as i16
                        })
                        .collect()
                } else {
                    mono_samples
                };

                if let Ok(mut buf) = audio_buffer_clone.lock() {
                    buf.extend(resampled.iter().cloned());
                }

                // Calculate RMS for volume visualization
                if !resampled.is_empty() {
                    let sum_sq: f64 = resampled
                        .iter()
                        .map(|&s| (s as f64 / 32768.0).powi(2))
                        .sum();
                    let rms = (sum_sq / resampled.len() as f64).sqrt() as f32;
                    REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if stop_signal_audio.load(Ordering::Relaxed)
                    || pause_signal_audio.load(Ordering::Relaxed)
                {
                    return;
                }

                // Convert to mono
                let mono_samples: Vec<i16> = data
                    .chunks(channels)
                    .map(|frame| {
                        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                        (sum / channels as i32) as i16
                    })
                    .collect();

                // Simple resampling
                let resampled: Vec<i16> = if resample_ratio < 1.0 {
                    let new_len = (mono_samples.len() as f64 * resample_ratio) as usize;
                    (0..new_len)
                        .map(|i| {
                            let src_idx = i as f64 / resample_ratio;
                            let idx0 = src_idx as usize;
                            let idx1 = (idx0 + 1).min(mono_samples.len() - 1);
                            let frac = src_idx - idx0 as f64;
                            let s0 = mono_samples[idx0] as f64;
                            let s1 = mono_samples[idx1] as f64;
                            (s0 + (s1 - s0) * frac) as i16
                        })
                        .collect()
                } else {
                    mono_samples
                };

                if let Ok(mut buf) = audio_buffer_clone.lock() {
                    buf.extend(resampled.iter().cloned());
                }

                // Calculate RMS for volume visualization
                if !resampled.is_empty() {
                    let sum_sq: f64 = resampled
                        .iter()
                        .map(|&s| (s as f64 / 32768.0).powi(2))
                        .sum();
                    let rms = (sum_sq / resampled.len() as f64).sqrt() as f32;
                    REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        )?,
        _ => return Err(anyhow::anyhow!("Unsupported audio format")),
    };

    stream.play()?;
    Ok(stream)
}

/// Start microphone capture
/// Returns the cpal Stream that must be kept alive
pub fn start_mic_capture(
    audio_buffer: Arc<Mutex<Vec<i16>>>,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("No microphone available. Please connect a microphone."))?;
    let config = device.default_input_config()?;

    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let audio_buffer_clone = audio_buffer.clone();
    let target_rate = 16000u32;
    let resample_ratio = target_rate as f64 / sample_rate as f64;
    let stop_signal_audio = stop_signal.clone();
    let pause_signal_audio = pause_signal.clone();
    let err_fn = |err| eprintln!("Audio stream error: {}", err);

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if stop_signal_audio.load(Ordering::Relaxed)
                    || pause_signal_audio.load(Ordering::Relaxed)
                {
                    return;
                }

                let mono_samples: Vec<i16> = data
                    .chunks(channels)
                    .map(|frame| {
                        let sum: f32 = frame.iter().sum();
                        let avg = sum / channels as f32;
                        (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                    })
                    .collect();

                let resampled: Vec<i16> = if resample_ratio < 1.0 {
                    let new_len = (mono_samples.len() as f64 * resample_ratio) as usize;
                    (0..new_len)
                        .map(|i| {
                            let src_idx = i as f64 / resample_ratio;
                            let idx0 = src_idx as usize;
                            let idx1 = (idx0 + 1).min(mono_samples.len() - 1);
                            let frac = src_idx - idx0 as f64;
                            let s0 = mono_samples[idx0] as f64;
                            let s1 = mono_samples[idx1] as f64;
                            (s0 + frac * (s1 - s0)) as i16
                        })
                        .collect()
                } else {
                    mono_samples
                };

                if let Ok(mut buf) = audio_buffer_clone.lock() {
                    buf.extend(resampled.iter().cloned());
                }

                if !resampled.is_empty() {
                    let sum_sq: f64 = resampled
                        .iter()
                        .map(|&s| (s as f64 / 32768.0).powi(2))
                        .sum();
                    let rms = (sum_sq / resampled.len() as f64).sqrt() as f32;
                    REALTIME_RMS.store(rms.to_bits(), Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        )?,
        _ => return Err(anyhow::anyhow!("Unsupported audio format")),
    };

    stream.play()?;
    Ok(stream)
}
