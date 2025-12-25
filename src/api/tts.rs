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
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

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
    pub _id: u64,
    pub text: String,
    pub hwnd: isize, // Window handle to update state when audio starts
    pub is_realtime: bool, // True if this is from realtime translation (uses REALTIME_TTS_SPEED)
}

lazy_static! {
    /// The global TTS connection manager
    pub static ref TTS_MANAGER: Arc<TtsManager> = Arc::new(TtsManager::new());
    
    /// Counter for generating unique request IDs
    static ref REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
}

/// Manages the persistent TTS WebSocket connection
pub struct TtsManager {
    /// Flag to indicate if the connection is ready
    _is_ready: AtomicBool,
    
    /// Queue for Socket Workers: (Request + Generation, Output Channel)
    work_queue: Mutex<VecDeque<(QueuedRequest, mpsc::Sender<AudioEvent>)>>,
    /// Signal for Socket Workers
    work_signal: Condvar,

    /// Queue for Player: (Input Channel, Window Handle, Request ID, Generation ID, IsRealtime)
    playback_queue: Mutex<VecDeque<(mpsc::Receiver<AudioEvent>, isize, u64, u64, bool)>>,
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
            _is_ready: AtomicBool::new(false),
            work_queue: Mutex::new(VecDeque::new()),
            work_signal: Condvar::new(),
            playback_queue: Mutex::new(VecDeque::new()),
            playback_signal: Condvar::new(),
            interrupt_generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        }
    }
    
    /// Check if TTS is ready to accept requests
    pub fn _is_ready(&self) -> bool {
        self._is_ready.load(Ordering::SeqCst)
    }
    
    /// Request TTS for the given text. Appends to queue (sequential playback).
    /// Returns the request ID.
    pub fn speak(&self, text: &str, hwnd: isize) -> u64 {
        self.speak_internal(text, hwnd, false)
    }
    
    /// Request TTS for realtime translation. Uses REALTIME_TTS_SPEED and auto-catchup.
    /// Returns the request ID.
    pub fn speak_realtime(&self, text: &str, hwnd: isize) -> u64 {
        self.speak_internal(text, hwnd, true)
    }
    
    /// Internal speak implementation
    fn speak_internal(&self, text: &str, hwnd: isize, is_realtime: bool) -> u64 {
        let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let current_gen = self.interrupt_generation.load(Ordering::SeqCst);
        
        let (tx, rx) = mpsc::channel();
        
        // Add to queues
        {
            let mut wq = self.work_queue.lock().unwrap();
            wq.push_back((
                QueuedRequest {
                    req: TtsRequest { _id: id, text: text.to_string(), hwnd, is_realtime },
                    generation: current_gen,
                },
                tx
            ));
        }
        self.work_signal.notify_one();
        
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.push_back((rx, hwnd, id, current_gen, is_realtime));
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
                    req: TtsRequest { _id: id, text: text.to_string(), hwnd, is_realtime: false },
                    generation: new_gen,
                },
                tx
            ));
        }
        self.work_signal.notify_one();
        
        {
            let mut pq = self.playback_queue.lock().unwrap();
            pq.push_back((rx, hwnd, id, new_gen, false));
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
    pub fn _shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.interrupt_generation.fetch_add(1, Ordering::SeqCst);
        self.work_signal.notify_all();
        self.playback_signal.notify_all();
    }
    
    /// List available audio output devices (ID, Name)
    pub fn get_output_devices() -> Vec<(String, String)> {
        AudioPlayer::get_output_devices()
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
        let _ = InvalidateRect(Some(HWND(hwnd as *mut std::ffi::c_void)), None, false);
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
        let _ = InvalidateRect(Some(HWND(hwnd as *mut std::ffi::c_void)), None, false);
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
fn send_tts_setup(socket: &mut tungstenite::WebSocket<native_tls::TlsStream<TcpStream>>, voice_name: &str, speed: &str, custom_instructions: &str) -> Result<()> {
    // System instruction based on speed
    let mut system_text = "You are a text-to-speech reader. Your ONLY job is to read the user's text out loud, exactly as written, word for word. Do NOT respond conversationally. Do NOT add commentary. Do NOT ask questions. ".to_string();
    
    match speed {
        "Slow" => system_text.push_str("Speak slowly, clearly, and with deliberate pacing. "),
        "Fast" => system_text.push_str("Speak quickly, efficiently, and with a brisk pace. "),
        _ => system_text.push_str("Simply read the provided text aloud naturally and clearly. "),
    }
    
    // Append custom tone/style instructions if provided
    if !custom_instructions.trim().is_empty() {
        system_text.push_str(" Additional instructions: ");
        system_text.push_str(custom_instructions.trim());
        system_text.push(' ');
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
        
        if let Some((rx, hwnd, _req_id, generation, is_realtime)) = playback_job {
             let mut loading_cleared = false;
             
             // Loop reading chunks from this channel
             // This blocks if the worker is buffering (which is what we want)
             loop {
                 // Check interrupt before blocking? No, wait for data or close.
                 
                 match rx.recv() {
                     Ok(AudioEvent::Data(data)) => {
                         // Check interrupt before playing
                         if generation < manager.interrupt_generation.load(Ordering::SeqCst) {
                             audio_player.stop();
                             clear_tts_state(hwnd);
                             break;
                         }

                         if !loading_cleared {
                             loading_cleared = true;
                             clear_tts_loading_state(hwnd);
                         }
                         audio_player.play(&data, is_realtime);
                     }
                     Ok(AudioEvent::End) => {
                         // Check if we were interrupted or finished normally
                         if generation < manager.interrupt_generation.load(Ordering::SeqCst) {
                             audio_player.stop(); // Immediate cut-off
                         } else {
                             audio_player.drain(); // Normal finish
                         }
                         clear_tts_state(hwnd);
                         break; // Job done
                     }
                     Err(_) => {
                         // Sender disconnected
                         // If interrupted, stop immediately
                         if generation < manager.interrupt_generation.load(Ordering::SeqCst) {
                             audio_player.stop();
                         } else {
                             audio_player.drain(); // Or flush?
                         }
                         clear_tts_state(hwnd);
                         break;
                     }
                 }
                 
                 if manager.shutdown.load(Ordering::SeqCst) { return; }
                 
                 // Check interrupt again
                 if generation < manager.interrupt_generation.load(Ordering::SeqCst) {
                      audio_player.stop();
                      clear_tts_state(hwnd); 
                      break; 
                 }
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
        
        // Read config for setup - handle realtime vs regular TTS
        let (current_voice, current_speed, custom_instructions) = {
            let app = APP.lock().unwrap();
            let voice = app.config.tts_voice.clone();
            let instructions = app.config.tts_system_instructions.clone();
            
            if request.req.is_realtime {
                // For realtime TTS: AI always speaks at normal pace
                // Playback speed adjustment is done in the audio player
                (voice, "Normal".to_string(), instructions)
            } else {
                // Regular TTS: Use app config speed
                (voice, app.config.tts_speed.clone(), instructions)
            }
        };

        // Send setup
        if let Err(e) = send_tts_setup(&mut socket, &current_voice, &current_speed, &custom_instructions) {
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

/// Simple audio player using Windows WASAPI with loopback exclusion
/// Uses AudioClientProperties to prevent TTS from being captured by loopback
struct AudioPlayer {
    _sample_rate: u32,
    // Shared buffer for audio data (thread-safe)
    shared_buffer: Arc<Mutex<VecDeque<i16>>>,
    // Shutdown signal for the player thread
    shutdown: Arc<AtomicBool>,
    // Player thread handle
    _thread: Option<std::thread::JoinHandle<()>>,
    // WSOLA time stretcher for pitch-preserving speed control
    wsola: Mutex<WsolaStretcher>,
}

impl AudioPlayer {
    fn new(sample_rate: u32) -> Self {
        let shared_buffer: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
        let buffer_clone = shared_buffer.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();
        
        // Read config for device ID
        let target_device_id = {
             if let Ok(app) = crate::APP.lock() {
                 let id = app.config.tts_output_device.clone();
                 if id.is_empty() { None } else { Some(id) }
             } else {
                 None
             }
        };
        
        // Spawn a dedicated thread for WASAPI playback
        // This is needed because WASAPI requires COM initialization on the same thread
        let thread = std::thread::spawn(move || {
            // Initialize COM for this thread
            if wasapi::initialize_mta().is_err() {
                eprintln!("TTS: Failed to initialize COM");
                return;
            }
            
            // Try to create an AudioClient with loopback exclusion
            let result = Self::create_excluded_stream(sample_rate, buffer_clone.clone(), shutdown_clone.clone(), target_device_id);
            
            if let Err(e) = result {
                eprintln!("TTS: WASAPI with exclusion failed ({}), falling back to cpal", e);
                // Fallback to cpal (which doesn't have exclusion but works everywhere)
                // Note: CPAL fallback doesn't support custom device selection easily here without rewrite 
                // so we only use custom device in WASAPI path for now.
                // Self::run_cpal_fallback(sample_rate, buffer_clone, shutdown_clone);
            }
        });
        
        Self {
            _sample_rate: sample_rate,
            shared_buffer,
            shutdown,
            _thread: Some(thread),
            wsola: Mutex::new(WsolaStretcher::new(SOURCE_SAMPLE_RATE)),
        }
    }
    
    /// Create audio stream for playback
    /// NOTE: Loopback exclusion (AUDCLNT_STREAMOPTIONS_EXCLUDE_FROM_SESSION) requires
    /// windows crate v0.52+ which has breaking changes. For windows v0.48, we use
    /// the cpal fallback. TTS audio may be captured by loopback.
    ///
    /// Workaround for the feedback loop:
    /// - Use headphones for TTS output when capturing device audio
    /// Create audio stream with loopback exclusion
    fn create_excluded_stream(
        _sample_rate: u32,
        shared_buffer: Arc<Mutex<VecDeque<i16>>>,
        shutdown: Arc<AtomicBool>,
        target_device_id: Option<String>,
    ) -> anyhow::Result<()> {
        let buffer_clone = shared_buffer.clone();
        let shutdown_clone = shutdown.clone();
        
        // Attempt WASAPI with exclusion
        std::thread::spawn(move || {
            if let Err(e) = unsafe { Self::run_wasapi_excluded(_sample_rate, buffer_clone.clone(), shutdown_clone.clone(), target_device_id) } {
                eprintln!("TTS: WASAPI exclusion FAILED with error: {:?}. Call ended.", e);
            }
        });
        
        Ok(())
    }

    /// List available audio output devices (ID, Name)
    pub fn get_output_devices() -> Vec<(String, String)> {
        let mut devices = Vec::new();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                 if let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                     if let Ok(count) = collection.GetCount() {
                         for i in 0..count {
                             if let Ok(device) = collection.Item(i) {
                                 if let Ok(id) = device.GetId() {
                                     let id_str = id.to_string().unwrap_or_default();
                                     // Try to get friendly name
                                     let name = if let Ok(_props) = device.OpenPropertyStore(STGM_READ) {
                                         // PKEY_Device_FriendlyName is {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
                                         // We use a manual retrieval or just use ID for now if helpers missing
                                         // For now, let's just use a placeholder or partial ID if name fails, 
                                         // but ideally we want the name. 
                                         // In windows 0.62, PropVariant access is verbose. 
                                         // Let's rely on the ID for uniqueness and maybe a simple name hack or just ID.
                                         id_str.clone() 
                                     } else {
                                         id_str.clone()
                                     };
                                     devices.push((id_str, name));
                                 }
                             }
                         }
                     }
                 }
            }
        }
        devices
    }

    unsafe fn run_wasapi_excluded(
        _sample_rate: u32,
        shared_buffer: Arc<Mutex<VecDeque<i16>>>,
        shutdown: Arc<AtomicBool>,
        target_device_id: Option<String>,
    ) ->  anyhow::Result<()> {
        // Use STA for better compatibility with audio drivers
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
        
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        
        let device = if let Some(id_str) = target_device_id {
            // Try to find specific device
             let id_hstring = windows::core::HSTRING::from(id_str);
             enumerator.GetDevice(&id_hstring)?
        } else {
            // Use Console role for TTS (Default)
            enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?
        };
        
        // Activate IAudioClient
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        
        // Note: We no longer try to exclude from loopback (AUDCLNT_STREAMOPTIONS_EXCLUDE_FROM_SESSION)
        // because per-app audio capture solves this problem at the capture side instead.

        let mix_format_ptr = client.GetMixFormat()?;
        let mix_format = *mix_format_ptr;
        
        // Initialize (Shared Mode)
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0, // flags
            1000000, // 100ms buffer
            0,
            mix_format_ptr,
            None
        )?;
        
        let buffer_size = client.GetBufferSize()?;
        let render_client: IAudioRenderClient = client.GetService()?;
        
        client.Start()?;
        
        let channels = mix_format.nChannels as usize;
        let is_float = mix_format.wFormatTag == 3 // WAVE_FORMAT_IEEE_FLOAT
                       || (mix_format.wFormatTag == 65534 // WAVE_FORMAT_EXTENSIBLE 
                          && (mix_format.cbSize >= 22)); 
        
        let _frames_written = 0;
        
        let mut last_gen = TTS_MANAGER.interrupt_generation.load(Ordering::SeqCst);
        
         while !shutdown.load(Ordering::Relaxed) {
             let current_gen = TTS_MANAGER.interrupt_generation.load(Ordering::SeqCst);
             if current_gen > last_gen {
                 if let Ok(mut deck) = shared_buffer.lock() {
                     deck.clear();
                 }
                 last_gen = current_gen;
             }
             let padding = client.GetCurrentPadding()?;
             let available = buffer_size.saturating_sub(padding);
             
             if available > 0 {
                 let buffer_ptr = render_client.GetBuffer(available)?;
                 
                 // Lock inner buffer
                 let mut deck = shared_buffer.lock().unwrap();
                 
                 if is_float {
                     let out_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut f32, (available as usize) * channels);
                     
                     for i in 0..available as usize {
                        if let Some(sample) = deck.pop_front() {
                            let s = (sample as f32) / 32768.0;
                            for c in 0..channels {
                                out_slice[i*channels + c] = s;
                            }
                        } else {
                            // Silence when buffer is empty
                             for c in 0..channels {
                                out_slice[i*channels + c] = 0.0;
                            }
                        }
                     }
                 } else {
                     // PCM i16
                     let out_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut i16, (available as usize) * channels);
                     for i in 0..available as usize {
                        if let Some(sample) = deck.pop_front() {
                            for c in 0..channels {
                                out_slice[i*channels + c] = sample;
                            }
                        } else {
                             for c in 0..channels {
                                out_slice[i*channels + c] = 0;
                            }
                        }
                     }
                 }
                 
                 render_client.ReleaseBuffer(available, 0)?;
            }
             
            std::thread::sleep(Duration::from_millis(10));
        }
        
        client.Stop()?;
        Ok(())
    }
    
    fn play(&self, audio_data: &[u8], is_realtime: bool) {
        // Get effective speed for realtime TTS (or 100 for normal TTS)
        let effective_speed = if is_realtime {
            use crate::overlay::realtime_webview::state::{REALTIME_TTS_SPEED, REALTIME_TTS_AUTO_SPEED, COMMITTED_TRANSLATION_QUEUE, CURRENT_TTS_SPEED, WM_UPDATE_TTS_SPEED, REALTIME_HWND};
            
            let base_speed = REALTIME_TTS_SPEED.load(Ordering::Relaxed);
            let auto_enabled = REALTIME_TTS_AUTO_SPEED.load(Ordering::Relaxed);
            
            // Auto-catchup: boost speed if queue is building up
            let queue_len = COMMITTED_TRANSLATION_QUEUE.lock()
                .map(|q| q.len())
                .unwrap_or(0);
            
            let speed = if auto_enabled && queue_len > 0 {
                // +15% per queued item, up to +60%
                let boost = (queue_len as u32 * 15).min(60);
                (base_speed + boost).min(200)
            } else {
                base_speed
            };
            
            // Update current speed for UI if it changed
            let old_speed = CURRENT_TTS_SPEED.swap(speed, Ordering::Relaxed);
            if old_speed != speed {
                unsafe {
                    use crate::overlay::realtime_webview::state::TRANSLATION_HWND;
                    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
                    use windows::Win32::Foundation::{WPARAM, LPARAM};
                    if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
                        let _ = PostMessageW(Some(REALTIME_HWND), WM_UPDATE_TTS_SPEED, WPARAM(speed as usize), LPARAM(0));
                    }
                    if !std::ptr::addr_of!(TRANSLATION_HWND).read().is_invalid() {
                        let _ = PostMessageW(Some(TRANSLATION_HWND), WM_UPDATE_TTS_SPEED, WPARAM(speed as usize), LPARAM(0));
                    }
                }
            }
            speed
        } else {
            100 // Normal speed for non-realtime TTS
        };
        
        // Speed ratio: 100 = 1.0x, 150 = 1.5x, 50 = 0.5x
        let speed_ratio = effective_speed as f64 / 100.0;
        
        // Convert raw PCM bytes to i16 samples (little-endian)
        let input_samples: Vec<i16> = audio_data.chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        
        if input_samples.is_empty() {
            return;
        }
        

        
        // Apply WSOLA time-stretching for pitch-preserving speed change
        let stretched_samples = if (speed_ratio - 1.0).abs() < 0.05 {
            // Speed is close to 1.0 - no processing needed

            input_samples
        } else {
            // Use WSOLA for pitch-preserving time stretch
            if let Ok(mut wsola) = self.wsola.lock() {
                let result = wsola.stretch(&input_samples, speed_ratio);

                // KEY FIX: If WSOLA returns empty, don't output anything!
                // The samples are buffered in WSOLA and will come out next time.
                // Do NOT mix raw samples with processed samples!
                if result.is_empty() {

                    return; // Wait for more data - don't output anything
                }
                result
            } else {
                input_samples
            }
        };
        
        // Upsample from 24kHz to 48kHz (duplicate each sample)
        let output_samples: Vec<i16> = stretched_samples.iter()
            .flat_map(|&s| [s, s])
            .collect();
        
        // Add to shared buffer
        if let Ok(mut buf) = self.shared_buffer.lock() {
            buf.extend(output_samples);
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
        // Extra grace period
        std::thread::sleep(Duration::from_millis(100));
    }

    fn stop(&self) {
        if let Ok(mut buf) = self.shared_buffer.lock() {
            buf.clear();
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Thread will exit on its own
    }
}

/// Simple OLA (Overlap-Add) time stretcher for pitch-preserving tempo change.
/// Uses Hann window for perfect reconstruction at 50% overlap.
struct WsolaStretcher {
    /// Frame size in samples (20ms at 24kHz = 480 samples)
    frame_size: usize,
    /// Hop size (frame_size / 2 for 50% overlap)
    hop_size: usize,
    /// Hann window
    window: Vec<f32>,
    /// Input buffer for accumulating samples
    pub input_buffer: Vec<f32>,
    /// Output overlap buffer - carries the "tail" that needs to overlap with next chunk
    output_overlap: Vec<f32>,
    /// Search range for alignment (SOLA)
    search_range: usize,
    /// Previous speed ratio (to detect changes)
    last_speed: f64,
}

impl WsolaStretcher {
    fn new(sample_rate: u32) -> Self {
        // 20ms frame size for better streaming with small chunks
        // At 24kHz: 20ms = 480 samples
        let frame_size = (sample_rate as usize * 20) / 1000;
        let hop_size = frame_size / 2; // 50% overlap
        
        // Create Hann window - with 50% overlap, Hann windows sum to exactly 1.0
        // This is crucial for artifact-free overlap-add!
        let window: Vec<f32> = (0..frame_size)
            .map(|i| {
                let t = i as f32 / frame_size as f32;
                // Hann window: 0.5 * (1 - cos(2*pi*t))
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * t).cos())
            })
            .collect();
        
        Self {
            frame_size,
            hop_size,
            window,
            input_buffer: Vec::new(),
            output_overlap: Vec::new(),
            search_range: hop_size / 2, // Search +/- 50% of hop size
            last_speed: 1.0,
        }
    }
    
    /// Find best offset using cross-correlation
    fn find_best_offset(&self, input_pos: usize, target_hop: usize) -> usize {
        // We want to find a shift 'k' such that input[input_pos + k] is most similar to
        // what would have naturally followed the previous frame.
        // However, in standard OLA/WSOLA for time-scale modification, we look for 
        // periodicity.
        // We compare the region around input_pos + target_hop with the region at input_pos + synthesis_hop
        // But since we don't have the previous synthesis output readily mapped back to input in this simple structure,
        // we use a simpler strategy: optimize for continuity (phase alignment).
        
        // Strategy: We want to overlap the END of the previous frame (which is in output buffer)
        // with the BEGINNING of the new frame.
        // Since we don't have easy access to the unfinished output buffer here (it's being built),
        // we use the input signal itself.
        // We look for self-similarity in the input signal around the analysis hop.
        
        // Search range: [target_hop - search_range, target_hop + search_range]
        let start = target_hop.saturating_sub(self.search_range);
        let end = (target_hop + self.search_range).min(self.input_buffer.len().saturating_sub(self.frame_size + input_pos).saturating_sub(1));
        
        if start >= end {
            return target_hop;
        }

        let mut best_offset = target_hop;
        let mut max_corr = -1.0;
        
        // Use a subset of samples for correlation to save CPU
        let compare_len = self.search_range;
        
        // "Natural" continuation would be at input_pos + hop_size (synthesis hop)
        // We want to link that to input_pos + analysis_hop (target_hop)
        // So we compare input[input_pos + hop_size ...] with input[input_pos + k ...]
        
        let ref_pos = input_pos + self.hop_size;
        if ref_pos + compare_len > self.input_buffer.len() {
             return target_hop;
        }
        
        let ref_segment = &self.input_buffer[ref_pos..ref_pos + compare_len];

        for k in start..end {
            let candidate_pos = input_pos + k;
             if candidate_pos + compare_len > self.input_buffer.len() {
                continue;
            }
            
            let candidate = &self.input_buffer[candidate_pos..candidate_pos + compare_len];
            
            // Cross-correlation
            let mut corr = 0.0;
            for i in 0..compare_len {
                corr += ref_segment[i] * candidate[i];
            }
            
            if corr > max_corr {
                max_corr = corr;
                best_offset = k;
            }
        }
        
        best_offset
    }

    /// Time-stretch the input samples.
    /// speed_ratio > 1.0 = faster (compress time), < 1.0 = slower (expand time)
    fn stretch(&mut self, input: &[i16], speed_ratio: f64) -> Vec<i16> {
        // Bypass for normal speed
        if (speed_ratio - 1.0).abs() < 0.05 || input.is_empty() {
            // Flush any remaining overlap buffer
            if !self.output_overlap.is_empty() {
                let result: Vec<i16> = self.output_overlap.drain(..)
                    .map(|s| s.clamp(-32768.0, 32767.0) as i16)
                    .collect();
                // Also return the input
                let mut combined = result;
                combined.extend(input.iter().cloned());
                return combined;
            }
            return input.to_vec();
        }
        
        // Clear buffers if speed changed significantly (avoid artifacts)
        if (speed_ratio - self.last_speed).abs() > 0.15 {
            self.input_buffer.clear();
            self.output_overlap.clear();
        }
        self.last_speed = speed_ratio;
        
        // Add input samples to buffer (convert to f32)
        self.input_buffer.extend(input.iter().map(|&s| s as f32));
        
        // Need at least one frame + search range to process
        if self.input_buffer.len() < self.frame_size + self.search_range {
            return Vec::new();
        }
        
        // Ideal analysis hop
        let target_analysis_hop = (self.hop_size as f64 * speed_ratio).round() as usize;
        
        // Synthesis hop stays constant at 50% of frame size
        let synthesis_hop = self.hop_size;
        
        // Output buffer
        // We guess size based on target ratio, but it will vary slightly due to SOLA
        let estimated_frames = self.input_buffer.len() / target_analysis_hop.max(1);
        let mut output = vec![0.0f32; estimated_frames * synthesis_hop + self.frame_size];
        
        // Initialize output with overlap from previous call
        for (i, &v) in self.output_overlap.iter().enumerate() {
            if i < output.len() {
                output[i] = v;
            }
        }
        
        let mut input_pos = 0usize;
        let mut output_pos = 0usize;
        
        loop {
            // Ensure we have enough input for:
            // 1. Comparison (at current pos + hop_size)
            // 2. Next frame (at current pos + target_hop + search_range)
            if input_pos + self.frame_size + self.search_range + target_analysis_hop > self.input_buffer.len() {
                break;
            }
            if output_pos + self.frame_size > output.len() {
                output.resize(output_pos + self.frame_size * 2, 0.0);
            }
            
            // Find best alignment offset (SOLA)
            let actual_analysis_hop = self.find_best_offset(input_pos, target_analysis_hop);
            
            // Advance input by the OPTIMIZED hop
            input_pos += actual_analysis_hop;
            
            // Apply window and overlap-add
            for i in 0..self.frame_size {
                let in_sample = self.input_buffer[input_pos + i];
                let w = self.window[i];
                output[output_pos + i] += in_sample * w;
            }
            
            output_pos += synthesis_hop;
        }
        
        // The "complete" output is everything up to the start of the last frame's tail
        let complete_len = output_pos.min(output.len());
        
        // Save the tail for next call's overlap
        self.output_overlap.clear();
        if complete_len < output.len() {
            self.output_overlap.extend_from_slice(&output[complete_len..]);
        }
        
        // Remove consumed input
        // Maintain overlap context: input_pos points to start of LAST processed frame
        // We need to keep that frame because next search compares against it
        let consumed = input_pos.min(self.input_buffer.len());
        
        if consumed > 0 {
            self.input_buffer.drain(0..consumed);
        }
        
        // Return the complete portion as i16
        output[..complete_len].iter()
            .map(|&s| s.clamp(-32768.0, 32767.0) as i16)
            .collect()
    }
}

