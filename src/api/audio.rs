use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use std::io::{Cursor, BufRead, BufReader};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::config::Preset;
use crate::model_config::get_model_by_id;
use crate::APP;
use super::client::UREQ_AGENT;

pub fn transcribe_audio_gemini<F>(
    gemini_api_key: &str,
    prompt: String,
    model: String,
    wav_data: Vec<u8>,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    if gemini_api_key.trim().is_empty() {
        return Err(anyhow::anyhow!("NO_API_KEY:google"));
    }

    let b64_audio = general_purpose::STANDARD.encode(&wav_data);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
        model
    );

    let mut payload = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [
                { "text": prompt },
                {
                    "inline_data": {
                        "mime_type": "audio/wav",
                        "data": b64_audio
                    }
                }
            ]
        }]
    });
    
    // Add grounding tools for all models except gemma-3-27b-it
    if !model.contains("gemma-3-27b-it") {
        payload["tools"] = serde_json::json!([
            { "url_context": {} },
            { "google_search": {} }
        ]);
    }

    let resp = UREQ_AGENT.post(&url)
        .set("x-goog-api-key", gemini_api_key)
        .send_json(payload)
        .map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("401") || err_str.contains("403") {
                anyhow::anyhow!("INVALID_API_KEY")
            } else {
                anyhow::anyhow!("Gemini Audio API Error: {}", err_str)
            }
        })?;

    let mut full_content = String::new();
    let reader = BufReader::new(resp.into_reader());

    for line in reader.lines() {
        let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
        if line.starts_with("data: ") {
            let json_str = &line["data: ".len()..];
            if json_str.trim() == "[DONE]" { break; }

            if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                    if let Some(first_candidate) = candidates.first() {
                        if let Some(parts) = first_candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                            if let Some(first_part) = parts.first() {
                                if let Some(text) = first_part.get("text").and_then(|t| t.as_str()) {
                                    full_content.push_str(text);
                                    on_chunk(text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if full_content.is_empty() {
        return Err(anyhow::anyhow!("No content received from Gemini Audio API"));
    }
    
    Ok(full_content)
}

fn upload_audio_to_whisper(api_key: &str, model: &str, audio_data: Vec<u8>) -> anyhow::Result<String> {
    // Create multipart form data
    let boundary = format!("----SGTBoundary{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis());
    
    let mut body = Vec::new();
    
    // Add model field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"model\"\r\n\r\n");
    body.extend_from_slice(model.as_bytes());
    body.extend_from_slice(b"\r\n");
    
    // Add file field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n");
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(&audio_data);
    body.extend_from_slice(b"\r\n");
    
    // End boundary
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    
    // Make API request
    let response = UREQ_AGENT.post("https://api.groq.com/openai/v1/audio/transcriptions")
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
        .send_bytes(&body)
        .map_err(|e| anyhow::anyhow!("API request failed: {}", e))?;
    
    // --- CAPTURE RATE LIMITS ---
    if let Some(remaining) = response.header("x-ratelimit-remaining-requests") {
         let limit = response.header("x-ratelimit-limit-requests").unwrap_or("?");
         let usage_str = format!("{} / {}", remaining, limit);
         if let Ok(mut app) = APP.lock() {
             app.model_usage_stats.insert(model.to_string(), usage_str);
         }
    }
    // ---------------------------

    // Parse response
    let json: serde_json::Value = response.into_json()
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;
    
    let text = json.get("text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("No text in response"))?;
    
    Ok(text.to_string())
}

pub fn record_audio_and_transcribe(
    preset: Preset, 
    stop_signal: Arc<AtomicBool>, 
    pause_signal: Arc<AtomicBool>,
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND
) {
    #[cfg(target_os = "windows")]
    let host = if preset.audio_source == "device" {
        cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host())
    } else {
        cpal::default_host()
    };
    #[cfg(not(target_os = "windows"))]
    let host = cpal::default_host();

    let device = if preset.audio_source == "device" {
        #[cfg(target_os = "windows")]
        {
            match host.default_output_device() {
                Some(d) => d,
                None => {
                    eprintln!("Error: No default output device found for loopback.");
                    host.default_input_device().expect("No input device available")
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            host.default_input_device().expect("No input device available")
        }
    } else {
        host.default_input_device().expect("No input device available")
    };

    let config = if preset.audio_source == "device" {
        match device.default_output_config() {
            Ok(c) => c,
            Err(_) => {
                 device.default_input_config().expect("Failed to get audio config")
            }
        }
    } else {
        device.default_input_config().expect("Failed to get audio config")
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    let err_fn = |err| eprintln!("Audio stream error: {}", err);
    
    // Threshold for "meaningful audio" - above this RMS means mic is truly receiving sound
    const WARMUP_RMS_THRESHOLD: f32 = 0.001;
    
    let stream_res = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    let _ = tx.send(data.to_vec());
                    let mut rms = 0.0;
                    for &x in data { rms += x * x; }
                    rms = (rms / data.len() as f32).sqrt();
                    crate::overlay::recording::update_audio_viz(rms);
                    
                    // Signal warmup complete when we get meaningful audio
                    if rms > WARMUP_RMS_THRESHOLD {
                        crate::overlay::recording::AUDIO_WARMUP_COMPLETE.store(true, Ordering::SeqCst);
                    }
                }
            },
            err_fn,
            None
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let _ = tx.send(f32_data);
                    let mut rms = 0.0;
                    for &x in data { let f = x as f32 / i16::MAX as f32; rms += f * f; }
                    rms = (rms / data.len() as f32).sqrt();
                    crate::overlay::recording::update_audio_viz(rms);
                    
                    // Signal warmup complete when we get meaningful audio
                    if rms > WARMUP_RMS_THRESHOLD {
                        crate::overlay::recording::AUDIO_WARMUP_COMPLETE.store(true, Ordering::SeqCst);
                    }
                }
            },
            err_fn,
            None
        ),
        _ => {
            eprintln!("Unsupported audio sample format: {:?}", config.sample_format());
             Err(cpal::BuildStreamError::StreamConfigNotSupported)
        },
    };

    if let Err(e) = stream_res {
        eprintln!("Failed to build stream: {}", e);
        unsafe { let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
        return;
    }
    let stream = stream_res.unwrap();

    if let Err(e) = stream.play() {
        eprintln!("Failed to play stream: {}", e);
        unsafe { let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
        return;
    }

    let mut collected_samples: Vec<f32> = Vec::new();

    // --- AUTO-STOP LOGIC STATE ---
    // Only active when preset.auto_stop_recording is true
    let auto_stop_enabled = preset.auto_stop_recording;
    let mut has_spoken = false;           // True once user starts speaking
    let mut first_speech_time: Option<std::time::Instant> = None;  // When user first spoke
    let mut last_active_time = std::time::Instant::now();
    
    // Thresholds tuned for typical speech vs silence
    const NOISE_THRESHOLD: f32 = 0.015;   // RMS above this = speech
    const SILENCE_LIMIT_MS: u128 = 800;   // ms of silence after speech to trigger stop
    const MIN_RECORDING_MS: u128 = 2000;  // Minimum 2 seconds after first speech

    while !stop_signal.load(Ordering::SeqCst) {
        while let Ok(chunk) = rx.try_recv() {
            collected_samples.extend(chunk);
        }
        
        // --- AUTO-STOP: Check volume and silence duration ---
        if auto_stop_enabled && !stop_signal.load(Ordering::Relaxed) {
            // Get current RMS from the shared atomic
            let rms_bits = crate::overlay::recording::CURRENT_RMS.load(Ordering::Relaxed);
            let current_rms = f32::from_bits(rms_bits);
            
            if current_rms > NOISE_THRESHOLD {
                // User is speaking (volume above threshold)
                if !has_spoken {
                    first_speech_time = Some(std::time::Instant::now());
                }
                has_spoken = true;
                last_active_time = std::time::Instant::now();
            } else if has_spoken {
                // User was speaking but now is silent
                // Check minimum recording duration first
                let recording_duration = first_speech_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);
                if recording_duration >= MIN_RECORDING_MS {
                    let silence_duration = last_active_time.elapsed().as_millis();
                    if silence_duration > SILENCE_LIMIT_MS {
                        // Silence exceeded limit after speech - auto-stop!
                        stop_signal.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
        
        std::thread::sleep(std::time::Duration::from_millis(50));
        if !preset.hide_recording_ui {
             if !unsafe { IsWindow(Some(overlay_hwnd)).as_bool() } {
                return;
            }
        }
    }

    drop(stream);

    if abort_signal.load(Ordering::SeqCst) {
        println!("Audio recording aborted by user.");
        unsafe {
            if IsWindow(Some(overlay_hwnd)).as_bool() {
                 let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    while let Ok(chunk) = rx.try_recv() {
        collected_samples.extend(chunk);
    }

    let samples: Vec<i16> = collected_samples.iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    
    if samples.is_empty() {
        println!("Warning: Recorded audio buffer is empty.");
        unsafe {
            let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    let mut wav_cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut wav_cursor, spec).expect("Failed to create memory writer");
        for sample in &samples {
            writer.write_sample(*sample).expect("Failed to write sample");
        }
        writer.finalize().expect("Failed to finalize WAV");
    }
    let wav_data = wav_cursor.into_inner();

    // For MASTER presets, show the wheel BEFORE transcription to get the actual preset
    let working_preset = if preset.is_master {
        // Get cursor position for wheel center (use center of screen)
        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let cursor_pos = POINT { x: screen_w / 2, y: screen_h / 2 };
        
        // Show preset wheel - filter by audio source
        let audio_mode = Some(preset.audio_source.as_str());
        let selected = crate::overlay::preset_wheel::show_preset_wheel("audio", audio_mode, cursor_pos);
        
        if let Some(idx) = selected {
            // Get the selected preset from config AND update active_preset_idx
            let mut app = crate::APP.lock().unwrap();
            // CRITICAL: Update active_preset_idx so auto_paste logic works!
            app.config.active_preset_idx = idx;
            app.config.presets[idx].clone()
        } else {
            // User dismissed wheel - close overlay and cancel
            unsafe {
                if IsWindow(Some(overlay_hwnd)).as_bool() {
                    let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            return;
        }
    } else {
        preset.clone()
    };
    
    // Get audio block (Block 0) - use new block-based structure
    let audio_block = match working_preset.blocks.first() {
        Some(b) => b.clone(),
        None => {
            eprintln!("Error: Audio preset has no blocks configured");
            unsafe { let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
            return;
        }
    };
    
    let model_config = get_model_by_id(&audio_block.model);
    let model_config = match model_config {
        Some(c) => c,
        None => {
            eprintln!("Error: Model config not found for audio model: {}", audio_block.model);
            unsafe { let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
            return;
        }
    };
    let model_name = model_config.full_name.clone();
    let provider = model_config.provider.clone();
    
    let (groq_api_key, gemini_api_key) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone())
    };

    // Use block's prompt and language settings
    let mut final_prompt = audio_block.prompt.clone();
    
    for (key, value) in &audio_block.language_vars {
        let pattern = format!("{{{}}}", key);
        final_prompt = final_prompt.replace(&pattern, value);
    }
    
    // Fallback: if {language1} is still in prompt but not in language_vars, use selected_language
    if final_prompt.contains("{language1}") && !audio_block.language_vars.contains_key("language1") {
        final_prompt = final_prompt.replace("{language1}", &audio_block.selected_language);
    }
    
    final_prompt = final_prompt.replace("{language}", &audio_block.selected_language);
    
    // Clone wav_data for history saving
    let wav_data_for_history = wav_data.clone();
    
    let transcription_result = if provider == "groq" {
        if groq_api_key.trim().is_empty() {
            Err(anyhow::anyhow!("NO_API_KEY:groq"))
        } else {
            upload_audio_to_whisper(&groq_api_key, &model_name, wav_data)
        }
    } else if provider == "google" {
        if gemini_api_key.trim().is_empty() {
            Err(anyhow::anyhow!("NO_API_KEY:google"))
        } else {
            transcribe_audio_gemini(&gemini_api_key, final_prompt, model_name, wav_data, |_| {})
        }
    } else {
        Err(anyhow::anyhow!("Unsupported audio provider: {}", provider))
    };
    
    // DON'T close overlay here - pass it to chain processing instead
    // The chain will keep the recording animation until the first visible block appears

    // Check if user aborted during the API call
    if abort_signal.load(Ordering::SeqCst) {
        unsafe {
            if IsWindow(Some(overlay_hwnd)).as_bool() {
                 let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    match transcription_result {
        Ok(transcription_text) => {
            
            // SAVE HISTORY
            {
                let app = crate::APP.lock().unwrap();
                app.history.save_audio(wav_data_for_history, transcription_text.clone());
            }
            
            // Use working_preset (already resolved by wheel for MASTER presets)
            let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
            
            // Use block count to determine layout - multiple blocks means multi-window layout
            let has_multiple_blocks = working_preset.blocks.len() > 1;
            let (rect, retranslate_rect) = if has_multiple_blocks {
                let w = 600;
                let h = 300;
                let gap = 20;
                let total_w = w * 2 + gap;
                let start_x = (screen_w - total_w) / 2;
                let y = (screen_h - h) / 2;
                
                (
                    RECT { left: start_x, top: y, right: start_x + w, bottom: y + h },
                    Some(RECT { left: start_x + w + gap, top: y, right: start_x + w + gap + w, bottom: y + h })
                )
            } else {
                let w = 700;
                let h = 300;
                let x = (screen_w - w) / 2;
                let y = (screen_h - h) / 2;
                (RECT { left: x, top: y, right: x + w, bottom: y + h }, None)
            };

            // Pass overlay_hwnd to chain processing - it will be kept alive until first visible block
            crate::overlay::process::show_audio_result(working_preset, transcription_text, rect, retranslate_rect, overlay_hwnd);
        },
        Err(e) => {
            eprintln!("Transcription error: {}", e);
            // Close overlay on error
            unsafe {
                if IsWindow(Some(overlay_hwnd)).as_bool() {
                     let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
}
