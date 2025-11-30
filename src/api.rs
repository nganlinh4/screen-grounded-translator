use anyhow::Result;
use serde::{Deserialize, Serialize};
use image::{ImageBuffer, Rgba};
use base64::{Engine as _, engine::general_purpose};
use std::io::{Cursor, BufRead, BufReader};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::config::Preset;
use crate::model_config::get_model_by_id;
use crate::APP;
use crate::overlay::result::RefineContext;

#[derive(Serialize, Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Serialize, Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Serialize, Deserialize)]
struct Delta {
    content: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Serialize, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    content: String,
}

lazy_static::lazy_static! {
    static ref UREQ_AGENT: ureq::Agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(30))
        .timeout_write(std::time::Duration::from_secs(30))
        .build();
}

pub fn translate_image_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    prompt: String,
    model: String,
    provider: String,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    streaming_enabled: bool,
    use_json_format: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    // Reverted resizing logic as it caused issues with main overlay capture
    let mut image_data = Vec::new();
    image.write_to(&mut Cursor::new(&mut image_data), image::ImageFormat::Png)?;
    let b64_image = general_purpose::STANDARD.encode(&image_data);

    let mut full_content = String::new();

    if provider == "google" {
        // Gemini API
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
        let url = if streaming_enabled {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
                model, method
            )
        } else {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
            )
        };

        let payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [
                    { "text": prompt },
                    {
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": b64_image
                        }
                    }
                ]
            }]
        });

        let resp = UREQ_AGENT.post(&url)
            .set("x-goog-api-key", gemini_api_key)
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

        if streaming_enabled {
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
        } else {
            let chat_resp: serde_json::Value = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
                if let Some(first_choice) = candidates.first() {
                    if let Some(parts) = first_choice.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                        full_content = parts.iter()
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<String>();
                        
                        on_chunk(&full_content);
                    }
                }
            }
        }
    } else {
        // Groq API (default)
        if groq_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let payload = if streaming_enabled {
            serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                        ]
                    }
                ],
                "temperature": 0.1,
                "max_completion_tokens": 1024,
                "stream": true
            })
        } else {
            let payload_obj = serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                        ]
                    }
                ],
                "temperature": 0.1,
                "max_completion_tokens": 1024,
                "stream": false
            });
            
            payload_obj
        };

        let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", groq_api_key))
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else if err_str.contains("400") {
                    anyhow::anyhow!("Groq API 400: Bad request. Check model availability or API request format.")
                } else {
                    anyhow::anyhow!("Error: https://api.groq.com/openai/v1/chat/completions: {}", err_str)
                }
            })?;

        // --- CAPTURE RATE LIMITS ---
        if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
             let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
             let usage_str = format!("{} / {}", remaining, limit);
             
             if let Ok(mut app) = APP.lock() {
                 app.model_usage_stats.insert(model.clone(), usage_str);
             }
        }
        // ---------------------------

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            for line in reader.lines() {
                let line = line?;

                if line.starts_with("data: ") {
                    let data = &line[6..];

                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            if let Some(content) = chunk.choices.get(0)
                                .and_then(|c| c.delta.content.as_ref()) {
                                full_content.push_str(content);
                                on_chunk(content);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                let content_str = &choice.message.content;
                
                if use_json_format {
                    if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(content_str) {
                        if let Some(translation) = json_obj.get("translation").and_then(|v| v.as_str()) {
                            full_content = translation.to_string();
                        } else {
                            full_content = content_str.clone();
                        }
                    } else {
                        full_content = content_str.clone();
                    }
                } else {
                    full_content = content_str.clone();
                }
                
                on_chunk(&full_content);
            }
        }
    }

    Ok(full_content)
}

pub fn translate_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    text: String,
    target_lang: String,
    model: String,
    provider: String,
    streaming_enabled: bool,
    use_json_format: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let mut full_content = String::new();
    let prompt = format!(
        "Translate the following text to {}. Output ONLY the translation. Text:\n\n{}",
        target_lang, text
    );

    if provider == "google" {
        // --- GEMINI TEXT API ---
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
        let url = if streaming_enabled {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
                model, method
            )
        } else {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
            )
        };

        let payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }]
        });

        let resp = UREQ_AGENT.post(&url)
            .set("x-goog-api-key", gemini_api_key)
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("Gemini Text API Error: {}", err_str)
                }
            })?;

        if streaming_enabled {
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
        } else {
            let chat_resp: serde_json::Value = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
                if let Some(first_choice) = candidates.first() {
                    if let Some(parts) = first_choice.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                        full_content = parts.iter()
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<String>();
                        on_chunk(&full_content);
                    }
                }
            }
        }

    } else {
        // --- GROQ API (Default) ---
        if groq_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY"));
        }

        let payload = if streaming_enabled {
            serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": true
            })
        } else {
            let mut payload_obj = serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": false
            });
            
            if use_json_format {
                payload_obj["response_format"] = serde_json::json!({ "type": "json_object" });
            }
            
            payload_obj
        };

        let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", groq_api_key))
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

        // --- CAPTURE RATE LIMITS ---
        if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
             let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
             let usage_str = format!("{} / {}", remaining, limit);
             
             if let Ok(mut app) = APP.lock() {
                 app.model_usage_stats.insert(model.clone(), usage_str);
             }
        }
        // ---------------------------

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            
            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" { break; }
                    
                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            if let Some(content) = chunk.choices.get(0)
                                .and_then(|c| c.delta.content.as_ref()) {
                                full_content.push_str(content);
                                on_chunk(content);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                let content_str = &choice.message.content;
                
                if use_json_format {
                    if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(content_str) {
                        if let Some(translation) = json_obj.get("translation").and_then(|v| v.as_str()) {
                            full_content = translation.to_string();
                        } else {
                            full_content = content_str.clone();
                        }
                    } else {
                        full_content = content_str.clone();
                    }
                } else {
                    full_content = content_str.clone();
                }
                
                on_chunk(&full_content);
            }
        }
    }

    Ok(full_content)
}

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
        return Err(anyhow::anyhow!("NO_API_KEY"));
    }

    let b64_audio = general_purpose::STANDARD.encode(&wav_data);
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
        model
    );

    let payload = serde_json::json!({
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

pub fn record_audio_and_transcribe(
    preset: Preset, 
    stop_signal: Arc<AtomicBool>, 
    pause_signal: Arc<AtomicBool>,
    // FIX: New argument to handle user cancellation
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND
) {
    // FIX 5: Host Selection (WASAPI for loopback, default for mic)
    #[cfg(target_os = "windows")]
    let host = if preset.audio_source == "device" {
        cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host())
    } else {
        cpal::default_host()
    };
    #[cfg(not(target_os = "windows"))]
    let host = cpal::default_host();

    // Improved Device Selection logic
    let device = if preset.audio_source == "device" {
        #[cfg(target_os = "windows")]
        {
            // For device audio (loopback), prefer WASAPI default output device
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

    // Robust Audio Configuration
    let config = if preset.audio_source == "device" {
        // Try output config first for loopback accuracy
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

    // FIX 2: Use channel instead of Mutex for better lock-free audio handling
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Stream Setup
    let err_fn = |err| eprintln!("Audio stream error: {}", err);
    
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
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        return;
    }
    let stream = stream_res.unwrap();

    stream.play().expect("Failed to start audio stream");

    let mut collected_samples: Vec<f32> = Vec::new();

    // Wait loop with channel draining
    while !stop_signal.load(Ordering::SeqCst) {
        // Drain channel
        while let Ok(chunk) = rx.try_recv() {
            collected_samples.extend(chunk);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Also check if UI died
        if !preset.hide_recording_ui {
             if !unsafe { IsWindow(overlay_hwnd).as_bool() } {
                return; // Aborted via window close
            }
        }
    }

    drop(stream);

    // FIX: Check if we should ABORT instead of submitting
    if abort_signal.load(Ordering::SeqCst) {
        println!("Audio recording aborted by user.");
        unsafe {
            if IsWindow(overlay_hwnd).as_bool() {
                 PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    // Final drain of any remaining samples
    while let Ok(chunk) = rx.try_recv() {
        collected_samples.extend(chunk);
    }

    // Convert f32 samples to i16 for WAV
    let samples: Vec<i16> = collected_samples.iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    
    if samples.is_empty() {
        println!("Warning: Recorded audio buffer is empty.");
        unsafe {
            PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    // OPTIMIZATION: Write directly to in-memory buffer instead of disk
    let mut wav_cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut wav_cursor, spec).expect("Failed to create memory writer");
        for sample in &samples {
            writer.write_sample(*sample).expect("Failed to write sample");
        }
        writer.finalize().expect("Failed to finalize WAV");
    }
    let wav_data = wav_cursor.into_inner();
    
    // Determine API endpoint, model, and provider
    let model_config = get_model_by_id(&preset.model);
    let model_config = model_config.expect("Model config not found for preset model");
    let model_name = model_config.full_name.clone();
    let provider = model_config.provider.clone();
    
    // Get API keys from config
    let (groq_api_key, gemini_api_key) = {
        let app = crate::APP.lock().unwrap();
        (app.config.api_key.clone(), app.config.gemini_api_key.clone())
    };

    // Prepare Prompt - replace all {languageN} with actual languages (same as image processing flow)
    let mut final_prompt = preset.prompt.clone();
    
    // Replace numbered language tags
    for (key, value) in &preset.language_vars {
        let pattern = format!("{{{}}}", key); // e.g., "{language1}"
        final_prompt = final_prompt.replace(&pattern, value);
    }
    
    // Backward compatibility: also replace old {language} tag
    final_prompt = final_prompt.replace("{language}", &preset.selected_language);
    
    // --- LOGIC SPLIT: Groq (Whisper API) vs Google (Multimodal Chat API) ---
    let transcription_result = if provider == "groq" {
        if groq_api_key.trim().is_empty() {
            Err(anyhow::anyhow!("NO_API_KEY"))
        } else {
            upload_audio_to_whisper(&groq_api_key, &model_name, wav_data)
        }
    } else if provider == "google" {
        // Must use the multimodal approach
        if gemini_api_key.trim().is_empty() {
            Err(anyhow::anyhow!("NO_API_KEY"))
        } else {
            // Pass the prompt for Gemini models with variable substitution applied
            transcribe_audio_gemini(&gemini_api_key, final_prompt, model_name, wav_data, |_| {})
        }
    } else {
        Err(anyhow::anyhow!("Unsupported audio provider: {}", provider))
    };
    
    // Handle Result showing
    unsafe {
        // If hidden UI, we can't post message to it to close itself, but it might not exist.
        if IsWindow(overlay_hwnd).as_bool() {
             PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    match transcription_result {
        Ok(transcription_text) => {
            let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
            
            // Logic to position windows based on retranslate need
            let (rect, retranslate_rect) = if preset.retranslate {
                // Split screen
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
                // Center
                let w = 700;
                let h = 300;
                let x = (screen_w - w) / 2;
                let y = (screen_h - h) / 2;
                (RECT { left: x, top: y, right: x + w, bottom: y + h }, None)
            };

            crate::overlay::process::show_audio_result(preset, transcription_text, rect, retranslate_rect);
        },
        Err(e) => {
            eprintln!("Transcription error: {}", e);
        }
    }
}

// NEW: Refinement API with model-aware and context-aware handling
pub fn refine_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    context: RefineContext,
    previous_text: String,
    user_prompt: String,
    original_model_id: &str,
    original_provider: &str,
    streaming_enabled: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    // 1. CONSTRUCT SIMPLE PROMPT
    // Removed "Previous Output (context):" labeling which confuses small models.
    let final_prompt = format!(
        "Content:\n{}\n\nInstruction:\n{}\n\nOutput ONLY the result.",
        previous_text, user_prompt
    );

    // 2. Determine the Base Model ID/Name and Provider we WANT to use
    let (mut target_id_or_name, mut target_provider) = match context {
        RefineContext::Image(_) => {
            // For images, we try to stick to the original vision model.
            (original_model_id.to_string(), original_provider.to_string())
        },
        _ => {
            // RefineContext::None (Retranslate) or RefineContext::Audio (Transcript Refinement)
            // Force smart text model: prioritize Google if key present, else Groq
            if !gemini_api_key.trim().is_empty() {
                 ("gemini-flash-lite".to_string(), "google".to_string()) 
            } else if !groq_api_key.trim().is_empty() {
                 ("text_accurate_kimi".to_string(), "groq".to_string()) 
            } else {
                 (original_model_id.to_string(), original_provider.to_string())
            }
        }
    };

    // 3. Resolve to Full Name (API-ready)
    // This converts IDs like "gemini-flash-lite" -> "gemini-flash-lite-latest"
    if let Some(conf) = get_model_by_id(&target_id_or_name) {
        target_id_or_name = conf.full_name;
        target_provider = conf.provider; // Also ensure provider matches config
    }
    
    // Helper closure to execute Text-Only generation using final_prompt
    let mut exec_text_only = |p_model: String, p_provider: String| -> Result<String> {
        let mut full_content = String::new();

        if p_provider == "google" {
             if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_GEMINI_KEY")); }
             
             let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
             let url = if streaming_enabled {
                 format!("https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse", p_model, method)
             } else {
                 format!("https://generativelanguage.googleapis.com/v1beta/models/{}:{}", p_model, method)
             };

             let payload = serde_json::json!({
                 "contents": [{ "role": "user", "parts": [{ "text": final_prompt }] }]
             });

             let resp = UREQ_AGENT.post(&url)
                 .set("x-goog-api-key", gemini_api_key)
                 .send_json(payload)
                 .map_err(|e| anyhow::anyhow!("Gemini Refine Error: {}", e))?;

             if streaming_enabled {
                 let reader = BufReader::new(resp.into_reader());
                 for line in reader.lines() {
                     let line = line?;
                     if line.starts_with("data: ") {
                         let json_str = &line["data: ".len()..];
                         if json_str.trim() == "[DONE]" { break; }
                         if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                             if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                                 if let Some(first) = candidates.first() {
                                     if let Some(parts) = first.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                         if let Some(p) = parts.first() {
                                             if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                                                 full_content.push_str(t);
                                                 on_chunk(t);
                                             }
                                         }
                                     }
                                 }
                             }
                         }
                     }
                 }
             } else {
                 let json: serde_json::Value = resp.into_json()?;
                 if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
                     if let Some(first) = candidates.first() {
                         if let Some(parts) = first.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                            full_content = parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect::<String>();
                            on_chunk(&full_content);
                         }
                     }
                 }
             }
        } else {
            // Groq
            if groq_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY")); }
            
            let payload = serde_json::json!({
                "model": p_model,
                "messages": [{ "role": "user", "content": final_prompt }],
                "stream": streaming_enabled
            });
            
            let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
                .set("Authorization", &format!("Bearer {}", groq_api_key))
                .send_json(payload)
                .map_err(|e| anyhow::anyhow!("Groq Refine Error: {}", e))?;

            // Capture Rate Limits
            if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
                 let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
                 let usage_str = format!("{} / {}", remaining, limit);
                 if let Ok(mut app) = APP.lock() {
                     app.model_usage_stats.insert(p_model.clone(), usage_str);
                 }
            }

            if streaming_enabled {
                let reader = BufReader::new(resp.into_reader());
                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("data: ") {
                         let data = &line[6..];
                         if data == "[DONE]" { break; }
                         if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                             if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                                 full_content.push_str(content);
                                 on_chunk(content);
                             }
                         }
                    }
                }
            } else {
                 let json: ChatCompletionResponse = resp.into_json()?;
                 if let Some(choice) = json.choices.first() {
                     full_content = choice.message.content.clone();
                     on_chunk(&full_content);
                 }
            }
        }
        
        Ok(full_content)
    };

    match context {
        RefineContext::Image(img_bytes) => {
            if target_provider == "google" {
                if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_GEMINI_KEY")); }
                let img = image::load_from_memory(&img_bytes)?.to_rgba8();
                translate_image_streaming(groq_api_key, gemini_api_key, final_prompt, target_id_or_name, target_provider, img, streaming_enabled, false, on_chunk)
            } else {
                // Groq/Llama Vision
                if groq_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY")); }
                let img = image::load_from_memory(&img_bytes)?.to_rgba8();
                translate_image_streaming(groq_api_key, gemini_api_key, final_prompt, target_id_or_name, target_provider, img, streaming_enabled, false, on_chunk)
            }
        },
        RefineContext::Audio(wav_bytes) => {
            if target_provider == "google" {
                if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_GEMINI_KEY")); }
                transcribe_audio_gemini(gemini_api_key, final_prompt, target_id_or_name, wav_bytes, on_chunk)
            } else {
                // Groq Audio (Whisper) - Fallback to TEXT refinement using the helper
                // because Whisper cannot do chat refinement on audio.
                exec_text_only(target_id_or_name, target_provider)
            }
        },
        RefineContext::None => {
            // Text Only - Use the helper to execute with final_prompt
            exec_text_only(target_id_or_name, target_provider)
        }
    }
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
