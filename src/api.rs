use anyhow::Result;
use serde::{Deserialize, Serialize};
use image::{ImageBuffer, Rgba, ImageFormat};
use base64::{Engine as _, engine::general_purpose};
use std::io::{Cursor, BufRead, BufReader};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::config::Preset;

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
    let mut png_data = Vec::new();
    image.write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)?;
    let b64_image = general_purpose::STANDARD.encode(&png_data);

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

        let resp = ureq::post(&url)
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
            
            // if use_json_format {
            //    payload_obj["response_format"] = serde_json::json!({ "type": "json_object" });
            // }
            
            payload_obj
        };

        let resp = ureq::post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", groq_api_key))
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else if err_str.contains("400") {
                    anyhow::anyhow!("Groq API 400: Bad request. Check model availability or API request format.")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

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
                    // Parse JSON response (from response_format: json_object)
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
                    // Plain text response
                    full_content = content_str.clone();
                }
                
                on_chunk(&full_content);
            }
        }
    }

    if full_content.is_empty() {
        return Err(anyhow::anyhow!("No content received from API"));
    }

    Ok(full_content)
}

pub fn translate_text_streaming<F>(
    groq_api_key: &str,
    text: String,
    target_lang: String,
    model: String,
    streaming_enabled: bool,
    use_json_format: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    if groq_api_key.trim().is_empty() {
        return Err(anyhow::anyhow!("NO_API_KEY"));
    }

    let prompt = format!(
        "Translate the following text to {}. Output ONLY the translation. Text:\n\n{}",
        target_lang, text
    );

    let payload = if streaming_enabled {
        serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": true
        })
    } else {
        let mut payload_obj = serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": false
        });
        
        if use_json_format {
            payload_obj["response_format"] = serde_json::json!({ "type": "json_object" });
        }
        
        payload_obj
    };

    let resp = ureq::post("https://api.groq.com/openai/v1/chat/completions")
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

    let mut full_content = String::new();

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
                // Parse JSON response (from response_format: json_object)
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
                // Plain text response
                full_content = content_str.clone();
            }
            
            on_chunk(&full_content);
        }
    }

    Ok(full_content)
}

pub fn record_audio_and_transcribe(
    preset: Preset, 
    stop_signal: Arc<AtomicBool>, 
    pause_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND
) {
    let host = cpal::default_host();
    
    // Improved Device Selection logic
    let device = if preset.audio_source == "device" {
        #[cfg(target_os = "windows")]
        {
            // For device audio (loopback), we MUST use the default output device
            // and treat it as an input source.
            match host.default_output_device() {
                Some(d) => {
                    println!("Debug: Selected Output Device for Loopback: {}", d.name().unwrap_or_default());
                    d
                },
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
        println!("Debug: Selected Microphone Input");
        host.default_input_device().expect("No input device available")
    };

    // Try to get a config. For loopback, we often need the default output config
    // but we use it to build an input stream.
    let config = if preset.audio_source == "device" {
        device.default_output_config().or_else(|_| device.default_input_config())
    } else {
        device.default_input_config()
    }.expect("Failed to get device config");

    println!("Audio Config: Channels={}, Sample Rate={}", config.channels(), config.sample_rate().0);

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    // Buffer to hold audio samples
    let audio_buffer = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer_buf = audio_buffer.clone();

    // Stream Setup
    let err_fn = |err| eprintln!("Audio stream error: {}", err);
    
    // We strictly assume the device supports the config we got.
    // For Loopback on Windows with CPAL, we must simply build an input stream on the output device.
    // CPAL handles the WASAPI loopback flag internally if it detects it's an output device used for input.
    let stream_res = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    let mut buf = writer_buf.lock().unwrap();
                    for &sample in data {
                        let s = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        buf.push(s); 
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
                    let mut buf = writer_buf.lock().unwrap();
                    buf.extend_from_slice(data);
                }
            },
            err_fn,
            None
        ),
        _ => panic!("Unsupported audio format"),
    };

    if let Err(e) = stream_res {
        eprintln!("Failed to build stream: {}", e);
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
        return;
    }
    let stream = stream_res.unwrap();

    stream.play().expect("Failed to start audio stream");

    // Wait loop
    while !stop_signal.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Also check if UI died
        if preset.hide_recording_ui {
            // If hidden, we rely purely on stop signal. 
            // BUT user might have closed app via tray.
        } else {
             if !unsafe { IsWindow(overlay_hwnd).as_bool() } {
                return; // Aborted via window close
            }
        }
    }

    drop(stream);

    // Get the recorded audio buffer
    let samples = audio_buffer.lock().unwrap().clone();
    
    if samples.is_empty() {
        println!("Warning: Recorded audio buffer is empty.");
        unsafe {
            PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    // Write to temporary WAV file
    let temp_dir = std::env::temp_dir();
    let wav_path = temp_dir.join(format!("sgt_debug_audio_{}.wav", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()));
    
    let mut writer = hound::WavWriter::create(&wav_path, spec).expect("Failed to create WAV file");
    for sample in &samples {
        writer.write_sample(*sample).expect("Failed to write sample");
    }
    writer.finalize().expect("Failed to finalize WAV file");

    // DEBUG: Log the path so user can listen
    println!("DEBUG: Audio file saved to: {:?}", wav_path);
    // DO NOT DELETE FILE FOR DEBUGGING
    // let _ = std::fs::remove_file(&wav_path);

    // Read WAV file for upload
    let wav_data = std::fs::read(&wav_path).expect("Failed to read WAV file");
    
    // Determine API endpoint and model
    let model_config = crate::model_config::get_model_by_id(&preset.model);
    let model_name = model_config.map(|m| m.full_name.clone()).unwrap_or_else(|| "whisper-large-v3-turbo".to_string());
    
    // Get API key from config
    let api_key = {
        let app = crate::APP.lock().unwrap();
        app.config.api_key.clone()
    };

    if api_key.trim().is_empty() {
        eprintln!("Error: No API Key found.");
        unsafe {
            PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    // Upload to Whisper API
    let transcription_result = upload_audio_to_whisper(&api_key, &model_name, wav_data);
    
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
    let response = ureq::post("https://api.groq.com/openai/v1/audio/transcriptions")
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
        .send_bytes(&body)
        .map_err(|e| anyhow::anyhow!("API request failed: {}", e))?;
    
    // Parse response
    let json: serde_json::Value = response.into_json()
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;
    
    let text = json.get("text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("No text in response"))?;
    
    Ok(text.to_string())
}
