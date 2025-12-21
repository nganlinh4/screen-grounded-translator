//! Real-time audio transcription using Gemini Live API
//! 
//! This module handles streaming audio to Gemini's native audio model
//! and receives real-time transcriptions via WebSocket.
//! 
//! Translation is handled separately via Groq's llama-3.1-8b-instant model
//! every 2 seconds for new sentence chunks.

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Mutex};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use std::io::BufRead;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::config::Preset;
use crate::APP;
use crate::api::client::UREQ_AGENT;

/// Interval for triggering translation (milliseconds)
const TRANSLATION_INTERVAL_MS: u64 = 3000;

/// Model for realtime audio transcription
const REALTIME_MODEL: &str = "gemini-2.5-flash-native-audio-preview-12-2025";

/// Safely truncate a string to max_bytes, respecting UTF-8 character boundaries
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the last valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Shared state for realtime transcription
pub struct RealtimeState {
    /// Full transcript (used for translation and display)
    pub full_transcript: String,
    /// Display transcript (same as full - WebView handles scrolling)
    pub display_transcript: String,
    
    /// Position after the last FULLY FINISHED sentence that was translated
    pub last_committed_pos: usize,
    /// The last text chunk we sent for translation (to detect changes)
    pub last_sent_text: String,
    
    /// Committed translation (finished sentences, never replaced)
    pub committed_translation: String,
    /// Current uncommitted translation (may be replaced when sentence grows)
    pub uncommitted_translation: String,
    /// Display translation (WebView handles scrolling)
    pub display_translation: String,
    
    /// Translation history for conversation context: (source_text, translation)
    /// Keeps last 9 entries to maintain consistent style/atmosphere
    pub translation_history: Vec<(String, String)>,
}

impl RealtimeState {
    pub fn new() -> Self {
        Self {
            full_transcript: String::new(),
            display_transcript: String::new(),
            last_committed_pos: 0,
            last_sent_text: String::new(),
            committed_translation: String::new(),
            uncommitted_translation: String::new(),
            display_translation: String::new(),
            translation_history: Vec::new(),
        }
    }
    
    /// Update display transcript from full transcript
    fn update_display_transcript(&mut self) {
        // No truncation - WebView handles smooth scrolling
        self.display_transcript = self.full_transcript.clone();
    }
    
    /// Update display translation from committed + uncommitted
    fn update_display_translation(&mut self) {
        let full = if self.committed_translation.is_empty() {
            self.uncommitted_translation.clone()
        } else if self.uncommitted_translation.is_empty() {
            self.committed_translation.clone()
        } else {
            format!("{} {}", self.committed_translation, self.uncommitted_translation)
        };
        // No truncation - WebView handles smooth scrolling
        self.display_translation = full;
    }

    
    /// Append new transcript text and update display
    pub fn append_transcript(&mut self, new_text: &str) {
        self.full_transcript.push_str(new_text);
        self.update_display_transcript();
    }
    
    /// Get text to translate: from last_committed_pos to end
    /// Returns (text_to_translate, contains_finished_sentence)
    pub fn get_translation_chunk(&self) -> Option<(String, bool)> {
        if self.last_committed_pos >= self.full_transcript.len() {
            return None;
        }
        if !self.full_transcript.is_char_boundary(self.last_committed_pos) {
            return None;
        }
        let text = &self.full_transcript[self.last_committed_pos..];
        if text.trim().is_empty() {
            return None;
        }
        
        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];
        let has_finished_sentence = text.chars().any(|c| sentence_delimiters.contains(&c));
        
        Some((text.trim().to_string(), has_finished_sentence))
    }
    
    /// Check if the chunk is the same as what we last sent (no change)
    pub fn is_chunk_unchanged(&self, chunk: &str) -> bool {
        chunk == self.last_sent_text
    }
    
    /// Find the end position of the last complete sentence in the transcript
    fn find_last_sentence_end(&self) -> Option<usize> {
        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];
        let mut last_end: Option<usize> = None;
        
        for (idx, ch) in self.full_transcript.char_indices() {
            if sentence_delimiters.contains(&ch) {
                last_end = Some(idx + ch.len_utf8());
            }
        }
        last_end
    }
    
    /// Commit finished sentences after successful translation
    pub fn commit_finished_sentences(&mut self) {
        if let Some(last_sentence_end) = self.find_last_sentence_end() {
            if last_sentence_end > self.last_committed_pos {
                self.last_committed_pos = last_sentence_end;
                // Move uncommitted translation to committed
                if !self.uncommitted_translation.is_empty() {
                    if self.committed_translation.is_empty() {
                        self.committed_translation = self.uncommitted_translation.clone();
                    } else {
                        self.committed_translation.push(' ');
                        self.committed_translation.push_str(&self.uncommitted_translation);
                    }
                    // No truncation needed - WebView handles scrolling
                    self.uncommitted_translation.clear();
                    self.update_display_translation();
                }
            }
        }
    }
    
    /// Check if we should replace the previous translation (sentence grew)
    pub fn should_replace_translation(&self, new_chunk: &str) -> bool {
        !self.last_sent_text.is_empty() && new_chunk != self.last_sent_text
    }
    
    /// Remember what we sent for translation
    pub fn set_last_sent(&mut self, text: &str) {
        self.last_sent_text = text.to_string();
    }
    
    /// Start new translation (clears uncommitted, keeps committed)
    pub fn start_new_translation(&mut self) {
        self.uncommitted_translation.clear();
        self.update_display_translation();
    }
    
    /// Append to uncommitted translation and update display
    pub fn append_translation(&mut self, new_text: &str) {
        self.uncommitted_translation.push_str(new_text);
        self.update_display_translation();
    }
    
    /// Add a completed translation to history for conversation context
    /// Keeps only the last 9 entries
    pub fn add_to_history(&mut self, source: String, translation: String) {
        self.translation_history.push((source, translation));
        // Keep only last 9 entries
        while self.translation_history.len() > 9 {
            self.translation_history.remove(0);
        }
    }
    
    /// Get translation history as messages for API request
    pub fn get_history_messages(&self, target_language: &str) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();
        
        for (source, translation) in &self.translation_history {
            // User message: request to translate
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("Translate to {}:\n{}", target_language, source)
            }));
            // Assistant message: the translation
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": translation
            }));
        }
        
        messages
    }
}

pub type SharedRealtimeState = Arc<Mutex<RealtimeState>>;

/// Create TLS WebSocket connection to Gemini Live API
fn connect_websocket(api_key: &str) -> Result<tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>> {
    let ws_url = format!(
        "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={}",
        api_key
    );
    
    let url = url::Url::parse(&ws_url)?;
    let host = url.host_str().ok_or_else(|| anyhow::anyhow!("No host in URL"))?;
    let port = 443;
    
    // Resolve hostname to IP address first
    use std::net::ToSocketAddrs;
    let addr = format!("{}:{}", host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("Failed to resolve hostname: {}", host))?;
    
    println!("Resolved {} to {}", host, addr);
    
    // Connect TCP with a long timeout for initial handshake
    let tcp_stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    // Use blocking mode with long timeout during setup
    tcp_stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    tcp_stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    tcp_stream.set_nodelay(true)?;
    
    // Wrap with TLS
    let connector = native_tls::TlsConnector::new()?;
    let tls_stream = connector.connect(host, tcp_stream)?;
    
    // WebSocket handshake
    let (socket, _response) = tungstenite::client::client(&ws_url, tls_stream)?;
    
    Ok(socket)
}

/// Set socket to non-blocking mode for the main loop
fn set_socket_nonblocking(socket: &mut tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>) -> Result<()> {
    let stream = socket.get_mut();
    let tcp_stream = stream.get_mut();
    tcp_stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    Ok(())
}

/// Send session setup message to configure transcription mode
fn send_setup_message(socket: &mut tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>) -> Result<()> {
    // Using camelCase as per Gemini Live API documentation
    // We set responseModalities to AUDIO to satisfy the native audio model,
    // but we'll only use the inputAudioTranscription (ignore audio talkback)
    let setup = serde_json::json!({
        "setup": {
            "model": format!("models/{}", REALTIME_MODEL),
            "generationConfig": {
                "responseModalities": ["AUDIO"]  // Required for native audio model
            },
            "inputAudioTranscription": {}  // This is what we actually want - input transcription
        }
    });
    
    let msg_str = setup.to_string();
    println!("Sending setup: {}", safe_truncate(&msg_str, 500));
    
    socket.write(tungstenite::Message::Text(msg_str))?;
    socket.flush()?;
    
    Ok(())
}

/// Send audio chunk to the WebSocket
fn send_audio_chunk(socket: &mut tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>, pcm_data: &[i16]) -> Result<()> {
    // Convert i16 samples to bytes (little-endian)
    let mut bytes = Vec::with_capacity(pcm_data.len() * 2);
    for sample in pcm_data {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    
    let b64_audio = general_purpose::STANDARD.encode(&bytes);
    
    let msg = serde_json::json!({
        "realtime_input": {
            "media_chunks": [{
                "data": b64_audio,
                "mime_type": "audio/pcm;rate=16000"
            }]
        }
    });
    
    socket.write(tungstenite::Message::Text(msg.to_string()))?;
    socket.flush()?;
    
    Ok(())
}

/// Parse inputTranscription from WebSocket message (what the user said)
fn parse_input_transcription(msg: &str) -> Option<String> {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(msg) {
        if let Some(server_content) = json.get("serverContent") {
            if let Some(input_transcription) = server_content.get("inputTranscription") {
                if let Some(text) = input_transcription.get("text").and_then(|t| t.as_str()) {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

/// Custom message for updating overlay text
pub const WM_REALTIME_UPDATE: u32 = WM_APP + 200;
pub const WM_TRANSLATION_UPDATE: u32 = WM_APP + 201;

/// Start realtime audio transcription
/// 
/// This function:
/// 1. Connects to Gemini Live API via WebSocket
/// 2. Streams audio from mic/device
/// 3. Receives transcriptions and updates the overlay
/// 4. Optionally triggers translation every 2 seconds on new sentence chunks
pub fn start_realtime_transcription(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    translation_hwnd: Option<HWND>,
    state: SharedRealtimeState,
) {
    std::thread::spawn(move || {
        if let Err(e) = run_realtime_transcription(preset, stop_signal, overlay_hwnd, translation_hwnd, state) {
            eprintln!("Realtime transcription error: {}", e);
        }
    });
}

fn run_realtime_transcription(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    translation_hwnd: Option<HWND>,
    state: SharedRealtimeState,
) -> Result<()> {
    // Get API key
    let gemini_api_key = {
        let app = APP.lock().unwrap();
        app.config.gemini_api_key.clone()
    };
    
    if gemini_api_key.trim().is_empty() {
        return Err(anyhow::anyhow!("NO_API_KEY:google"));
    }
    
    // Connect WebSocket
    println!("Connecting to Gemini Live API...");
    let mut socket = connect_websocket(&gemini_api_key)?;
    println!("Connected! Sending setup...");
    
    // Send setup for transcription only
    send_setup_message(&mut socket)?;
    println!("Setup sent. Starting audio capture...");
    
    // Wait for setup acknowledgment (blocking mode with 30s timeout)
    let setup_start = Instant::now();
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                println!("Received TEXT: {}", safe_truncate(&msg, 500));
                if msg.contains("setupComplete") {
                    println!("Setup complete!");
                    break;
                }
                // Check for error messages
                if msg.contains("error") || msg.contains("Error") {
                    println!("ERROR in response: {}", msg);
                    return Err(anyhow::anyhow!("Server returned error: {}", msg));
                }
            }
            Ok(tungstenite::Message::Close(frame)) => {
                let close_info = frame.map(|f| format!("code={}, reason={}", f.code, f.reason)).unwrap_or("no frame".to_string());
                println!("Received CLOSE: {}", close_info);
                return Err(anyhow::anyhow!("Connection closed by server: {}", close_info));
            }
            Ok(tungstenite::Message::Binary(data)) => {
                println!("Received BINARY: {} bytes", data.len());
                // Try to decode as UTF-8 text
                if let Ok(text) = String::from_utf8(data.clone()) {
                    println!("Binary as text: {}", safe_truncate(&text, 500));
                    if text.contains("setupComplete") {
                        println!("Setup complete (from binary)!");
                        break;
                    }
                } else {
                    // Could be protobuf or binary JSON - print hex for debugging
                    let hex: String = data.iter().take(50).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                    println!("Binary hex: {}", hex);
                    // The first binary message after setup is likely the setupComplete confirmation
                    // For native audio API, a small binary message is the setup acknowledgment
                    if data.len() < 100 {
                        println!("Assuming small binary is setupComplete");
                        break;
                    }
                }
            }
            Ok(other) => {
                println!("Received OTHER message type: {:?}", other);
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout during read - this is expected with blocking socket, retry
                if setup_start.elapsed() > Duration::from_secs(30) {
                    return Err(anyhow::anyhow!("Setup timeout - no response from server"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                println!("WebSocket error during setup: {:?}", e);
                return Err(e.into());
            }
        }
        
        if stop_signal.load(Ordering::Relaxed) {
            return Ok(());
        }
    }
    
    // Switch to non-blocking mode for the main loop
    set_socket_nonblocking(&mut socket)?;
    println!("Switched to non-blocking mode. Starting audio capture...");
    
    // Setup audio capture (similar to record_audio_and_transcribe)
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
                None => host.default_input_device().expect("No input device available"),
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
            Err(_) => device.default_input_config()?,
        }
    } else {
        device.default_input_config()?
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    
    // Audio buffer for accumulating samples before sending
    let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let audio_buffer_clone = audio_buffer.clone();
    
    // Resample to 16kHz if needed
    let target_rate = 16000u32;
    let resample_ratio = target_rate as f64 / sample_rate as f64;
    
    let stop_signal_audio = stop_signal.clone();
    let err_fn = |err| eprintln!("Audio stream error: {}", err);
    
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| {
                if stop_signal_audio.load(Ordering::Relaxed) { return; }
                
                // Convert to mono and i16
                let mono_samples: Vec<i16> = data.chunks(channels)
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
                    buf.extend(resampled);
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                if stop_signal_audio.load(Ordering::Relaxed) { return; }
                
                // Convert to mono
                let mono_samples: Vec<i16> = data.chunks(channels)
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
                    buf.extend(resampled);
                }
            },
            err_fn,
            None,
        )?,
        _ => return Err(anyhow::anyhow!("Unsupported audio format")),
    };
    
    stream.play()?;
    
    // Create translation thread using Groq's llama-3.1-8b-instant
    let has_translation = translation_hwnd.is_some() && preset.blocks.len() > 1;
    let translation_state = state.clone();
    let translation_stop = stop_signal.clone();
    let translation_preset = preset.clone();
    
    if has_translation {
        let translation_hwnd = translation_hwnd.unwrap();
        std::thread::spawn(move || {
            run_translation_loop(translation_preset, translation_stop, translation_hwnd, translation_state);
        });
    }
    
    // Main loop: send audio chunks and receive transcriptions
    let mut last_send = Instant::now();
    let mut last_debug = Instant::now();
    let send_interval = Duration::from_millis(100); // Send audio every 100ms
    let mut total_samples_sent: usize = 0;
    let mut messages_received: usize = 0;
    
    // Silence injection state machine
    // Every 20 seconds, inject 2 seconds of silence to "wake up" the lazy model
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum AudioMode {
        Normal,    // Normal audio sending
        Silence,   // Sending silence, buffering real audio
        CatchUp,   // Sending buffered audio at 2x speed
    }
    let mut audio_mode = AudioMode::Normal;
    let mut mode_start = Instant::now();
    let mut silence_buffer: Vec<i16> = Vec::new(); // Buffer for audio during silence/catch-up
    
    const NORMAL_DURATION: Duration = Duration::from_secs(20);
    const SILENCE_DURATION: Duration = Duration::from_secs(2);
    const TARGET_SAMPLE_RATE: usize = 16000;
    const SAMPLES_PER_100MS: usize = TARGET_SAMPLE_RATE / 10; // 1600 samples per 100ms
    
    while !stop_signal.load(Ordering::Relaxed) {
        // Check if overlay window still exists
        if !unsafe { IsWindow(overlay_hwnd).as_bool() } {
            stop_signal.store(true, Ordering::SeqCst);
            break;
        }
        
        // State machine transitions
        match audio_mode {
            AudioMode::Normal => {
                if mode_start.elapsed() >= NORMAL_DURATION {
                    println!("[SILENCE INJECTION] Entering silence mode - buffering real audio");
                    audio_mode = AudioMode::Silence;
                    mode_start = Instant::now();
                    silence_buffer.clear();
                }
            }
            AudioMode::Silence => {
                if mode_start.elapsed() >= SILENCE_DURATION {
                    println!("[SILENCE INJECTION] Entering catch-up mode - {} samples buffered", silence_buffer.len());
                    audio_mode = AudioMode::CatchUp;
                    mode_start = Instant::now();
                }
            }
            AudioMode::CatchUp => {
                // Exit catch-up when buffer is depleted
                if silence_buffer.is_empty() {
                    println!("[SILENCE INJECTION] Catch-up complete - returning to normal");
                    audio_mode = AudioMode::Normal;
                    mode_start = Instant::now();
                }
            }
        }
        
        // Send accumulated audio
        if last_send.elapsed() >= send_interval {
            // Get real audio from recording
            let real_audio: Vec<i16> = {
                let mut buf = audio_buffer.lock().unwrap();
                std::mem::take(&mut *buf)
            };
            
            match audio_mode {
                AudioMode::Normal => {
                    // Normal mode: send real audio directly
                    if !real_audio.is_empty() {
                        total_samples_sent += real_audio.len();
                        if let Err(e) = send_audio_chunk(&mut socket, &real_audio) {
                            eprintln!("Error sending audio: {}", e);
                            break;
                        }
                    }
                }
                AudioMode::Silence => {
                    // Silence mode: buffer real audio, send zeros
                    silence_buffer.extend(real_audio);
                    
                    // Send 100ms of silence (zeros)
                    let silence: Vec<i16> = vec![0i16; SAMPLES_PER_100MS];
                    total_samples_sent += silence.len();
                    if let Err(e) = send_audio_chunk(&mut socket, &silence) {
                        eprintln!("Error sending silence: {}", e);
                        break;
                    }
                }
                AudioMode::CatchUp => {
                    // Catch-up mode: buffer new audio, send from buffer at 2x speed
                    silence_buffer.extend(real_audio);
                    
                    // Send 2x normal chunk size (200ms worth = 3200 samples) from buffer
                    let chunk_size = SAMPLES_PER_100MS * 2;
                    let to_send: Vec<i16> = if silence_buffer.len() >= chunk_size {
                        silence_buffer.drain(..chunk_size).collect()
                    } else if !silence_buffer.is_empty() {
                        silence_buffer.drain(..).collect()
                    } else {
                        Vec::new()
                    };
                    
                    if !to_send.is_empty() {
                        total_samples_sent += to_send.len();
                        if let Err(e) = send_audio_chunk(&mut socket, &to_send) {
                            eprintln!("Error sending catch-up audio: {}", e);
                            break;
                        }
                    }
                }
            }
            last_send = Instant::now();
        }
        
        // Debug output every 3 seconds
        if last_debug.elapsed() > Duration::from_secs(3) {
            println!("[DEBUG] Samples sent: {}, Messages received: {}", total_samples_sent, messages_received);
            last_debug = Instant::now();
        }
        
        // Receive transcriptions (non-blocking)
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                messages_received += 1;
                println!("[RECV TEXT] {}", safe_truncate(&msg, 300));
                
                // Parse inputTranscription for Window 1 (what user said)
                if let Some(transcript) = parse_input_transcription(&msg) {
                    if !transcript.is_empty() {
                        println!("[TRANSCRIPT] {}", transcript);
                        if let Ok(mut s) = state.lock() {
                            s.append_transcript(&transcript);
                            let display = s.display_transcript.clone();
                            update_overlay_text(overlay_hwnd, &display);
                        }
                    }
                }
            }
            Ok(tungstenite::Message::Binary(data)) => {
                messages_received += 1;
                // Try to decode as JSON text (the API seems to send JSON in binary frames)
                if let Ok(text) = String::from_utf8(data.clone()) {
                    println!("[RECV BINARY as TEXT] {}", safe_truncate(&text, 300));
                    
                    // Parse inputTranscription for Window 1
                    if let Some(transcript) = parse_input_transcription(&text) {
                        if !transcript.is_empty() {
                            println!("[TRANSCRIPT from binary] {}", transcript);
                            if let Ok(mut s) = state.lock() {
                                s.append_transcript(&transcript);
                                let display = s.display_transcript.clone();
                                update_overlay_text(overlay_hwnd, &display);
                            }
                        }
                    }
                } else {
                    println!("[RECV BINARY] {} bytes (not UTF-8)", data.len());
                }
            }
            Ok(tungstenite::Message::Close(_)) => {
                println!("WebSocket closed by server");
                break;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Non-blocking or timeout, no data available - this is expected
            }
            Err(e) => {
                eprintln!("WebSocket read error: {}", e);
                break;
            }
        }
        
        std::thread::sleep(Duration::from_millis(10));
    }
    
    drop(stream);
    let _ = socket.close(None);
    
    Ok(())
}

/// Translation loop using Groq's llama-3.1-8b-instant model
/// Runs every 2 seconds and translates any new untranslated text
fn run_translation_loop(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    translation_hwnd: HWND,
    state: SharedRealtimeState,
) {
    let interval = Duration::from_millis(TRANSLATION_INTERVAL_MS);
    let mut last_run = Instant::now();
    
    // Get translation block (second block) for target language
    let translation_block = match preset.blocks.get(1) {
        Some(b) => b.clone(),
        None => return,
    };
    
    // Get target language
    let target_language = if !translation_block.selected_language.is_empty() {
        translation_block.selected_language.clone()
    } else {
        translation_block.language_vars.get("language").cloned()
            .or_else(|| translation_block.language_vars.get("language1").cloned())
            .unwrap_or_else(|| "English".to_string())
    };
    
    println!("Translation loop started. Target language: {}", target_language);
    
    while !stop_signal.load(Ordering::Relaxed) {
        if !unsafe { IsWindow(translation_hwnd).as_bool() } {
            break;
        }
        
        if last_run.elapsed() >= interval {
            // Get translation chunk (from last committed sentence to current end)
            let (chunk, should_replace, has_finished, is_unchanged) = {
                let s = state.lock().unwrap();
                match s.get_translation_chunk() {
                    Some((text, has_finished)) => {
                        let is_unchanged = s.is_chunk_unchanged(&text);
                        let should_replace = s.should_replace_translation(&text);
                        (Some(text), should_replace, has_finished, is_unchanged)
                    }
                    None => (None, false, false, true)
                }
            };
            
            // Skip if chunk is unchanged since last translation
            if is_unchanged {
                last_run = Instant::now();
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            
            if let Some(chunk) = chunk {
                println!("[TRANSLATION] {} chunk: {} (has_finished: {})", 
                    if should_replace { "REPLACING" } else { "NEW" },
                    safe_truncate(&chunk, 100),
                    has_finished
                );
                
                // Remember what we're sending
                {
                    let mut s = state.lock().unwrap();
                    s.set_last_sent(&chunk);
                }
                
                // Get API keys, model selection, and history
                let (groq_key, gemini_key, translation_model, history_messages) = {
                    let app = APP.lock().unwrap();
                    let groq = app.config.api_key.clone();
                    let gemini = app.config.gemini_api_key.clone();
                    let model = app.config.realtime_translation_model.clone();
                    drop(app);
                    
                    let history = if let Ok(s) = state.lock() {
                        s.get_history_messages(&target_language)
                    } else {
                        Vec::new()
                    };
                    (groq, gemini, model, history)
                };
                
                // Determine which API to use based on model selection
                let is_google = translation_model == "google-gemma";
                let (url, model_name, api_key) = if is_google {
                    // Google AI API with Gemma
                    (
                        format!("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"),
                        "gemma-3-27b-it".to_string(),
                        gemini_key
                    )
                } else {
                    // Default: Groq API with Llama
                    (
                        "https://api.groq.com/openai/v1/chat/completions".to_string(),
                        "llama-3.1-8b-instant".to_string(),
                        groq_key
                    )
                };
                
                // Build messages array
                let mut messages: Vec<serde_json::Value> = Vec::new();
                
                let system_instruction = format!(
                    "You are a professional translator. Translate text to {} while maintaining consistent style, tone, and atmosphere. Output ONLY the translation, nothing else.",
                    target_language
                );
                
                if is_google {
                    // Google Gemma: No system role, include instruction in first user message
                    // Add history first (without system prompt)
                    messages.extend(history_messages);
                    
                    // Add current translation request with instruction embedded
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": format!("{}\n\nTranslate to {}:\n{}", system_instruction, target_language, chunk)
                    }));
                } else {
                    // Groq Llama: Supports system role
                    messages.push(serde_json::json!({
                        "role": "system",
                        "content": system_instruction
                    }));
                    
                    // Add history (last 9 translation pairs for context)
                    messages.extend(history_messages);
                    
                    // Add current translation request
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": format!("Translate to {}:\n{}", target_language, chunk)
                    }));
                }
                
                if !api_key.is_empty() {
                    let payload = serde_json::json!({
                        "model": model_name,
                        "messages": messages,
                        "stream": true,
                        "max_tokens": 512
                    });
                    
                    // Clone chunk for adding to history later
                    let chunk_for_history = chunk.clone();
                    
                    match UREQ_AGENT.post(&url)
                        .set("Authorization", &format!("Bearer {}", api_key))
                        .set("Content-Type", "application/json")
                        .send_json(payload)
                    {
                        Ok(resp) => {
                            let reader = std::io::BufReader::new(resp.into_reader());
                            let mut full_translation = String::new();
                            let mut is_first_chunk = true;
                            
                            for line in reader.lines().flatten() {
                                if stop_signal.load(Ordering::Relaxed) { break; }
                                
                                if line.starts_with("data: ") {
                                    let json_str = &line["data: ".len()..];
                                    if json_str.trim() == "[DONE]" { break; }
                                    
                                    if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                                        if let Some(choices) = chunk_resp.get("choices").and_then(|c| c.as_array()) {
                                            if let Some(first) = choices.first() {
                                                if let Some(delta) = first.get("delta") {
                                                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                                        full_translation.push_str(content);
                                                        
                                                        // Update display in real-time
                                                        if let Ok(mut s) = state.lock() {
                                                            if is_first_chunk && should_replace {
                                                                // Clear uncommitted and start fresh
                                                                s.start_new_translation();
                                                                s.append_translation(content);
                                                                is_first_chunk = false;
                                                            } else if is_first_chunk {
                                                                // First chunk - just start appending
                                                                s.append_translation(content);
                                                                is_first_chunk = false;
                                                            } else {
                                                                // Subsequent chunks - just append
                                                                s.append_translation(content);
                                                            }
                                                            let display = s.display_translation.clone();
                                                            update_translation_text(translation_hwnd, &display);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            
                            // After successful translation, commit any finished sentences
                            if has_finished && !full_translation.is_empty() {
                                if let Ok(mut s) = state.lock() {
                                    // Add to history for context in future translations
                                    s.add_to_history(chunk_for_history, full_translation.clone());
                                    
                                    s.commit_finished_sentences();
                                    // Clear last_sent since we committed
                                    s.last_sent_text.clear();
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[TRANSLATION] API error ({}): {}", model_name, e);
                        }
                    }
                } else {
                    println!("[TRANSLATION] No API key available for {}", model_name);
                }
            }
            
            last_run = Instant::now();
        }
        
        std::thread::sleep(Duration::from_millis(100));
    }
    
    println!("Translation loop stopped.");
}

// Static buffer for overlay text updates (thread-safe)
lazy_static::lazy_static! {
    pub static ref REALTIME_DISPLAY_TEXT: Mutex<String> = Mutex::new(String::new());
    pub static ref TRANSLATION_DISPLAY_TEXT: Mutex<String> = Mutex::new(String::new());
}

/// Update overlay with new text
fn update_overlay_text(hwnd: HWND, text: &str) {
    if let Ok(mut display) = REALTIME_DISPLAY_TEXT.lock() {
        *display = text.to_string();
    }
    
    // Post message to trigger repaint
    unsafe {
        let _ = PostMessageW(hwnd, WM_REALTIME_UPDATE, WPARAM(0), LPARAM(0));
    }
}

/// Update translation overlay with new text
fn update_translation_text(hwnd: HWND, text: &str) {
    if let Ok(mut display) = TRANSLATION_DISPLAY_TEXT.lock() {
        *display = text.to_string();
    }
    
    // Post message to trigger repaint
    unsafe {
        let _ = PostMessageW(hwnd, WM_TRANSLATION_UPDATE, WPARAM(0), LPARAM(0));
    }
}

/// Get current realtime display text (called by overlay paint)
pub fn get_realtime_display_text() -> String {
    REALTIME_DISPLAY_TEXT.lock().map(|s| s.clone()).unwrap_or_default()
}

/// Get current translation display text (called by overlay paint)
pub fn get_translation_display_text() -> String {
    TRANSLATION_DISPLAY_TEXT.lock().map(|s| s.clone()).unwrap_or_default()
}
