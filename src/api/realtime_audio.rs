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
use urlencoding;
use isolang;
use crate::overlay::realtime_webview::SELECTED_APP_PID;

/// Interval for triggering translation (milliseconds)
const TRANSLATION_INTERVAL_MS: u64 = 1500;

/// Model for realtime audio transcription
const REALTIME_MODEL: &str = "gemini-2.5-flash-native-audio-preview-12-2025";

/// Shared state for realtime transcription
pub struct RealtimeState {
    /// Full transcript (used for translation and display)
    pub full_transcript: String,
    /// Display transcript (same as full - WebView handles scrolling)
    pub display_transcript: String,
    
    /// Position after the last FULLY FINISHED sentence that was translated
    pub last_committed_pos: usize,
    /// The length of full_transcript when we last triggered a translation
    pub last_processed_len: usize,
    
    /// Committed translation (finished sentences, never replaced)
    pub committed_translation: String,
    /// Current uncommitted translation (may be replaced when sentence grows)
    pub uncommitted_translation: String,
    /// Display translation (WebView handles scrolling)
    pub display_translation: String,
    
    /// Translation history for conversation context: (source_text, translation)
    /// Keeps last 3 entries to maintain consistent style/atmosphere
    pub translation_history: Vec<(String, String)>,
}

impl RealtimeState {
    pub fn new() -> Self {
        Self {
            full_transcript: String::new(),
            display_transcript: String::new(),
            last_committed_pos: 0,
            last_processed_len: 0,
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
        
        // Check if chunk contains any sentence delimiter
        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];
        let has_finished_sentence = text.chars().any(|c| sentence_delimiters.contains(&c));
        
        Some((text.trim().to_string(), has_finished_sentence))
    }
    
    /// Check if the transcript has grown since the last translation request
    pub fn is_transcript_unchanged(&self) -> bool {
        self.full_transcript.len() == self.last_processed_len
    }
    
    /// Mark the current transcript length as processed
    pub fn update_last_processed_len(&mut self) {
        self.last_processed_len = self.full_transcript.len();
    }
    
    /// Commit finished sentences after successful translation
    /// Matches sentence delimiters between source and translation, then commits all matched pairs.
    /// For long sentences, PROACTIVELY uses commas as split points before waiting for sentence end.
    /// Uses a low-threshold pressure valve for single sentences to avoid excessive re-translation.
    pub fn commit_finished_sentences(&mut self) {
        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];
        let clause_delimiters = [',', ';', ':', '，', '；', '：'];
        
        // Thresholds for proactive comma splitting
        const LONG_SENTENCE_THRESHOLD: usize = 60;  // Start looking for comma after this many chars
        const MIN_CLAUSE_LENGTH: usize = 20;        // Minimum chars before a comma to commit
        
        let uncommitted_len = self.uncommitted_translation.len();
        
        // Store all valid matches found: (source_absolute_end, translation_relative_end, is_clause)
        let mut matches: Vec<(usize, usize, bool)> = Vec::new();

        // 1. PROACTIVE COMMA SPLIT: If uncommitted is long, check for comma FIRST
        // This ensures we don't wait for sentence endings in long ongoing sentences
        if uncommitted_len > LONG_SENTENCE_THRESHOLD {
            let source_remaining = &self.full_transcript[self.last_committed_pos..];
            let trans_remaining = &self.uncommitted_translation;
            
            // Find the LAST comma that gives us at least MIN_CLAUSE_LENGTH chars
            // (prefer later split for more context while still being proactive)
            let src_clause_end = source_remaining.char_indices()
                .filter(|(_, c)| clause_delimiters.contains(c))
                .filter(|(i, _)| *i >= MIN_CLAUSE_LENGTH)
                .last()
                .map(|(i, c)| i + c.len_utf8());
            
            let trn_clause_end = trans_remaining.char_indices()
                .filter(|(_, c)| clause_delimiters.contains(c))
                .filter(|(i, _)| *i >= MIN_CLAUSE_LENGTH)
                .last()
                .map(|(i, c)| i + c.len_utf8());
            
            if let (Some(s_rel), Some(t_rel)) = (src_clause_end, trn_clause_end) {
                let s_abs = self.last_committed_pos + s_rel;
                matches.push((s_abs, t_rel, true));
                eprintln!("[Commit] PROACTIVE comma split: {} chars at pos {}", t_rel, t_rel);
            }
        }
        
        // 2. If no proactive comma split, look for sentence delimiters
        if matches.is_empty() {
            let mut temp_src_pos = self.last_committed_pos;
            let mut temp_trans_pos = 0;
            
            loop {
                if temp_src_pos >= self.full_transcript.len() { break; }
                if temp_trans_pos >= self.uncommitted_translation.len() { break; }
                
                let source_text = &self.full_transcript[temp_src_pos..];
                let trans_text = &self.uncommitted_translation[temp_trans_pos..];

                let src_sentence_end = source_text.char_indices()
                    .find(|(_, c)| sentence_delimiters.contains(c))
                    .map(|(i, c)| i + c.len_utf8());
                
                let trn_sentence_end = trans_text.char_indices()
                    .find(|(_, c)| sentence_delimiters.contains(c))
                    .map(|(i, c)| i + c.len_utf8());

                if let (Some(s_rel), Some(t_rel)) = (src_sentence_end, trn_sentence_end) {
                    let s_abs = temp_src_pos + s_rel;
                    let t_abs = temp_trans_pos + t_rel;
                    
                    matches.push((s_abs, t_abs, false));
                    temp_src_pos = s_abs;
                    temp_trans_pos = t_abs;
                } else {
                    break;
                }
            }
        }

        // 3. Decide how many to commit
        let num_matches = matches.len();
        let mut num_to_commit = num_matches;

        // Pressure Valve: For single sentence, still require minimum length
        // (but clause splits already checked for 30 char minimum)
        if num_matches == 1 && !matches[0].2 && self.uncommitted_translation.len() < 50 {
            num_to_commit = 0; // Wait for more text or another sentence
            eprintln!("[Commit] Waiting - single short sentence ({} chars)", 
                self.uncommitted_translation.len());
        }

        if num_to_commit > 0 {
            // Get the boundary of the last item we are committing
            let (final_src_pos, final_trans_pos, is_clause) = matches[num_to_commit - 1];
            
            // Extract the text chunk we are committing
            let source_segment = self.full_transcript[self.last_committed_pos..final_src_pos].trim().to_string();
            let trans_segment = self.uncommitted_translation[..final_trans_pos].trim().to_string();
            
            if !source_segment.is_empty() && !trans_segment.is_empty() {
                eprintln!("[Commit] {} {} chars: \"{}...\"", 
                    if is_clause { "CLAUSE" } else { "SENTENCE" },
                    trans_segment.len(),
                    trans_segment.chars().take(50).collect::<String>());
                
                // Add to history (Clean, stabilized context)
                self.add_to_history(source_segment, trans_segment.clone());
                
                // Add to committed string
                if self.committed_translation.is_empty() {
                    self.committed_translation = trans_segment;
                } else {
                    self.committed_translation.push(' ');
                    self.committed_translation.push_str(&trans_segment);
                }
                
                // Update the commit pointer
                self.last_committed_pos = final_src_pos;
                
                // Slice the uncommitted buffer
                self.uncommitted_translation = self.uncommitted_translation[final_trans_pos..].trim().to_string();
                
                if !self.uncommitted_translation.is_empty() {
                    eprintln!("[Commit] Remaining uncommitted: \"{}...\"", 
                        self.uncommitted_translation.chars().take(30).collect::<String>());
                }
            }
        }
        
        self.update_display_translation();
    }
    
    /// Start new translation (clears uncommitted, keeps committed)
    /// NOTE: Caller must update UI immediately after calling this to clear old partial
    pub fn start_new_translation(&mut self) {
        self.uncommitted_translation.clear();
    }
    
    /// Append to uncommitted translation and update display
    pub fn append_translation(&mut self, new_text: &str) {
        self.uncommitted_translation.push_str(new_text);
        self.update_display_translation();
    }
    
    /// Add a completed translation to history for conversation context
    /// Keeps only the last 3 entries
    pub fn add_to_history(&mut self, source: String, translation: String) {
        self.translation_history.push((source, translation));
        // Keep only last 3 entries
        while self.translation_history.len() > 3 {
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

/// Start per-app audio capture using WASAPI process loopback (Windows 10 1903+)
/// 
/// This function spawns a thread that captures audio from a specific process
/// and pushes samples to the provided buffer.
#[cfg(target_os = "windows")]
fn start_per_app_capture(
    process_id: u32,
    audio_buffer: Arc<Mutex<Vec<i16>>>,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    use wasapi::{AudioClient, Direction, StreamMode, SampleType, WaveFormat};
    use std::collections::VecDeque;
    
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
                eprintln!("Per-app capture: Failed to create loopback client for PID {}: {:?}", process_id, e);
                return;
            }
        };
        
        // Configure desired format: 16kHz mono 16-bit (what Gemini expects)
        // With autoconvert=true, Windows will handle resampling from the app's native format
        let desired_format = WaveFormat::new(
            16,  // bits per sample
            16,  // valid bits
            &SampleType::Int,
            16000,  // 16kHz sample rate
            1,      // mono
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
        if let Err(e) = audio_client.initialize_client(
            &desired_format,
            &Direction::Capture,
            &mode,
        ) {
            eprintln!("Per-app capture: Failed to initialize audio client: {:?}", e);
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
                                let sum_sq: f64 = samples.iter()
                                    .map(|&s| (s as f64 / 32768.0).powi(2))
                                    .sum();
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
                "responseModalities": ["AUDIO"],  // Required for native audio model
                "thinkingConfig": {
                    "thinkingBudget": 0  // Disable thinking for lower latency
                }
            },
            "inputAudioTranscription": {}  // This is what we actually want - input transcription
        }
    });
    
    let msg_str = setup.to_string();
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
pub const WM_VOLUME_UPDATE: u32 = WM_APP + 202;
pub const WM_MODEL_SWITCH: u32 = WM_APP + 203;

// Shared RMS value for volume visualization
pub static REALTIME_RMS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Start realtime audio transcription
/// 
/// This function:
/// 1. Connects to Gemini Live API via WebSocket
/// 2. Streams audio from mic/device
/// 3. Receives transcriptions and updates the overlay
/// 4. Optionally triggers translation every 2 seconds on new sentence chunks
/// 5. Restarts automatically when audio source change is requested
pub fn start_realtime_transcription(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    translation_hwnd: Option<HWND>,
    state: SharedRealtimeState,
) {
    let overlay_send = crate::win_types::SendHwnd(overlay_hwnd);
    let translation_send = translation_hwnd.map(crate::win_types::SendHwnd);

    std::thread::spawn(move || {
        transcription_thread_entry(preset, stop_signal, overlay_send, translation_send, state);
    });
}

fn transcription_thread_entry(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_send: crate::win_types::SendHwnd,
    translation_send: Option<crate::win_types::SendHwnd>,
    state: SharedRealtimeState,
) {
    let hwnd_overlay = overlay_send.0;
    let hwnd_translation = translation_send.map(|h| h.0);

    use crate::overlay::realtime_webview::{AUDIO_SOURCE_CHANGE, NEW_AUDIO_SOURCE};
    
    let mut current_preset = preset;
    
    loop {
        // Clear any pending audio source change request
        AUDIO_SOURCE_CHANGE.store(false, Ordering::SeqCst);
        
        // Run transcription - returns when stopped or when restart is needed
        let result = run_realtime_transcription(
            current_preset.clone(),
            stop_signal.clone(),
            hwnd_overlay,
            hwnd_translation,
            state.clone(),
        );
        
        if let Err(e) = result {
            eprintln!("Realtime transcription error: {}", e);
        }
        
        // Check if we should restart with new audio source
        if AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst) {
            if let Ok(new_source) = NEW_AUDIO_SOURCE.lock() {
                if !new_source.is_empty() {
                    current_preset.audio_source = new_source.clone();
                    
                    // IMPORTANT: Signal any running per-app capture thread to stop
                    // by keeping stop_signal true briefly, then waiting
                    stop_signal.store(true, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(200)); // Wait for old capture to notice and exit
                    
                    // Now reset stop signal and restart
                    stop_signal.store(false, Ordering::SeqCst);
                    continue; // Restart the loop
                }
            }
        }
        
        // Normal exit - don't restart
        break;
    }
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
    let mut socket = connect_websocket(&gemini_api_key)?;
    
    // Send setup for transcription only
    send_setup_message(&mut socket)?;
    
    // Wait for setup acknowledgment (blocking mode with 30s timeout)
    let setup_start = Instant::now();
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                if msg.contains("setupComplete") {
                    break;
                }
                if msg.contains("error") || msg.contains("Error") {
                    return Err(anyhow::anyhow!("Server returned error: {}", msg));
                }
            }
            Ok(tungstenite::Message::Close(frame)) => {
                let close_info = frame.map(|f| format!("code={}, reason={}", f.code, f.reason)).unwrap_or("no frame".to_string());
                return Err(anyhow::anyhow!("Connection closed by server: {}", close_info));
            }
            Ok(tungstenite::Message::Binary(data)) => {
                if let Ok(text) = String::from_utf8(data.clone()) {
                    if text.contains("setupComplete") {
                        break;
                    }
                } else if data.len() < 100 {
                    // Small binary message is likely setup acknowledgment
                    break;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                if setup_start.elapsed() > Duration::from_secs(30) {
                    return Err(anyhow::anyhow!("Setup timeout - no response from server"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(e.into());
            }
        }
        
        if stop_signal.load(Ordering::Relaxed) {
            return Ok(());
        }
    }
    
    // Switch to non-blocking mode for the main loop
    set_socket_nonblocking(&mut socket)?;
    
    // Audio buffer for accumulating samples before sending (used by both capture methods)
    let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    
    // Check TTS state and app selection
    use crate::overlay::realtime_webview::REALTIME_TTS_ENABLED;
    let tts_enabled = REALTIME_TTS_ENABLED.load(Ordering::SeqCst);
    let selected_pid = SELECTED_APP_PID.load(Ordering::SeqCst);
    
    // Capture mode decision:
    // - mic mode: always use microphone
    // - device mode + TTS on + app selected: per-app capture (isolate from TTS)
    // - device mode + TTS off: normal device loopback (capture all system audio)
    let using_per_app_capture = preset.audio_source == "device" && tts_enabled && selected_pid > 0;
    let using_device_loopback = preset.audio_source == "device" && !tts_enabled;
    
    // Stream holder (only used for cpal path)
    let _stream: Option<cpal::Stream>;
    

    
    if using_per_app_capture {
        // Use per-app audio capture via wasapi (TTS is on, so isolate)
        // Using per-app capture for TTS isolation
        #[cfg(target_os = "windows")]
        {
            start_per_app_capture(selected_pid, audio_buffer.clone(), stop_signal.clone())?;
        }
        _stream = None;
    } else if using_device_loopback {
        // TTS is off, safe to capture all device audio
        // Using device loopback
        #[cfg(target_os = "windows")]
        let host = cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host());
        #[cfg(not(target_os = "windows"))]
        let host = cpal::default_host();
        
        // Use default output device for loopback
        let device = host.default_output_device().expect("No output device available");
        let config = device.default_output_config()?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        
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
                        buf.extend(resampled.iter().cloned());
                    }
                    
                    // Calculate RMS for volume visualization
                    if !resampled.is_empty() {
                        let sum_sq: f64 = resampled.iter()
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
                        buf.extend(resampled.iter().cloned());
                    }
                    
                    // Calculate RMS for volume visualization
                    if !resampled.is_empty() {
                        let sum_sq: f64 = resampled.iter()
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
        _stream = Some(stream);
    } else if preset.audio_source == "device" && tts_enabled && selected_pid == 0 {
        // Device mode + TTS on + no app selected - need to select app first
        // Waiting for app selection
        _stream = None;
    } else {
        // Microphone mode
        // Using microphone input
        let host = cpal::default_host();
        let device = host.default_input_device().expect("No microphone available");
        let config = device.default_input_config()?;
        
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let audio_buffer_clone = audio_buffer.clone();
        let target_rate = 16000u32;
        let resample_ratio = target_rate as f64 / sample_rate as f64;
        let stop_signal_audio = stop_signal.clone();
        let err_fn = |err| eprintln!("Audio stream error: {}", err);
        
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &_| {
                    if stop_signal_audio.load(Ordering::Relaxed) { return; }
                    
                    let mono_samples: Vec<i16> = data.chunks(channels)
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
                        let sum_sq: f64 = resampled.iter()
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
        _stream = Some(stream);
    }
    
    // Create translation thread using Groq's llama-3.1-8b-instant
    let has_translation = translation_hwnd.is_some() && preset.blocks.len() > 1;
    let translation_state = state.clone();
    let translation_stop = stop_signal.clone();
    let translation_preset = preset.clone();
    
    if has_translation {
        let translation_hwnd = translation_hwnd.unwrap();
        let translation_send = crate::win_types::SendHwnd(translation_hwnd);
        std::thread::spawn(move || {
            translation_thread_entry(translation_preset, translation_stop, translation_send, translation_state);
        });
    }


fn translation_thread_entry(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    translation_send: crate::win_types::SendHwnd,
    state: SharedRealtimeState,
) {
    let translation_hwnd = translation_send.0;
    run_translation_loop(preset, stop_signal, translation_hwnd, state);
}
    
    // Main loop: send audio chunks and receive transcriptions
    let mut last_send = Instant::now();
    let send_interval = Duration::from_millis(100); // Send audio every 100ms
    
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
    
    // Proactive reconnection state: detect when model becomes "stuck"
    // If we're sending audio but not receiving any transcription for N seconds, reconnect
    let mut last_transcription_time = Instant::now();
    let mut consecutive_empty_reads: u32 = 0;
    const NO_RESULT_THRESHOLD_SECS: u64 = 8; // Reconnect if no results for 8 seconds
    const EMPTY_READ_CHECK_COUNT: u32 = 50; // Only check after ~5 seconds (50 * 100ms)
    
    while !stop_signal.load(Ordering::Relaxed) {
        // Check if overlay window still exists
        if !unsafe { IsWindow(Some(overlay_hwnd)).as_bool() } {
            stop_signal.store(true, Ordering::SeqCst);
            break;
        }
        
        // Check if audio source change was requested - exit to restart
        {
            use crate::overlay::realtime_webview::AUDIO_SOURCE_CHANGE;
            if AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst) {
                break; // Exit loop to restart with new source
            }
        }
        
        // State machine transitions
        match audio_mode {
            AudioMode::Normal => {
                if mode_start.elapsed() >= NORMAL_DURATION {
                    audio_mode = AudioMode::Silence;
                    mode_start = Instant::now();
                    silence_buffer.clear();
                }
            }
            AudioMode::Silence => {
                if mode_start.elapsed() >= SILENCE_DURATION {
                    audio_mode = AudioMode::CatchUp;
                    mode_start = Instant::now();
                }
            }
            AudioMode::CatchUp => {
                // Exit catch-up when buffer is depleted
                if silence_buffer.is_empty() {
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
                        if let Err(_) = send_audio_chunk(&mut socket, &real_audio) {
                            break;
                        }
                    }
                }
                AudioMode::Silence => {
                    // Silence mode: buffer real audio, send zeros
                    silence_buffer.extend(real_audio);
                    
                    // Send 100ms of silence (zeros)
                    let silence: Vec<i16> = vec![0i16; SAMPLES_PER_100MS];
                    if let Err(_) = send_audio_chunk(&mut socket, &silence) {
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
                        if let Err(_) = send_audio_chunk(&mut socket, &to_send) {
                            break;
                        }
                    }
                }
            }
            last_send = Instant::now();
            
            // Post volume update to overlay window for visualizer
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_VOLUME_UPDATE, WPARAM(0), LPARAM(0));
            }
        }
        
        // Receive transcriptions (non-blocking)
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                
                // Parse inputTranscription for Window 1 (what user said)
                if let Some(transcript) = parse_input_transcription(&msg) {
                    if !transcript.is_empty() {
                        // Reset no-result trackers - we got a valid transcription
                        last_transcription_time = Instant::now();
                        consecutive_empty_reads = 0;
                        
                        let display_text = if let Ok(mut s) = state.lock() {
                            s.append_transcript(&transcript);
                            s.display_transcript.clone()
                        } else {
                            String::new()
                        };
                        
                        if !display_text.is_empty() {
                            update_overlay_text(overlay_hwnd, &display_text);
                        }
                    }
                }
            }
            Ok(tungstenite::Message::Binary(data)) => {
                // Try to decode as JSON text (the API sends JSON in binary frames)
                if let Ok(text) = String::from_utf8(data.clone()) {
                    // Parse inputTranscription for Window 1
                    if let Some(transcript) = parse_input_transcription(&text) {
                        if !transcript.is_empty() {
                            // Reset no-result trackers - we got a valid transcription
                            last_transcription_time = Instant::now();
                            consecutive_empty_reads = 0;
                            
                            let display_text = if let Ok(mut s) = state.lock() {
                                s.append_transcript(&transcript);
                                s.display_transcript.clone()
                            } else {
                                String::new()
                            };
                            
                            if !display_text.is_empty() {
                                update_overlay_text(overlay_hwnd, &display_text);
                            }
                        }
                    }
                } else {
                    // Binary data that's not UTF-8 - ignore
                }
            }
            Ok(tungstenite::Message::Close(_)) => {
                // Enter reconnection mode - buffer audio while reconnecting
                let mut reconnect_buffer: Vec<i16> = Vec::new();
                let _ = socket.close(None);
                
                // Try to reconnect (with retry)
                let mut reconnected = false;
                for _attempt in 1..=3 {
                    {
                        let mut buf = audio_buffer.lock().unwrap();
                        reconnect_buffer.extend(std::mem::take(&mut *buf));
                    }
                    
                    match connect_websocket(&gemini_api_key) {
                        Ok(mut new_socket) => {
                            if send_setup_message(&mut new_socket).is_err() { continue; }
                            if set_socket_nonblocking(&mut new_socket).is_err() { continue; }
                            
                            {
                                let mut buf = audio_buffer.lock().unwrap();
                                reconnect_buffer.extend(std::mem::take(&mut *buf));
                            }
                            
                            silence_buffer.clear();
                            silence_buffer.extend(reconnect_buffer);
                            audio_mode = AudioMode::CatchUp;
                            mode_start = Instant::now();
                            socket = new_socket;
                            
                            // Reset no-result trackers after successful reconnection
                            last_transcription_time = Instant::now();
                            consecutive_empty_reads = 0;
                            
                            reconnected = true;
                            break;
                        }
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    }
                }
                
                if !reconnected { break; }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // Non-blocking or timeout, no data available - this is expected
                // Track consecutive empty reads for proactive reconnection
                consecutive_empty_reads += 1;
                
                // Check if we should proactively reconnect due to no results
                if consecutive_empty_reads >= EMPTY_READ_CHECK_COUNT 
                    && last_transcription_time.elapsed() > Duration::from_secs(NO_RESULT_THRESHOLD_SECS) 
                {
                    // Model appears stuck - proactively reconnect
                    // Start buffering audio NOW before we even close the socket
                    let mut reconnect_buffer: Vec<i16> = Vec::new();
                    {
                        let mut buf = audio_buffer.lock().unwrap();
                        reconnect_buffer.extend(std::mem::take(&mut *buf));
                    }
                    
                    let _ = socket.close(None);
                    
                    let mut reconnected = false;
                    for _attempt in 1..=3 {
                        // Keep collecting audio during reconnection attempts
                        {
                            let mut buf = audio_buffer.lock().unwrap();
                            reconnect_buffer.extend(std::mem::take(&mut *buf));
                        }
                        
                        match connect_websocket(&gemini_api_key) {
                            Ok(mut new_socket) => {
                                if send_setup_message(&mut new_socket).is_err() { continue; }
                                if set_socket_nonblocking(&mut new_socket).is_err() { continue; }
                                
                                // Collect any audio that came in during setup
                                {
                                    let mut buf = audio_buffer.lock().unwrap();
                                    reconnect_buffer.extend(std::mem::take(&mut *buf));
                                }
                                
                                // Transfer buffered audio to catch-up mode
                                silence_buffer.clear();
                                silence_buffer.extend(reconnect_buffer);
                                audio_mode = AudioMode::CatchUp;
                                mode_start = Instant::now();
                                socket = new_socket;
                                
                                // Reset no-result trackers after successful reconnection
                                last_transcription_time = Instant::now();
                                consecutive_empty_reads = 0;
                                
                                reconnected = true;
                                break;
                            }
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(500));
                            }
                        }
                    }
                    
                    if !reconnected { break; }
                }
            }
            Err(e) => {
                // Check if it's a connection reset error - treat similar to close
                let error_str = e.to_string();
                if error_str.contains("reset") || error_str.contains("closed") || error_str.contains("broken") {
                    let mut reconnect_buffer: Vec<i16> = Vec::new();
                    let _ = socket.close(None);
                    
                    let mut reconnected = false;
                    for _attempt in 1..=3 {
                        {
                            let mut buf = audio_buffer.lock().unwrap();
                            reconnect_buffer.extend(std::mem::take(&mut *buf));
                        }
                        
                        match connect_websocket(&gemini_api_key) {
                            Ok(mut new_socket) => {
                                if send_setup_message(&mut new_socket).is_err() { continue; }
                                if set_socket_nonblocking(&mut new_socket).is_err() { continue; }
                                
                                {
                                    let mut buf = audio_buffer.lock().unwrap();
                                    reconnect_buffer.extend(std::mem::take(&mut *buf));
                                }
                                
                                silence_buffer.clear();
                                silence_buffer.extend(reconnect_buffer);
                                audio_mode = AudioMode::CatchUp;
                                mode_start = Instant::now();
                                socket = new_socket;
                                
                                // Reset no-result trackers after successful reconnection
                                last_transcription_time = Instant::now();
                                consecutive_empty_reads = 0;
                                
                                reconnected = true;
                                break;
                            }
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(500));
                            }
                        }
                    }
                    
                    if !reconnected { break; }
                } else {
                    break;
                }
            }
        }
        
        std::thread::sleep(Duration::from_millis(10));
    }
    
    drop(_stream);
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
    
    // Get target language - FIRST check if UI already set NEW_TARGET_LANGUAGE (race condition fix)
    // Then fall back to translation block settings
    let mut target_language = {
        // Check if realtime_webview already set the language at startup
        let from_ui = crate::overlay::realtime_webview::NEW_TARGET_LANGUAGE.lock()
            .ok()
            .and_then(|lang| if lang.is_empty() { None } else { Some(lang.clone()) });
        
        from_ui.unwrap_or_else(|| {
            // Fall back to preset's translation block
            if !translation_block.selected_language.is_empty() {
                translation_block.selected_language.clone()
            } else {
                translation_block.language_vars.get("language").cloned()
                    .or_else(|| translation_block.language_vars.get("language1").cloned())
                    .unwrap_or_else(|| "English".to_string())
            }
        })
    };
    
    while !stop_signal.load(Ordering::Relaxed) {
        if !unsafe { IsWindow(Some(translation_hwnd)).as_bool() } {
            break;
        }
        
        // Check for language change
        if crate::overlay::realtime_webview::LANGUAGE_CHANGE.load(Ordering::SeqCst) {
            if let Ok(new_lang) = crate::overlay::realtime_webview::NEW_TARGET_LANGUAGE.lock() {
                if !new_lang.is_empty() {
                    target_language = new_lang.clone();
                    
                    // Clear history to prevent context poisoning, but keep existing translations on screen
                    if let Ok(mut s) = state.lock() {
                        s.translation_history.clear();
                    }
                }
            }
            crate::overlay::realtime_webview::LANGUAGE_CHANGE.store(false, Ordering::SeqCst);
        }
        
        // Check for translation model change
        if crate::overlay::realtime_webview::TRANSLATION_MODEL_CHANGE.load(Ordering::SeqCst) {
            // Model change doesn't need to clear state - just let next translation use the new model
            // The model is read from config on each translation iteration anyway
            crate::overlay::realtime_webview::TRANSLATION_MODEL_CHANGE.store(false, Ordering::SeqCst);
        }
        
        if last_run.elapsed() >= interval {
            // Check visibility - avoid burning API requests if hidden
            if !crate::overlay::realtime_webview::TRANS_VISIBLE.load(Ordering::SeqCst) {
                 last_run = Instant::now();
                 std::thread::sleep(Duration::from_millis(500));
                 continue;
            }

            // Check if transcript has grown since last translation
            let (chunk, has_finished, is_unchanged) = {
                let s = state.lock().unwrap();
                
                // If the transcript hasn't grown since last time, skip entirely.
                // The previous uncommitted translation is still valid on screen.
                if s.is_transcript_unchanged() {
                    (None, false, true)
                } else {
                    // It has changed/grown. Get the new chunk.
                    match s.get_translation_chunk() {
                        Some((text, has_finished)) => (Some(text), has_finished, false),
                        None => (None, false, true)
                    }
                }
            };
            
            // Skip if chunk is unchanged since last translation
            if is_unchanged {
                last_run = Instant::now();
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            
            if let Some(chunk) = chunk {
                
                // Mark transcript length as processed and clear uncommitted internally
                // DON'T post UI update here - keep old partial visible until new chunk arrives
                // JS will detect fullText !== totalChunkText and do atomic rebuild
                {
                    let mut s = state.lock().unwrap();
                    s.update_last_processed_len();
                    s.start_new_translation();
                    // No UI update here - avoids empty flash
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
                
                // Determine current model strategy
                let current_model = translation_model.as_str();
                let mut primary_failed = false;

                // --- 1. PRIMARY ATTEMPT ---
                if current_model == "google-gtx" {
                    // Strategy: Google GTX (Unlimited, No Key)
                    if let Some(text) = translate_with_google_gtx(&chunk, &target_language) {
                        if let Ok(mut s) = state.lock() {
                            s.append_translation(&text);
                            let display = s.display_translation.clone();
                            update_translation_text(translation_hwnd, &display);
                            if has_finished { s.commit_finished_sentences(); }
                        }
                    } else {
                        primary_failed = true;
                    }
                } else {
                    // Strategy: LLM (Groq or Gemma)
                    let is_google = current_model == "google-gemma";
                    let (url, model_name, api_key) = if is_google {
                        (
                            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string(),
                            "gemma-3-27b-it".to_string(),
                            gemini_key.clone()
                        )
                    } else {
                        (
                            "https://api.groq.com/openai/v1/chat/completions".to_string(),
                            "llama-3.1-8b-instant".to_string(),
                            groq_key.clone()
                        )
                    };

                    // Construct messages
                    let mut messages: Vec<serde_json::Value> = Vec::new();
                    let system_instruction = format!(
                        "You are a professional translator. Translate text to {} to append suitably to the context. Output ONLY the translation, nothing else.",
                        target_language
                    );

                    if is_google {
                        // Google Gemma manual instruction embedding
                        messages.extend(history_messages.clone());
                        messages.push(serde_json::json!({
                            "role": "user",
                            "content": format!("{}\n\nTranslate to {}:\n{}", system_instruction, target_language, chunk)
                        }));
                    } else {
                        // Groq/Others standard system role
                        messages.push(serde_json::json!({ "role": "system", "content": system_instruction }));
                        messages.extend(history_messages.clone());
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

                        match UREQ_AGENT.post(&url)
                            .set("Authorization", &format!("Bearer {}", api_key))
                            .set("Content-Type", "application/json")
                            .send_json(payload) 
                        {
                            Ok(resp) => {
                                // Track usage for llama-3.1-8b-instant (Groq)
                                if !is_google {
                                    if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
                                        let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
                                        let usage_str = format!("{} / {}", remaining, limit);
                                        if let Ok(mut app) = APP.lock() {
                                            app.model_usage_stats.insert("llama-3.1-8b-instant".to_string(), usage_str);
                                        }
                                    }
                                }
                                
                                // Streaming Loop
                                let reader = std::io::BufReader::new(resp.into_reader());
                                let mut full_translation = String::new();
                                for line in reader.lines().flatten() {
                                    if stop_signal.load(Ordering::Relaxed) { break; }
                                    if line.starts_with("data: ") {
                                        let json_str = &line["data: ".len()..];
                                        if json_str.trim() == "[DONE]" { break; }
                                        if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                                            if let Some(content) = chunk_resp.get("choices").and_then(|c| c.as_array())
                                                .and_then(|a| a.first()).and_then(|f| f.get("delta"))
                                                .and_then(|d| d.get("content")).and_then(|t| t.as_str()) 
                                            {
                                                full_translation.push_str(content);
                                                if let Ok(mut s) = state.lock() {
                                                    s.append_translation(content);
                                                    let display = s.display_translation.clone();
                                                    update_translation_text(translation_hwnd, &display);
                                                }
                                            }
                                        }
                                    }
                                }
                                if has_finished && !full_translation.is_empty() {
                                    if let Ok(mut s) = state.lock() { s.commit_finished_sentences(); }
                                }
                            }
                            Err(_) => { primary_failed = true; }
                        }
                    } else {
                        primary_failed = true; // No key
                    }
                }

                // --- 2. RETRY WITH RANDOM FALLBACK ---
                if primary_failed {
                    // Fallback Logic: Groq<->GTX, Gemini->Random
                    let alt_model = if current_model == "groq-llama" {
                        "google-gtx"
                    } else if current_model == "google-gtx" {
                        "groq-llama"
                    } else {
                        let pool = ["groq-llama", "google-gtx"];
                        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
                        pool[(nanos as usize) % pool.len()]
                    };
                    
                    if true {

                        // Config update & UI Signal
                        {
                            let mut app = APP.lock().unwrap();
                            app.config.realtime_translation_model = alt_model.to_string();
                            crate::config::save_config(&app.config);
                        }
                        unsafe {
                            let flag = match alt_model { "google-gemma" => 1, "google-gtx" => 2, _ => 0 };
                            let _ = PostMessageW(Some(translation_hwnd), WM_MODEL_SWITCH, WPARAM(flag), LPARAM(0));
                        }

                        // Execute Retry
                        if alt_model == "google-gtx" {
                            if let Some(text) = translate_with_google_gtx(&chunk, &target_language) {
                                if let Ok(mut s) = state.lock() {
                                    s.append_translation(&text);
                                    let display = s.display_translation.clone();
                                    update_translation_text(translation_hwnd, &display);
                                    if has_finished { s.commit_finished_sentences(); }
                                }
                            }
                        } else {
                            // Retry with LLM (Groq/Gemma)
                            let alt_is_google = alt_model == "google-gemma";
                            let (alt_url, alt_model_name, alt_key) = if alt_is_google {
                                ("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string(), "gemma-3-27b-it".to_string(), gemini_key.clone())
                            } else {
                                ("https://api.groq.com/openai/v1/chat/completions".to_string(), "llama-3.1-8b-instant".to_string(), groq_key.clone())
                            };

                            if !alt_key.is_empty() {
                                let mut alt_msgs = Vec::new();
                                let alt_sys = format!("You are a professional translator. Translate text to {} to append suitably to the context. Output ONLY the translation, nothing else.", target_language);
                                
                                if alt_is_google {
                                    alt_msgs.extend(history_messages.clone());
                                    alt_msgs.push(serde_json::json!({
                                        "role": "user", 
                                        "content": format!("{}\n\nTranslate to {}:\n{}", alt_sys, target_language, chunk)
                                    }));
                                } else {
                                    alt_msgs.push(serde_json::json!({ "role": "system", "content": alt_sys }));
                                    alt_msgs.extend(history_messages.clone());
                                    alt_msgs.push(serde_json::json!({ "role": "user", "content": format!("Translate to {}:\n{}", target_language, chunk) }));
                                }

                                let payload = serde_json::json!({ "model": alt_model_name, "messages": alt_msgs, "stream": true, "max_tokens": 512 });
                                
                                if let Ok(resp) = UREQ_AGENT.post(&alt_url).set("Authorization", &format!("Bearer {}", alt_key)).set("Content-Type", "application/json").send_json(payload) {
                                    // Track usage for llama-3.1-8b-instant (Groq fallback)
                                    if !alt_is_google {
                                        if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
                                            let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
                                            let usage_str = format!("{} / {}", remaining, limit);
                                            if let Ok(mut app) = APP.lock() {
                                                app.model_usage_stats.insert("llama-3.1-8b-instant".to_string(), usage_str);
                                            }
                                        }
                                    }
                                    
                                    let reader = std::io::BufReader::new(resp.into_reader());
                                    let mut full_t = String::new();
                                    for line in reader.lines().flatten() {
                                        if stop_signal.load(Ordering::Relaxed) { break; }
                                        if line.starts_with("data: ") {
                                            let json_str = &line["data: ".len()..];
                                            if json_str.trim() == "[DONE]" { break; }
                                            if let Ok(c) = serde_json::from_str::<serde_json::Value>(json_str) {
                                                if let Some(txt) = c.get("choices").and_then(|a| a.as_array()).and_then(|v| v.first()).and_then(|f| f.get("delta")).and_then(|d| d.get("content")).and_then(|s| s.as_str()) {
                                                    full_t.push_str(txt);
                                                    if let Ok(mut s) = state.lock() {
                                                        s.append_translation(txt);
                                                        let d = s.display_translation.clone();
                                                        update_translation_text(translation_hwnd, &d);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if has_finished && !full_t.is_empty() {
                                        if let Ok(mut s) = state.lock() { s.commit_finished_sentences(); }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            last_run = Instant::now();
        }
        
        std::thread::sleep(Duration::from_millis(100));
    }
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
    
    // Force update regardless of visibility flag to prevent state desync
    // if !crate::overlay::realtime_webview::MIC_VISIBLE.load(Ordering::SeqCst) {
    //     return;
    // }
    
    // Post message to trigger repaint
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_REALTIME_UPDATE, WPARAM(0), LPARAM(0));
    }
}

/// Update translation overlay with new text
fn update_translation_text(hwnd: HWND, text: &str) {
    if let Ok(mut display) = TRANSLATION_DISPLAY_TEXT.lock() {
        *display = text.to_string();
    }
    
    // Force update regardless of visibility flag
    // if !crate::overlay::realtime_webview::TRANS_VISIBLE.load(Ordering::SeqCst) {
    //     return;
    // }
    
    // Post message to trigger repaint
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_TRANSLATION_UPDATE, WPARAM(0), LPARAM(0));
    }
}

/// Unofficial Google Translate (GTX) fallback
/// "Unlimited" for personal desktop use (IP-based rate limits are very high)
fn translate_with_google_gtx(text: &str, target_lang: &str) -> Option<String> {
    // Convert full language name to ISO code
    let target_code = isolang::Language::from_name(target_lang)
        .and_then(|lang| lang.to_639_1())
        .map(|code| code.to_string())
        .unwrap_or_else(|| "en".to_string());

    let encoded_text = urlencoding::encode(text);
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={}&dt=t&q={}",
        target_code, encoded_text
    );

    match UREQ_AGENT.get(&url)
        .set("User-Agent", "Mozilla/5.0") // Mimic browser
        .timeout(std::time::Duration::from_secs(10))
        .call()
    {
        Ok(resp) => {
            if let Ok(json) = resp.into_json::<serde_json::Value>() {
                // Response is [[["Translated","Source",...], ...], ...]
                if let Some(sentences) = json.get(0).and_then(|v| v.as_array()) {
                    let mut full_text = String::new();
                    for sentence_node in sentences {
                        if let Some(segment) = sentence_node.get(0).and_then(|s| s.as_str()) {
                            full_text.push_str(segment);
                        }
                    }
                    if !full_text.is_empty() {
                        return Some(full_text);
                    }
                }
            }
        }
        Err(_) => {}
    }
    None
}


