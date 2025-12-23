//! Text-to-Speech using Gemini Live API
//!
//! This module provides persistent TTS capabilities using Gemini's native
//! audio model. The WebSocket connection is maintained at app startup
//! for instant speech synthesis with minimal latency.

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}, Mutex, Condvar};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use std::sync::mpsc;
use lazy_static::lazy_static;

use crate::APP;

/// Model for TTS (same native audio model, configured for output only)
const TTS_MODEL: &str = "gemini-2.5-flash-native-audio-preview-12-2025";

/// Output audio sample rate from Gemini (24kHz)
const SOURCE_SAMPLE_RATE: u32 = 24000;

/// Playback sample rate (48kHz - most devices support this)
const PLAYBACK_SAMPLE_RATE: u32 = 48000;

/// Events passed from socket workers to the player thread
enum AudioEvent {
    Data(Vec<u8>),
    End,
}

/// Request paired with its generation ID (to handle interrupts)
#[derive(Clone)]
struct QueuedRequest {
    req: TtsRequest,
    generation: u64,
}

/// TTS request with unique ID for cancellation
#[derive(Clone)]
pub struct TtsRequest {
    pub id: u64,
    pub text: String,
    pub hwnd: isize, // Window handle to update state when audio starts
}

/// Global TTS manager - singleton pattern for persistent connection
lazy_static! {
    /// The global TTS connection manager
    pub static ref TTS_MANAGER: Arc<TtsManager> = Arc::new(TtsManager::new());
    
    /// Counter for generating unique request IDs
    static ref REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
}

/// Manages the persistent TTS WebSocket connection
pub struct TtsManager {
    /// Flag to indicate if the connection is ready
    is_ready: AtomicBool,
    
    /// Queue for Socket Workers: (Request + Generation, Output Channel)
    work_queue: Mutex<VecDeque<(QueuedRequest, mpsc::Sender<AudioEvent>)>>,
    /// Signal for Socket Workers
    work_signal: Condvar,

    /// Queue for Player: (Input Channel, Window Handle, Request ID)
    playback_queue: Mutex<VecDeque<(mpsc::Receiver<AudioEvent>, isize, u64)>>,
    /// Signal for Player
    playback_signal: Condvar,

    /// Generation counter for interrupts (incrementing this invalidates old jobs)
    interrupt_generation: AtomicU64,
    
    /// Flag to shutdown the manager
    shutdown: AtomicBool,
}

impl TtsManager {
    pub fn new() -> Self {
        Self {
            is_ready: AtomicBool::new(false),
            work_queue: Mutex::new(VecDeque::new()),
            work_signal: Condvar::new(),
            playback_queue: Mutex::new(VecDeque::new()),
            playback_signal: Condvar::new(),
            interrupt_generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        }
    }
    
    /// Check if TTS is ready to accept requests
    pub fn is_ready(&self) -> bool {
        self.is_ready.load(Ordering::SeqCst)
    }
    
    /// Request TTS for the given text. Appends to queue (sequential playback).
    /// Returns the request ID.
    pub fn speak(&self, text: &str, hwnd: isize) -> u64 {
        let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let current_gen = self.interrupt_generation.load(Ordering::SeqCst);
        
        let (tx, rx) = mpsc::channel();
        
        // Add to queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.push_back((
                QueuedRequest {
                    req: TtsRequest { id, text: text.to_string(), hwnd },
                    generation: current_gen,
                },
                tx
            ));
        }
        self.work_signal.notify_one();
        
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.push_back((rx, hwnd, id));
        }
        self.playback_signal.notify_one();
        
        id
    }

    /// Request TTS for the given text, interrupting any current speech.
    /// Clears the queue and stops current playback immediately.
    pub fn speak_interrupt(&self, text: &str, hwnd: isize) -> u64 {
        // Increment generation to invalidate all currently running/queued work
        let new_gen = self.interrupt_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        
        // Clear all queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.clear();
        }
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.clear(); // Drops receivers, causing senders to error and workers to reset
        }
        
        // Push new request
        let (tx, rx) = mpsc::channel();
        
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.push_back((
                QueuedRequest {
                    req: TtsRequest { id, text: text.to_string(), hwnd },
                    generation: new_gen,
                },
                tx
            ));
        }
        self.work_signal.notify_one();
        
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.push_back((rx, hwnd, id));
        }
        // Force notify player to wake up and check generation/queue
        self.playback_signal.notify_one();
        
        id
    }
    
    /// Stop the current speech or cancel pending request
    pub fn stop(&self) {
        self.interrupt_generation.fetch_add(1, Ordering::SeqCst);
        
        // Clear queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.clear();
        }
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.clear();
        }
        
        // Wake up player to realize it should stop
        self.playback_signal.notify_all();
    }
    
    /// Stop speech for a specific request ID (only if it's the current one)
    /// Note: With the new parallel architecture, checking "is active" is harder. 
    /// We simply stop everything if the request ID matches the *active* player job.
    /// But typically stop is global. We will assume global stop for simplicity or implement targeted stop later if needed.
    pub fn stop_if_active(&self, _request_id: u64) {
         // Simplified to just stop, as we don't track detailed per-request status efficiently across threads yet
         // and usually UI calls this when the "Stop" button is clicked for a specific item, effectively meaning "Stop Playback"
         self.stop();
    }
    
    /// Check if this request ID is currently active
    /// Note: Approximate check based on presence in queues or player active state would require more tracking.
    /// Returning false for now as this is mainly used for UI state which updates via callbacks anyway.
    pub fn is_speaking(&self, _request_id: u64) -> bool {
        false 
    }
    
    /// Shutdown the TTS manager
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.interrupt_generation.fetch_add(1, Ordering::SeqCst);
        self.work_signal.notify_all();
        self.playback_signal.notify_all();
    }
}

/// Initialize the TTS system - call this at app startup
pub fn init_tts() {
    // Spawn 1 Player Thread
    std::thread::spawn(|| {
        run_player_thread();
    });

    // Spawn 2 Socket Worker Threads (Parallel Fetching)
    for _ in 0..2 {
        std::thread::spawn(|| {
            run_socket_worker();
        });
    }
}

/// Clear the TTS loading state for a window and trigger repaint
fn clear_tts_loading_state(hwnd: isize) {
    use crate::overlay::result::state::WINDOW_STATES;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::InvalidateRect;
    
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd) {
            state.tts_loading = false;
        }
    }
    
    // Trigger repaint to update button appearance
    unsafe {
        InvalidateRect(HWND(hwnd), None, false);
    }
}

/// Clear TTS state completely when speech ends
fn clear_tts_state(hwnd: isize) {
    use crate::overlay::result::state::WINDOW_STATES;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::InvalidateRect;
    
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd) {
            state.tts_loading = false;
            state.tts_request_id = 0;
        }
    }
    
    // Trigger repaint to update button appearance
    unsafe {
        InvalidateRect(HWND(hwnd), None, false);
    }
}

/// Create TLS WebSocket connection to Gemini Live API for TTS
fn connect_tts_websocket(api_key: &str) -> Result<tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>> {
    let ws_url = format!(
        "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={}",
        api_key
    );
    
    let url = url::Url::parse(&ws_url)?;
    let host = url.host_str().ok_or_else(|| anyhow::anyhow!("No host in URL"))?;
    let port = 443;
    
    use std::net::ToSocketAddrs;
    let addr = format!("{}:{}", host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("Failed to resolve hostname: {}", host))?;
    
    let tcp_stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    tcp_stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    tcp_stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    tcp_stream.set_nodelay(true)?;
    
    let connector = native_tls::TlsConnector::new()?;
    let tls_stream = connector.connect(host, tcp_stream)?;
    
    let (socket, _response) = tungstenite::client::client(&ws_url, tls_stream)?;
    
    Ok(socket)
}

/// Send TTS setup message - configures for audio output only, no input transcription
fn send_tts_setup(socket: &mut tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>, voice_name: &str, speed: &str) -> Result<()> {
    
    // System instruction based on speed


    // System instruction based on speed
    let mut system_text = "You are a text-to-speech reader. Your ONLY job is to read the user's text out loud, exactly as written, word for word. Do NOT respond conversationally. Do NOT add commentary. Do NOT ask questions. ".to_string();
    
    match speed {
        "Slow" => system_text.push_str("Speak slowly, clearly, and with deliberate pacing. "),
        "Fast" => system_text.push_str("Speak quickly, efficiently, and with a brisk pace. "),
        _ => system_text.push_str("Simply read the provided text aloud naturally and clearly. "),
    }
    system_text.push_str("Start reading immediately.");

    let setup = serde_json::json!({
        "setup": {
            "model": format!("models/{}", TTS_MODEL),
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": {
                    "voiceConfig": {
                        "prebuiltVoiceConfig": {
                            "voiceName": voice_name
                        }
                    }
                },
                "thinkingConfig": {
                    "thinkingBudget": 0
                }
            },
            "systemInstruction": {
                "parts": [{
                    "text": system_text
                }]
            }
        }
    });
    
    let msg_str = setup.to_string();
    socket.write(tungstenite::Message::Text(msg_str))?;
    socket.flush()?;
    
    Ok(())
}

/// Send text to be spoken
fn send_tts_text(socket: &mut tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>, text: &str) -> Result<()> {
    // Format with explicit instruction to read verbatim
    let prompt = format!("[READ ALOUD VERBATIM - START NOW]\n\n{}", text);
    
    let msg = serde_json::json!({
        "clientContent": {
            "turns": [{
                "role": "user",
                "parts": [{
                    "text": prompt
                }]
            }],
            "turnComplete": true
        }
    });
    
    socket.write(tungstenite::Message::Text(msg.to_string()))?;
    socket.flush()?;
    
    Ok(())
}

/// Parse audio data from WebSocket message
fn parse_audio_data(msg: &str) -> Option<Vec<u8>> {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(msg) {
        // Check for serverContent -> modelTurn -> parts -> inlineData
        if let Some(server_content) = json.get("serverContent") {
            if let Some(model_turn) = server_content.get("modelTurn") {
                if let Some(parts) = model_turn.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(inline_data) = part.get("inlineData") {
                            if let Some(data_b64) = inline_data.get("data").and_then(|d| d.as_str()) {
                                if let Ok(audio_bytes) = general_purpose::STANDARD.decode(data_b64) {
                                    return Some(audio_bytes);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Check if the response indicates turn is complete
fn is_turn_complete(msg: &str) -> bool {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(msg) {
        if let Some(server_content) = json.get("serverContent") {
            // Check for turnComplete
            if let Some(turn_complete) = server_content.get("turnComplete") {
                if turn_complete.as_bool().unwrap_or(false) {
                    return true;
                }
            }
            // Also check for generationComplete (seen in TTS responses)
            if let Some(gen_complete) = server_content.get("generationComplete") {
                if gen_complete.as_bool().unwrap_or(false) {
                    return true;
                }
            }
        }
    }
    false
}

/// Main Player thread - consumes audio streams sequentially
fn run_player_thread() {
    let manager = &*TTS_MANAGER;
    // Create ONE persistent audio player
    // This avoids the overhead of opening the audio device for every request
    let audio_player = AudioPlayer::new(PLAYBACK_SAMPLE_RATE);
    
    loop {
        if manager.shutdown.load(Ordering::SeqCst) { break; }
        
        let playback_job = {
            let mut pq = manager.playback_queue.lock().unwrap();
            while pq.is_empty() && !manager.shutdown.load(Ordering::SeqCst) {
                 let result = manager.playback_signal.wait(pq).unwrap();
                 pq = result;
            }
            if manager.shutdown.load(Ordering::SeqCst) { return; }
            pq.pop_front()
        };
        
        if let Some((rx, hwnd, _req_id)) = playback_job {
             let mut loading_cleared = false;
             
             // Loop reading chunks from this channel
             // This blocks if the worker is buffering (which is what we want)
             loop {
                 match rx.recv() {
                     Ok(AudioEvent::Data(data)) => {
                         if !loading_cleared {
                             loading_cleared = true;
                             clear_tts_loading_state(hwnd);
                         }
                         audio_player.play(&data);
                     }
                     Ok(AudioEvent::End) => {
                         audio_player.drain();
                         clear_tts_state(hwnd);
                         break; // Job done
                     }
                     Err(_) => {
                         // Sender disconnected (likely worker aborted due to interrupt or network error)
                         // Stop immediately
                         audio_player.drain(); // Or flush? Draining is safer to finish partials.
                         clear_tts_state(hwnd);
                         break;
                     }
                 }
                 
                 if manager.shutdown.load(Ordering::SeqCst) { return; }
             }
        }
    }
}

/// Socket Worker thread - fetches audio data and pipes it to the player
fn run_socket_worker() {
    let manager = &*TTS_MANAGER;
    
    // Delay start slightly to stagger connections if multiple workers start at once
    std::thread::sleep(Duration::from_millis(100));
    
    loop {
        if manager.shutdown.load(Ordering::SeqCst) {
            break;
        }
        
        // Wait for a request
        let (request, tx) = {
            let mut queue = manager.work_queue.lock().unwrap();
            while queue.is_empty() && !manager.shutdown.load(Ordering::SeqCst) {
                let result = manager.work_signal.wait(queue).unwrap();
                queue = result;
            }
            if manager.shutdown.load(Ordering::SeqCst) {
                return;
            }
            queue.pop_front().unwrap()
        };
        
        // Check if this request is stale (interrupted before we picked it up)
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            // Signal end immediately so player unblocks and drops it
            let _ = tx.send(AudioEvent::End);
            continue;
        }
        
        // Get API key
        let api_key = {
            match APP.lock() {
                Ok(app) => app.config.gemini_api_key.clone(),
                Err(_) => {
                    let _ = tx.send(AudioEvent::End);
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
        };
        
        if api_key.trim().is_empty() {
            // No API key configured
            eprintln!("TTS: No Gemini API key configured");
            let _ = tx.send(AudioEvent::End);
            clear_tts_loading_state(request.req.hwnd); // Ensure loading is cleared
            clear_tts_state(request.req.hwnd);
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        
        // Attempt to connect
        let socket_result = connect_tts_websocket(&api_key);
        let mut socket = match socket_result {
            Ok(s) => s,
            Err(e) => {
                eprintln!("TTS: Failed to connect: {}", e);
                let _ = tx.send(AudioEvent::End);
                clear_tts_loading_state(request.req.hwnd); // Ensure loading is cleared
                clear_tts_state(request.req.hwnd);
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
        };
        
        // Read config for setup
        let (current_voice, current_speed) = {
             let app = APP.lock().unwrap();
             (app.config.tts_voice.clone(), app.config.tts_speed.clone())
        };

        // Send setup
        if let Err(e) = send_tts_setup(&mut socket, &current_voice, &current_speed) {
            eprintln!("TTS: Failed to send setup: {}", e);
            let _ = socket.close(None);
            let _ = tx.send(AudioEvent::End);
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        
        // Wait for setup acknowledgment (blocking mode)
        let setup_start = Instant::now();
        let mut setup_complete = false;
        loop {
            // Check interruption during setup
            if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) || manager.shutdown.load(Ordering::SeqCst) {
                 let _ = socket.close(None);
                 let _ = tx.send(AudioEvent::End);
                 break; // break inner setup loop
            }

            match socket.read() {
                Ok(tungstenite::Message::Text(msg)) => {
                    if msg.contains("setupComplete") {
                        setup_complete = true;
                        break;
                    }
                    if msg.contains("error") || msg.contains("Error") {
                        eprintln!("TTS: Setup error: {}", msg);
                        break;
                    }
                }
                Ok(tungstenite::Message::Close(_)) => { break; }
                Ok(tungstenite::Message::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data) {
                        if text.contains("setupComplete") { setup_complete = true; break; }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                     if setup_start.elapsed() > Duration::from_secs(10) { break; }
                     std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => { break; }
            }
        }
        
        if manager.shutdown.load(Ordering::SeqCst) { return; }
        
        if !setup_complete {
            let _ = socket.close(None);
            let _ = tx.send(AudioEvent::End); 
            continue;
        }
        
        // Connection ready
        // manager.is_ready.store(true, Ordering::SeqCst); // No longer purely accurate with multiple workers, but fine
        
        // Send request text
        if let Err(e) = send_tts_text(&mut socket, &request.req.text) {
             eprintln!("TTS: Failed to send text: {}", e);
             let _ = tx.send(AudioEvent::End);
             let _ = socket.close(None);
             continue;
        }
        
        // Read loop
        loop {
            // CHECK INTERRUPT
            if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) || manager.shutdown.load(Ordering::SeqCst) {
                // Abort!
                let _ = socket.close(None);
                // Drop tx mostly handles it, but sending End is explicit
                let _ = tx.send(AudioEvent::End);
                break;
            }
            
            match socket.read() {
                Ok(tungstenite::Message::Text(msg)) => {
                    if let Some(audio_data) = parse_audio_data(&msg) {
                        let _ = tx.send(AudioEvent::Data(audio_data));
                    }
                    if is_turn_complete(&msg) {
                        let _ = tx.send(AudioEvent::End);
                        break;
                    }
                }
                Ok(tungstenite::Message::Binary(data)) => {
                     if let Ok(text) = String::from_utf8(data) {
                        if let Some(audio_data) = parse_audio_data(&text) {
                             let _ = tx.send(AudioEvent::Data(audio_data));
                        }
                        if is_turn_complete(&text) {
                            let _ = tx.send(AudioEvent::End);
                            break;
                        }
                     }
                }
                Ok(tungstenite::Message::Close(_)) => {
                    let _ = tx.send(AudioEvent::End);
                    break;
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => {
                    eprintln!("TTS: Read error: {}", e);
                    let _ = tx.send(AudioEvent::End);
                    break;
                }
            }
        }
        
        // Close socket after turn (to avoid context build up)
        let _ = socket.close(None);
    }
}

/// Simple audio player using Windows Audio API
struct AudioPlayer {
    #[allow(dead_code)]
    sample_rate: u32,
    // Audio buffer for accumulating samples
    buffer: Vec<u8>,
    // Handle to Windows audio stream (cpal)
    stream: Option<cpal::Stream>,
    // Shared buffer for audio data
    shared_buffer: Arc<Mutex<VecDeque<i16>>>,
}

impl AudioPlayer {
    fn new(sample_rate: u32) -> Self {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        
        let shared_buffer: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
        let buffer_clone = shared_buffer.clone();
        
        // Use WASAPI explicitly on Windows for better compatibility
        #[cfg(target_os = "windows")]
        let host = cpal::host_from_id(cpal::HostId::Wasapi).unwrap_or(cpal::default_host());
        #[cfg(not(target_os = "windows"))]
        let host = cpal::default_host();
        
        let device = host.default_output_device();
        
        if device.is_none() {
            eprintln!("TTS: No audio output device found!");
        }
        
        let stream = device.and_then(|device| {
            
            // Try to get supported configs for debugging
            // if let Ok(configs) = device.supported_output_configs() {
            //     for cfg in configs {
            //         eprintln!("TTS: Supported config: {:?}", cfg);
            //     }
            // }
            
            // Try f32 format first (more commonly supported)
            // Use stereo (2 channels) since many devices don't support mono
            let config = cpal::StreamConfig {
                channels: 2,
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Default,
            };
            
            // Clone for the f32 closure
            let buffer_clone_f32 = buffer_clone.clone();
            
            // Try building with f32 format
            match device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buf = buffer_clone_f32.lock().unwrap();
                    // For stereo, output same sample to both channels
                    for frame in data.chunks_mut(2) {
                        let i16_sample = buf.pop_front().unwrap_or(0);
                        let sample = i16_sample as f32 / 32768.0;
                        frame[0] = sample; // Left
                        frame[1] = sample; // Right (same as left for mono source)
                    }
                },
                |err| eprintln!("TTS Audio error: {}", err),
                None,
            ) {
                Ok(stream) => {
                    Some(stream)
                }
                Err(e) => {
                    eprintln!("TTS: Failed to create f32 stream: {}", e);
                    // Try i16 format as fallback
                    match device.build_output_stream(
                        &config,
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            let mut buf = buffer_clone.lock().unwrap();
                            // For stereo, output same sample to both channels
                            for frame in data.chunks_mut(2) {
                                let sample = buf.pop_front().unwrap_or(0);
                                frame[0] = sample; // Left
                                frame[1] = sample; // Right
                            }
                        },
                        |err| eprintln!("TTS Audio error: {}", err),
                        None,
                    ) {
                        Ok(stream) => {
                            Some(stream)
                        }
                        Err(e2) => {
                            eprintln!("TTS: Failed to create i16 stream: {}", e2);
                            None
                        }
                    }
                }
            }
        });
        
        if stream.is_none() {
            eprintln!("TTS: Failed to create audio stream!");
        }
        
        if let Some(ref s) = stream {
            if let Err(e) = s.play() {
                eprintln!("TTS: Failed to start stream: {}", e);
            }
        }
        
        Self {
            sample_rate,
            buffer: Vec::new(),
            stream,
            shared_buffer,
        }
    }
    
    fn play(&self, audio_data: &[u8]) {
        // Convert raw PCM bytes to i16 samples (little-endian)
        // Also upsample from 24kHz to 48kHz by duplicating each sample
        let mut samples = Vec::with_capacity(audio_data.len()); // 2x because of upsampling
        for chunk in audio_data.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            // Duplicate each sample for 2x upsampling (24kHz -> 48kHz)
            samples.push(sample);
            samples.push(sample);
        }
        
        // Add to shared buffer
        if let Ok(mut buf) = self.shared_buffer.lock() {
            buf.extend(samples);
        }
    }
    
    fn drain(&self) {
        // Wait for buffer to drain
        loop {
            let len = self.shared_buffer.lock().map(|b| b.len()).unwrap_or(0);
            if len == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Extra grace period for audio hardware
        std::thread::sleep(Duration::from_millis(100));
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        // Stream will be stopped when dropped
    }
}
