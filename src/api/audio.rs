use super::client::UREQ_AGENT;
use crate::config::Preset;
use crate::model_config::{get_model_by_id, model_is_non_llm};
use crate::APP;
use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::{BufRead, BufReader, Cursor};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

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

    let resp = UREQ_AGENT
        .post(&url)
        .header("x-goog-api-key", gemini_api_key)
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
    let reader = BufReader::new(resp.into_body().into_reader());

    for line in reader.lines() {
        let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
        if line.starts_with("data: ") {
            let json_str = &line["data: ".len()..];
            if json_str.trim() == "[DONE]" {
                break;
            }

            if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                    if let Some(first_candidate) = candidates.first() {
                        if let Some(parts) = first_candidate
                            .get("content")
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.as_array())
                        {
                            if let Some(first_part) = parts.first() {
                                if let Some(text) = first_part.get("text").and_then(|t| t.as_str())
                                {
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

/// Transcribe audio using Gemini Live WebSocket with INPUT transcription
/// (transcribes what was recorded, not AI response)
fn transcribe_with_gemini_live_input(api_key: &str, wav_data: Vec<u8>) -> anyhow::Result<String> {
    use crate::api::realtime_audio::websocket::{
        connect_websocket, parse_input_transcription, send_audio_chunk, send_setup_message,
        set_socket_nonblocking, set_socket_short_timeout,
    };
    use crate::overlay::recording::AUDIO_INITIALIZING;
    use std::time::{Duration, Instant};

    println!(
        "[GeminiLiveInput] Starting transcription, WAV data size: {} bytes",
        wav_data.len()
    );

    // Signal that we're initializing (WebSocket connection)
    AUDIO_INITIALIZING.store(true, Ordering::SeqCst);

    // Connect and setup WebSocket
    println!("[GeminiLiveInput] Connecting to WebSocket...");
    let mut socket = match connect_websocket(api_key) {
        Ok(s) => {
            println!("[GeminiLiveInput] WebSocket connected successfully");
            s
        }
        Err(e) => {
            println!("[GeminiLiveInput] WebSocket connection failed: {}", e);
            AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    println!("[GeminiLiveInput] Sending setup message...");
    if let Err(e) = send_setup_message(&mut socket) {
        println!("[GeminiLiveInput] Setup message failed: {}", e);
        AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
        return Err(e);
    }

    // Set short timeout for setup phase
    if let Err(e) = set_socket_short_timeout(&mut socket) {
        AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
        return Err(e);
    }

    // Wait for setup complete
    println!("[GeminiLiveInput] Waiting for setupComplete...");
    let setup_start = Instant::now();
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                let msg = msg.as_str();
                println!(
                    "[GeminiLiveInput] Received text message: {}",
                    &msg[..msg.len().min(200)]
                );
                if msg.contains("setupComplete") {
                    println!("[GeminiLiveInput] Setup complete received!");
                    break;
                }
                if msg.contains("error") || msg.contains("Error") {
                    println!("[GeminiLiveInput] Server error: {}", msg);
                    AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
                    return Err(anyhow::anyhow!("Server returned error: {}", msg));
                }
            }
            Ok(tungstenite::Message::Binary(data)) => {
                println!(
                    "[GeminiLiveInput] Received binary message: {} bytes",
                    data.len()
                );
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if text.contains("setupComplete") {
                        println!("[GeminiLiveInput] Setup complete (from binary)!");
                        break;
                    }
                }
            }
            Ok(other) => {
                println!("[GeminiLiveInput] Received other message type: {:?}", other);
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if setup_start.elapsed() > Duration::from_secs(30) {
                    println!("[GeminiLiveInput] Setup timeout after 30s");
                    AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
                    return Err(anyhow::anyhow!("Setup timeout"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                println!("[GeminiLiveInput] Socket error during setup: {}", e);
                AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
                return Err(e.into());
            }
        }
    }

    // Setup complete - switch to non-blocking mode and clear initializing state
    if let Err(e) = set_socket_nonblocking(&mut socket) {
        AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
        return Err(e);
    }
    AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
    // Now signal warmup complete so UI shows recording state
    crate::overlay::recording::AUDIO_WARMUP_COMPLETE.store(true, Ordering::SeqCst);

    // Extract PCM samples from WAV data
    println!("[GeminiLiveInput] Extracting PCM samples from WAV...");
    let pcm_samples = extract_pcm_from_wav(&wav_data)?;
    println!(
        "[GeminiLiveInput] Extracted {} PCM samples",
        pcm_samples.len()
    );

    // Send audio in chunks (16kHz, 100ms chunks = 1600 samples)
    let chunk_size = 1600;
    let mut accumulated_text = String::new();
    let mut offset = 0;
    let mut chunks_sent = 0;
    let mut transcripts_received = 0;

    println!("[GeminiLiveInput] Sending audio chunks...");
    while offset < pcm_samples.len() {
        let end = (offset + chunk_size).min(pcm_samples.len());
        let chunk = &pcm_samples[offset..end];

        if send_audio_chunk(&mut socket, chunk).is_err() {
            println!(
                "[GeminiLiveInput] Failed to send audio chunk at offset {}",
                offset
            );
            break;
        }
        chunks_sent += 1;
        offset = end;

        // Read any available transcriptions
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Text(msg)) => {
                    let msg = msg.as_str();
                    println!(
                        "[GeminiLiveInput] Message while sending: {}",
                        &msg[..msg.len().min(300)]
                    );
                    if let Some(transcript) = parse_input_transcription(msg) {
                        if !transcript.is_empty() {
                            println!("[GeminiLiveInput] Got transcript: '{}'", transcript);
                            transcripts_received += 1;
                            accumulated_text.push_str(&transcript);
                        }
                    }
                }
                Ok(tungstenite::Message::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data.to_vec()) {
                        if let Some(transcript) = parse_input_transcription(&text) {
                            if !transcript.is_empty() {
                                println!(
                                    "[GeminiLiveInput] Got transcript (binary): '{}'",
                                    transcript
                                );
                                transcripts_received += 1;
                                accumulated_text.push_str(&transcript);
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break; // No more messages available, continue sending
                }
                Err(_) => break,
            }
        }

        // Small delay between chunks to not overwhelm the connection
        std::thread::sleep(Duration::from_millis(10));
    }

    println!(
        "[GeminiLiveInput] Sent {} chunks, waiting 2s for final transcriptions...",
        chunks_sent
    );

    // Wait 2 seconds after sending all audio for final transcriptions
    let conclude_start = Instant::now();
    let conclude_duration = Duration::from_secs(2);

    while conclude_start.elapsed() < conclude_duration {
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                let msg = msg.as_str();
                println!(
                    "[GeminiLiveInput] Message in conclude phase: {}",
                    &msg[..msg.len().min(300)]
                );
                if let Some(transcript) = parse_input_transcription(msg) {
                    if !transcript.is_empty() {
                        println!("[GeminiLiveInput] Got final transcript: '{}'", transcript);
                        transcripts_received += 1;
                        accumulated_text.push_str(&transcript);
                    }
                }
            }
            Ok(tungstenite::Message::Binary(data)) => {
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if let Some(transcript) = parse_input_transcription(&text) {
                        if !transcript.is_empty() {
                            println!(
                                "[GeminiLiveInput] Got final transcript (binary): '{}'",
                                transcript
                            );
                            transcripts_received += 1;
                            accumulated_text.push_str(&transcript);
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }

    let _ = socket.close(None);

    println!(
        "[GeminiLiveInput] Done! Transcripts received: {}, Total text length: {}",
        transcripts_received,
        accumulated_text.len()
    );
    println!("[GeminiLiveInput] Final result: '{}'", accumulated_text);

    if accumulated_text.is_empty() {
        // This is actually okay - could be silence or inaudible
        Ok(String::new())
    } else {
        Ok(accumulated_text)
    }
}

/// Extract PCM i16 samples from WAV data
fn extract_pcm_from_wav(wav_data: &[u8]) -> anyhow::Result<Vec<i16>> {
    use std::io::Cursor;

    let cursor = Cursor::new(wav_data);
    let reader = hound::WavReader::new(cursor)?;
    let spec = reader.spec();

    // Get samples based on format
    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .map(|f| (f * i16::MAX as f32) as i16)
            .collect(),
    };

    // Convert to mono 16kHz if needed
    let mono_samples: Vec<i16> = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|chunk| {
                let sum: i32 = chunk.iter().map(|&s| s as i32).sum();
                (sum / chunk.len() as i32) as i16
            })
            .collect()
    } else {
        samples
    };

    // Resample to 16kHz if needed
    let target_rate = 16000;
    if spec.sample_rate != target_rate {
        let ratio = target_rate as f64 / spec.sample_rate as f64;
        let new_len = (mono_samples.len() as f64 * ratio) as usize;
        let mut resampled = Vec::with_capacity(new_len);

        for i in 0..new_len {
            let src_idx = (i as f64 / ratio) as usize;
            if src_idx < mono_samples.len() {
                resampled.push(mono_samples[src_idx]);
            }
        }
        Ok(resampled)
    } else {
        Ok(mono_samples)
    }
}

/// Simple nearest-neighbor resampling to 16kHz
fn resample_to_16khz(samples: &[i16], source_rate: u32) -> Vec<i16> {
    if source_rate == 16000 {
        return samples.to_vec();
    }
    let ratio = 16000.0 / source_rate as f64;
    let new_len = (samples.len() as f64 * ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let src_idx = (i as f64 / ratio) as usize;
        if src_idx < samples.len() {
            resampled.push(samples[src_idx]);
        }
    }
    resampled
}

/// Real-time record and stream to Gemini Live WebSocket
/// Connects WebSocket FIRST, then streams audio in real-time during recording
pub fn record_and_stream_gemini_live(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    target_window: Option<HWND>,
) {
    use crate::api::realtime_audio::websocket::{
        connect_websocket, parse_input_transcription, send_audio_chunk, send_setup_message,
        set_socket_nonblocking, set_socket_short_timeout,
    };
    use crate::overlay::recording::AUDIO_INITIALIZING;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    println!("[GeminiLiveStream] Starting real-time streaming...");

    let gemini_api_key = {
        let app = APP.lock().unwrap();
        app.config.gemini_api_key.clone()
    };

    if gemini_api_key.trim().is_empty() {
        eprintln!("[GeminiLiveStream] No API key");
        unsafe {
            let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    // Connect WebSocket (Initializing state)
    AUDIO_INITIALIZING.store(true, Ordering::SeqCst);
    println!("[GeminiLiveStream] Connecting WebSocket...");

    let mut socket = match connect_websocket(&gemini_api_key) {
        Ok(s) => {
            println!("[GeminiLiveStream] Connected");
            s
        }
        Err(e) => {
            println!("[GeminiLiveStream] Connection failed: {}", e);
            AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return;
        }
    };

    if let Err(e) = send_setup_message(&mut socket) {
        println!("[GeminiLiveStream] Setup failed: {}", e);
        AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
        unsafe {
            let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    let _ = set_socket_short_timeout(&mut socket);

    // Wait for setupComplete
    let setup_start = Instant::now();
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                if msg.as_str().contains("setupComplete") {
                    break;
                }
            }
            Ok(tungstenite::Message::Binary(data)) => {
                if String::from_utf8(data.to_vec())
                    .map(|t| t.contains("setupComplete"))
                    .unwrap_or(false)
                {
                    break;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if setup_start.elapsed() > Duration::from_secs(30) {
                    AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
                    unsafe {
                        let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
                unsafe {
                    let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return;
            }
        }
    }

    let _ = set_socket_nonblocking(&mut socket);
    AUDIO_INITIALIZING.store(false, Ordering::SeqCst);
    crate::overlay::recording::AUDIO_WARMUP_COMPLETE.store(true, Ordering::SeqCst);
    println!("[GeminiLiveStream] Setup complete, starting audio...");

    // Start audio capture
    #[cfg(target_os = "windows")]
    let host = if preset.audio_source == "device" {
        cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host())
    } else {
        cpal::default_host()
    };
    #[cfg(not(target_os = "windows"))]
    let host = cpal::default_host();

    let device = if preset.audio_source == "device" {
        match host.default_output_device() {
            Some(d) => d,
            None => {
                let _ = socket.close(None);
                unsafe {
                    let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return;
            }
        }
    } else {
        match host.default_input_device() {
            Some(d) => d,
            None => {
                let _ = socket.close(None);
                unsafe {
                    let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return;
            }
        }
    };

    let config = if preset.audio_source == "device" {
        device
            .default_output_config()
            .or_else(|_| device.default_input_config())
    } else {
        device.default_input_config()
    };
    let config = match config {
        Ok(c) => c,
        Err(_) => {
            let _ = socket.close(None);
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return;
        }
    };

    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let accumulated_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let audio_buffer_clone = audio_buffer.clone();
    let pause_clone = pause_signal.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if pause_clone.load(Ordering::Relaxed) {
                    return;
                }
                let mut rms = 0.0;
                for &x in data {
                    rms += x * x;
                }
                rms = (rms / data.len() as f32).sqrt();
                crate::overlay::recording::update_audio_viz(rms);
                let mono: Vec<i16> = if channels > 1 {
                    data.chunks(channels)
                        .map(|c| {
                            ((c.iter().sum::<f32>() / channels as f32) * i16::MAX as f32) as i16
                        })
                        .collect()
                } else {
                    data.iter().map(|&f| (f * i16::MAX as f32) as i16).collect()
                };
                let resampled = resample_to_16khz(&mono, sample_rate);
                if let Ok(mut buf) = audio_buffer_clone.lock() {
                    buf.extend(resampled);
                }
            },
            |e| eprintln!("Stream error: {}", e),
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if pause_clone.load(Ordering::Relaxed) {
                    return;
                }
                let mut rms = 0.0;
                for &x in data {
                    let f = x as f32 / i16::MAX as f32;
                    rms += f * f;
                }
                rms = (rms / data.len() as f32).sqrt();
                crate::overlay::recording::update_audio_viz(rms);
                let mono: Vec<i16> = if channels > 1 {
                    data.chunks(channels)
                        .map(|c| (c.iter().map(|&s| s as i32).sum::<i32>() / c.len() as i32) as i16)
                        .collect()
                } else {
                    data.to_vec()
                };
                let resampled = resample_to_16khz(&mono, sample_rate);
                if let Ok(mut buf) = audio_buffer_clone.lock() {
                    buf.extend(resampled);
                }
            },
            |e| eprintln!("Stream error: {}", e),
            None,
        ),
        _ => {
            let _ = socket.close(None);
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return;
        }
    };

    let stream = match stream {
        Ok(s) => s,
        Err(_) => {
            let _ = socket.close(None);
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return;
        }
    };
    if stream.play().is_err() {
        let _ = socket.close(None);
        unsafe {
            let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    println!("[GeminiLiveStream] Streaming audio...");
    let chunk_size = 1600;
    let mut last_send = Instant::now();
    let send_interval = Duration::from_millis(100);
    let auto_stop = preset.auto_stop_recording;
    let mut has_spoken = false;
    let mut first_speech: Option<Instant> = None;
    let mut last_active = Instant::now();

    while !stop_signal.load(Ordering::SeqCst) && !abort_signal.load(Ordering::SeqCst) {
        if !preset.hide_recording_ui && !unsafe { IsWindow(Some(overlay_hwnd)).as_bool() } {
            break;
        }

        if last_send.elapsed() >= send_interval {
            let chunk: Vec<i16> = {
                let mut buf = audio_buffer.lock().unwrap();
                if buf.len() >= chunk_size {
                    buf.drain(..chunk_size).collect()
                } else if !buf.is_empty() {
                    std::mem::take(&mut *buf)
                } else {
                    Vec::new()
                }
            };
            if !chunk.is_empty() {
                let _ = send_audio_chunk(&mut socket, &chunk);
            }
            last_send = Instant::now();
        }

        // Read transcriptions
        loop {
            match socket.read() {
                Ok(tungstenite::Message::Text(msg)) => {
                    if let Some(t) = parse_input_transcription(msg.as_str()) {
                        if !t.is_empty() {
                            if let Ok(mut txt) = accumulated_text.lock() {
                                txt.push_str(&t);
                            }
                            if preset.auto_paste {
                                if let Some(target) = target_window {
                                    crate::overlay::utils::type_text_to_window(target, &t);
                                }
                            }
                        }
                    }
                }
                Ok(tungstenite::Message::Binary(data)) => {
                    if let Ok(s) = String::from_utf8(data.to_vec()) {
                        if let Some(t) = parse_input_transcription(&s) {
                            if !t.is_empty() {
                                if let Ok(mut txt) = accumulated_text.lock() {
                                    txt.push_str(&t);
                                }
                                if preset.auto_paste {
                                    if let Some(target) = target_window {
                                        crate::overlay::utils::type_text_to_window(target, &t);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break
                }
                Err(_) => break,
            }
        }

        // Auto-stop
        if auto_stop {
            let rms =
                f32::from_bits(crate::overlay::recording::CURRENT_RMS.load(Ordering::Relaxed));
            if rms > 0.015 {
                if !has_spoken {
                    first_speech = Some(Instant::now());
                }
                has_spoken = true;
                last_active = Instant::now();
            } else if has_spoken
                && first_speech.map(|t| t.elapsed().as_millis()).unwrap_or(0) >= 2000
                && last_active.elapsed().as_millis() > 800
            {
                stop_signal.store(true, Ordering::SeqCst);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    drop(stream);
    println!("[GeminiLiveStream] Stopped, waiting 2s...");

    if !abort_signal.load(Ordering::SeqCst) {
        let remaining: Vec<i16> = std::mem::take(&mut *audio_buffer.lock().unwrap());
        if !remaining.is_empty() {
            let _ = send_audio_chunk(&mut socket, &remaining);
        }

        // Adaptive wait: Start with 500ms
        // If we get data, extend by 600ms (was 300ms), up to max 4.0s (was 2.5s)
        let mut conclude_end = Instant::now() + Duration::from_millis(500);
        let max_stop_time = Instant::now() + Duration::from_millis(4000);
        let extension = Duration::from_millis(600);

        println!("[GeminiLiveStream] Waiting for tail...");

        while Instant::now() < conclude_end && Instant::now() < max_stop_time {
            // We need to set the socket timeout dynamically or just rely on non-blocking + sleep
            // Since we set non-blocking earlier, read() retrieves immediately.

            match socket.read() {
                Ok(tungstenite::Message::Text(msg)) => {
                    if let Some(t) = parse_input_transcription(msg.as_str()) {
                        if !t.is_empty() {
                            if let Ok(mut txt) = accumulated_text.lock() {
                                txt.push_str(&t);
                            }
                            // Found data, extend wait
                            conclude_end = Instant::now() + extension;
                        }
                    }
                }
                Ok(tungstenite::Message::Binary(data)) => {
                    if let Ok(s) = String::from_utf8(data.to_vec()) {
                        if let Some(t) = parse_input_transcription(&s) {
                            if !t.is_empty() {
                                if let Ok(mut txt) = accumulated_text.lock() {
                                    txt.push_str(&t);
                                }
                                if preset.auto_paste {
                                    if let Some(target) = target_window {
                                        crate::overlay::utils::type_text_to_window(target, &t);
                                    }
                                }
                                // Found data, extend wait
                                conclude_end = Instant::now() + extension;
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    }

    let _ = socket.close(None);
    let final_text = accumulated_text.lock().unwrap().clone();
    println!("[GeminiLiveStream] Result: '{}'", final_text);

    if abort_signal.load(Ordering::SeqCst) || final_text.is_empty() {
        unsafe {
            if IsWindow(Some(overlay_hwnd)).as_bool() {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    {
        let app = APP.lock().unwrap();
        app.history.save_audio(Vec::new(), final_text.clone());
    }

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let (rect, retrans) = if preset.blocks.len() > 1 {
        let w = 600;
        let h = 300;
        let gap = 20;
        let total = w * 2 + gap;
        let x = (screen_w - total) / 2;
        let y = (screen_h - h) / 2;
        (
            RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            Some(RECT {
                left: x + w + gap,
                top: y,
                right: x + w + gap + w,
                bottom: y + h,
            }),
        )
    } else {
        let w = 700;
        let h = 300;
        let x = (screen_w - w) / 2;
        let y = (screen_h - h) / 2;
        (
            RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            None,
        )
    };

    crate::overlay::process::show_audio_result(
        preset,
        final_text,
        Vec::new(),
        rect,
        retrans,
        overlay_hwnd,
        true, // is_streaming_result: disable auto-paste for Gemini Live
    );
}

pub fn record_and_stream_parakeet(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    _pause_signal: Arc<AtomicBool>,
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    target_window: Option<HWND>,
) {
    use std::sync::Mutex;
    let accumulated_text = Arc::new(Mutex::new(String::new()));
    let acc_clone = accumulated_text.clone();
    let preset_clone = preset.clone();
    let target_window_clone = target_window;

    let callback = move |text: String| {
        if !text.is_empty() {
            if let Ok(mut txt) = acc_clone.lock() {
                txt.push_str(&text);
            }
            // Real-time typing
            if preset_clone.auto_paste {
                if let Some(target) = target_window_clone {
                    crate::overlay::utils::type_text_to_window(target, &text);
                }
            }
        }
    };

    println!("[ParakeetStream] Starting Parakeet session...");

    // Run Parakeet session (blocks until stopped)
    let res = crate::api::realtime_audio::parakeet::run_parakeet_session(
        stop_signal.clone(),
        Some(overlay_hwnd), // Send volume updates to overlay
        true,               // Enable download badge
        Some(preset_clone.audio_source.clone()),
        preset_clone.auto_stop_recording,
        callback,
    );

    if let Err(e) = res {
        eprintln!("[ParakeetStream] Error: {:?}", e);
    }

    // Check for abort
    if abort_signal.load(Ordering::SeqCst) {
        unsafe {
            if IsWindow(Some(overlay_hwnd)).as_bool() {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    let final_text = accumulated_text.lock().unwrap().clone();
    println!("[ParakeetStream] Final Result: '{}'", final_text);

    if final_text.is_empty() {
        unsafe {
            if IsWindow(Some(overlay_hwnd)).as_bool() {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    // Save history
    {
        let app = crate::APP.lock().unwrap();
        app.history.save_audio(Vec::new(), final_text.clone());
    }

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let (rect, retrans) = if preset.blocks.len() > 1 {
        let w = 600;
        let h = 300;
        let gap = 20;
        let total = w * 2 + gap;
        let x = (screen_w - total) / 2;
        let y = (screen_h - h) / 2;
        (
            RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            Some(RECT {
                left: x + w + gap,
                top: y,
                right: x + w + gap + w,
                bottom: y + h,
            }),
        )
    } else {
        let w = 700;
        let h = 300;
        let x = (screen_w - w) / 2;
        let y = (screen_h - h) / 2;
        (
            RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            None,
        )
    };

    crate::overlay::process::show_audio_result(
        preset,
        final_text,
        Vec::new(),
        rect,
        retrans,
        overlay_hwnd,
        true, // is_streaming_result: disable auto-paste
    );
}

fn upload_audio_to_whisper(
    api_key: &str,
    model: &str,
    audio_data: Vec<u8>,
) -> anyhow::Result<String> {
    // Create multipart form data
    let boundary = format!(
        "----SGTBoundary{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let mut body = Vec::new();

    // Add model field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"model\"\r\n\r\n");
    body.extend_from_slice(model.as_bytes());
    body.extend_from_slice(b"\r\n");

    // Add file field
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(&audio_data);
    body.extend_from_slice(b"\r\n");

    // End boundary
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    // Make API request
    let response = UREQ_AGENT
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .header("Authorization", &format!("Bearer {}", api_key))
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send(&body);

    let response = match response {
        Ok(resp) => resp,
        Err(e) => {
            let err_str = e.to_string();
            return Err(anyhow::anyhow!("API request failed: {}", err_str));
        }
    };

    // --- CAPTURE RATE LIMITS ---
    if let Some(remaining) = response
        .headers()
        .get("x-ratelimit-remaining-requests")
        .and_then(|v| v.to_str().ok())
    {
        let limit = response
            .headers()
            .get("x-ratelimit-limit-requests")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?");
        let usage_str = format!("{} / {}", remaining, limit);
        if let Ok(mut app) = APP.lock() {
            app.model_usage_stats.insert(model.to_string(), usage_str);
        }
    }
    // ---------------------------

    // Parse response
    let json: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

    let text = json
        .get("text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("No text in response"))?;

    Ok(text.to_string())
}

/// Shared logic to process audio data based on a preset's configuration
/// Returns the transcription/processing result text
fn execute_audio_processing_logic(preset: &Preset, wav_data: Vec<u8>) -> anyhow::Result<String> {
    // Find the first block that is specifically an "audio" processing block
    // OR allow input_adapter if no audio block exists (for raw audio overlay)
    let (audio_block, is_raw_input_adapter) =
        match preset.blocks.iter().find(|b| b.block_type == "audio") {
            Some(b) => (b.clone(), false),
            None => match preset
                .blocks
                .iter()
                .find(|b| b.block_type == "input_adapter")
            {
                Some(b) => (b.clone(), true),
                None => {
                    let debug_types: Vec<_> = preset.blocks.iter().map(|b| &b.block_type).collect();
                    eprintln!(
                    "DEBUG [Audio]: No 'audio' blocks found in preset. Block types present: {:?}",
                    debug_types
                );
                    return Err(anyhow::anyhow!(
                        "Audio preset has no 'audio' processing blocks configured"
                    ));
                }
            },
        };

    if is_raw_input_adapter {
        return Ok(String::new());
    }

    let model_config = get_model_by_id(&audio_block.model);
    let model_config = match model_config {
        Some(c) => c,
        None => {
            return Err(anyhow::anyhow!(
                "Model config not found for audio model: {}",
                audio_block.model
            ));
        }
    };
    let model_name = model_config.full_name.clone();
    let provider = model_config.provider.clone();

    let (groq_api_key, gemini_api_key) = {
        let app = crate::APP.lock().unwrap();
        (
            app.config.api_key.clone(),
            app.config.gemini_api_key.clone(),
        )
    };

    // Use block's prompt and language settings
    let mut final_prompt = if model_is_non_llm(&audio_block.model) {
        String::new()
    } else {
        audio_block.prompt.clone()
    };

    for (key, value) in &audio_block.language_vars {
        let pattern = format!("{{{}}}", key);
        final_prompt = final_prompt.replace(&pattern, value);
    }

    if final_prompt.contains("{language1}") && !audio_block.language_vars.contains_key("language1")
    {
        final_prompt = final_prompt.replace("{language1}", &audio_block.selected_language);
    }

    final_prompt = final_prompt.replace("{language}", &audio_block.selected_language);

    if provider == "groq" {
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
    } else if provider == "gemini-live" {
        // Gemini Live API (WebSocket-based) - uses INPUT transcription (what user said)
        // instead of LLM output transcription
        if gemini_api_key.trim().is_empty() {
            Err(anyhow::anyhow!("NO_API_KEY:gemini"))
        } else {
            transcribe_with_gemini_live_input(&gemini_api_key, wav_data)
        }
    } else {
        Err(anyhow::anyhow!("Unsupported audio provider: {}", provider))
    }
}

pub fn record_audio_and_transcribe(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
    abort_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
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
                    unsafe {
                        let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return;
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Strict failure if not Windows (device loopback primarily supported on Windows via WASAPI)
            eprintln!("Error: Device capture not supported on this OS or no device found.");
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return;
        }
    } else {
        match host.default_input_device() {
            Some(d) => d,
            None => {
                eprintln!("Error: No input device available.");
                unsafe {
                    let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return;
            }
        }
    };

    let config = if preset.audio_source == "device" {
        match device.default_output_config() {
            Ok(c) => c,
            Err(_) => match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to get audio config: {}", e);
                    unsafe {
                        let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return;
                }
            },
        }
    } else {
        match device.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to get audio config: {}", e);
                unsafe {
                    let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return;
            }
        }
    };

    let sample_rate = config.sample_rate();
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
                    for &x in data {
                        rms += x * x;
                    }
                    rms = (rms / data.len() as f32).sqrt();
                    crate::overlay::recording::update_audio_viz(rms);

                    // Signal warmup complete when we get meaningful audio
                    if rms > WARMUP_RMS_THRESHOLD {
                        crate::overlay::recording::AUDIO_WARMUP_COMPLETE
                            .store(true, Ordering::SeqCst);
                    }
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if !pause_signal.load(Ordering::Relaxed) {
                    let f32_data: Vec<f32> =
                        data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let _ = tx.send(f32_data);
                    let mut rms = 0.0;
                    for &x in data {
                        let f = x as f32 / i16::MAX as f32;
                        rms += f * f;
                    }
                    rms = (rms / data.len() as f32).sqrt();
                    crate::overlay::recording::update_audio_viz(rms);

                    // Signal warmup complete when we get meaningful audio
                    if rms > WARMUP_RMS_THRESHOLD {
                        crate::overlay::recording::AUDIO_WARMUP_COMPLETE
                            .store(true, Ordering::SeqCst);
                    }
                }
            },
            err_fn,
            None,
        ),
        _ => {
            eprintln!(
                "Unsupported audio sample format: {:?}",
                config.sample_format()
            );
            Err(cpal::BuildStreamError::StreamConfigNotSupported)
        }
    };

    if let Err(e) = stream_res {
        eprintln!("Failed to build stream: {}", e);
        unsafe {
            let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }
    let stream = stream_res.unwrap();

    if let Err(e) = stream.play() {
        eprintln!("Failed to play stream: {}", e);
        unsafe {
            let _ = PostMessageW(Some(overlay_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return;
    }

    let mut collected_samples: Vec<f32> = Vec::new();

    // --- AUTO-STOP LOGIC STATE ---
    // Only active when preset.auto_stop_recording is true
    let auto_stop_enabled = preset.auto_stop_recording;
    let mut has_spoken = false; // True once user starts speaking
    let mut first_speech_time: Option<std::time::Instant> = None; // When user first spoke
    let mut last_active_time = std::time::Instant::now();

    // Thresholds tuned for typical speech vs silence
    const NOISE_THRESHOLD: f32 = 0.015; // RMS above this = speech
    const SILENCE_LIMIT_MS: u128 = 800; // ms of silence after speech to trigger stop
    const MIN_RECORDING_MS: u128 = 2000; // Minimum 2 seconds after first speech

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
                let recording_duration = first_speech_time
                    .map(|t| t.elapsed().as_millis())
                    .unwrap_or(0);
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

    let samples: Vec<i16> = collected_samples
        .iter()
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
        let mut writer =
            hound::WavWriter::new(&mut wav_cursor, spec).expect("Failed to create memory writer");
        for sample in &samples {
            writer
                .write_sample(*sample)
                .expect("Failed to write sample");
        }
        writer.finalize().expect("Failed to finalize WAV");
    }
    let wav_data = wav_cursor.into_inner();

    // For MASTER presets, show the wheel BEFORE transcription to get the actual preset
    let working_preset = if preset.is_master {
        // Get cursor position for wheel center (use center of screen)
        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let cursor_pos = POINT {
            x: screen_w / 2,
            y: screen_h / 2,
        };

        // Show preset wheel - filter by audio source
        let audio_mode = Some(preset.audio_source.as_str());
        let selected =
            crate::overlay::preset_wheel::show_preset_wheel("audio", audio_mode, cursor_pos);

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

    // Clone wav_data for history saving
    let wav_data_for_history = wav_data.clone();

    // EXECUTE SHARED AUDIO PROCESSING LOGIC
    let transcription_result = execute_audio_processing_logic(&working_preset, wav_data);

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
            // Clone wav_data for the input overlay BEFORE saving to history
            let wav_data_for_overlay = wav_data_for_history.clone();

            // SAVE HISTORY
            {
                let app = crate::APP.lock().unwrap();
                app.history
                    .save_audio(wav_data_for_history, transcription_text.clone());
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
                    RECT {
                        left: start_x,
                        top: y,
                        right: start_x + w,
                        bottom: y + h,
                    },
                    Some(RECT {
                        left: start_x + w + gap,
                        top: y,
                        right: start_x + w + gap + w,
                        bottom: y + h,
                    }),
                )
            } else {
                let w = 700;
                let h = 300;
                let x = (screen_w - w) / 2;
                let y = (screen_h - h) / 2;
                (
                    RECT {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    None,
                )
            };

            // Pass overlay_hwnd to chain processing - it will be kept alive until first visible block
            crate::overlay::process::show_audio_result(
                working_preset,
                transcription_text,
                wav_data_for_overlay,
                rect,
                retranslate_rect,
                overlay_hwnd,
                false, // is_streaming_result: standard transcription (allow paste)
            );
        }
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

/// Process an existing audio file (WAV data) using a specific preset
/// This is used for drag-and-drop audio file processing without recording
pub fn process_audio_file_request(preset: Preset, wav_data: Vec<u8>) {
    // EXECUTE SHARED AUDIO PROCESSING LOGIC
    let processing_result = execute_audio_processing_logic(&preset, wav_data.clone());

    match processing_result {
        Ok(result_text) => {
            // Save history
            {
                let app = crate::APP.lock().unwrap();
                app.history
                    .save_audio(wav_data.clone(), result_text.clone());
            }

            // Calculate centered position for result
            let (screen_w, screen_h) =
                unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };

            // Layout logic same as recording flow
            let has_multiple_blocks = preset.blocks.len() > 1;
            let (rect, retranslate_rect) = if has_multiple_blocks {
                let w = 600;
                let h = 300;
                let gap = 20;
                let total_w = w * 2 + gap;
                let start_x = (screen_w - total_w) / 2;
                let y = (screen_h - h) / 2;

                (
                    RECT {
                        left: start_x,
                        top: y,
                        right: start_x + w,
                        bottom: y + h,
                    },
                    Some(RECT {
                        left: start_x + w + gap,
                        top: y,
                        right: start_x + w + gap + w,
                        bottom: y + h,
                    }),
                )
            } else {
                let w = 700;
                let h = 300;
                let x = (screen_w - w) / 2;
                let y = (screen_h - h) / 2;
                (
                    RECT {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    None,
                )
            };

            // Show result
            crate::overlay::process::show_audio_result(
                preset,
                result_text,
                wav_data,
                rect,
                retranslate_rect,
                HWND(std::ptr::null_mut()), // No recording overlay handle needed
                false,                      // is_streaming_result: file processing (allow paste)
            );
        }
        Err(e) => {
            eprintln!("Audio file processing error: {}", e);
        }
    }
}
