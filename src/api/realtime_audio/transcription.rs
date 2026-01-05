//! Main transcription loop for realtime audio

use anyhow::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Preset;
use crate::overlay::realtime_webview::SELECTED_APP_PID;
use crate::APP;

use super::capture::{start_device_loopback_capture, start_mic_capture, start_per_app_capture};
use super::state::SharedRealtimeState;
use super::translation::run_translation_loop;
use super::utils::update_overlay_text;
use super::websocket::{
    connect_websocket, parse_input_transcription, send_audio_chunk, send_setup_message,
    set_socket_nonblocking,
};
use super::WM_VOLUME_UPDATE;

/// Audio mode state machine for silence injection
#[derive(Clone, Copy, PartialEq)]
enum AudioMode {
    Normal,
    Silence,
    CatchUp,
}

/// Start realtime audio transcription
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

    use crate::overlay::realtime_webview::{
        AUDIO_SOURCE_CHANGE, NEW_AUDIO_SOURCE, NEW_TRANSCRIPTION_MODEL, TRANSCRIPTION_MODEL_CHANGE,
    };

    let mut current_preset = preset;

    loop {
        AUDIO_SOURCE_CHANGE.store(false, Ordering::SeqCst);
        TRANSCRIPTION_MODEL_CHANGE.store(false, Ordering::SeqCst);

        let trans_model = {
            let app = APP.lock().unwrap();
            app.config.realtime_transcription_model.clone()
        };

        let result = if trans_model == "parakeet" {
            super::parakeet::run_parakeet_transcription(
                current_preset.clone(),
                stop_signal.clone(),
                hwnd_overlay,
                state.clone(),
            )
        } else {
            run_realtime_transcription(
                current_preset.clone(),
                stop_signal.clone(),
                hwnd_overlay,
                hwnd_translation,
                state.clone(),
            )
        };

        if let Err(e) = result {
            if !AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst) {
                let err_msg = format!(" [Error: {}]", e);
                eprintln!("Realtime transcription error: {}", e);

                // Append error to state so it's visible in the window
                if let Ok(mut s) = state.lock() {
                    s.append_transcript(&err_msg);
                }

                // Force immediate UI update
                let display_text = if let Ok(s) = state.lock() {
                    s.display_transcript.clone()
                } else {
                    String::new()
                };
                use super::utils::update_overlay_text;
                update_overlay_text(hwnd_overlay, &display_text);

                // Do NOT close the window - let the user see the error
            }
        }

        let restart_source = AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst);
        let restart_model = TRANSCRIPTION_MODEL_CHANGE.load(Ordering::SeqCst);

        if restart_source {
            if let Ok(new_source) = NEW_AUDIO_SOURCE.lock() {
                if !new_source.is_empty() {
                    println!("Changing audio source to: {}", new_source);
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_audio_source = new_source.clone();
                    // Save config? Optional, but UI should sync.
                }
            }
        }

        if restart_model {
            if let Ok(new_model) = NEW_TRANSCRIPTION_MODEL.lock() {
                if !new_model.is_empty() {
                    println!("Changing transcription model to: {}", new_model);
                    let mut app = APP.lock().unwrap();
                    app.config.realtime_transcription_model = new_model.clone();
                }
            }
        }

        if !restart_source && !restart_model && stop_signal.load(Ordering::Relaxed) {
            break;
        }
        // If a restart is triggered (source or model changed), the loop continues.
        // Otherwise, if stop_signal is set, we break.
        // If neither, we also break, meaning the transcription loop only runs once
        // unless a restart is explicitly requested.
        if !restart_source && !restart_model {
            break;
        }
    }
}

fn run_realtime_transcription(
    preset: Preset,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    translation_hwnd: Option<HWND>,
    state: SharedRealtimeState,
) -> Result<()> {
    let gemini_api_key = {
        let app = APP.lock().unwrap();
        app.config.gemini_api_key.clone()
    };

    if gemini_api_key.trim().is_empty() {
        return Err(anyhow::anyhow!("NO_API_KEY:google"));
    }

    let mut socket = connect_websocket(&gemini_api_key)?;
    send_setup_message(&mut socket)?;

    // Wait for setup acknowledgment
    let setup_start = Instant::now();
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                let msg = msg.as_str();
                if msg.contains("setupComplete") {
                    break;
                }
                if msg.contains("error") || msg.contains("Error") {
                    return Err(anyhow::anyhow!("Server returned error: {}", msg));
                }
            }
            Ok(tungstenite::Message::Close(frame)) => {
                let close_info = frame
                    .map(|f| format!("code={}, reason={}", f.code, f.reason))
                    .unwrap_or("no frame".to_string());
                return Err(anyhow::anyhow!(
                    "Connection closed by server: {}",
                    close_info
                ));
            }
            Ok(tungstenite::Message::Binary(data)) => {
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if text.contains("setupComplete") {
                        break;
                    }
                } else if data.len() < 100 {
                    break;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
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

    set_socket_nonblocking(&mut socket)?;

    let audio_buffer: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));

    use crate::overlay::realtime_webview::REALTIME_TTS_ENABLED;
    let tts_enabled = REALTIME_TTS_ENABLED.load(Ordering::SeqCst);
    let selected_pid = SELECTED_APP_PID.load(Ordering::SeqCst);

    let using_per_app_capture = preset.audio_source == "device" && tts_enabled && selected_pid > 0;
    let using_device_loopback = preset.audio_source == "device" && !tts_enabled;

    let _stream: Option<cpal::Stream>;

    if using_per_app_capture {
        #[cfg(target_os = "windows")]
        {
            start_per_app_capture(selected_pid, audio_buffer.clone(), stop_signal.clone())?;
        }
        _stream = None;
    } else if using_device_loopback {
        _stream = Some(start_device_loopback_capture(
            audio_buffer.clone(),
            stop_signal.clone(),
        )?);
    } else if preset.audio_source == "device" && tts_enabled && selected_pid == 0 {
        _stream = None;
    } else {
        _stream = Some(start_mic_capture(
            audio_buffer.clone(),
            stop_signal.clone(),
        )?);
    }

    // Start translation thread if needed
    let has_translation = translation_hwnd.is_some() && preset.blocks.len() > 1;
    if has_translation {
        let translation_send = crate::win_types::SendHwnd(translation_hwnd.unwrap());
        let translation_state = state.clone();
        let translation_stop = stop_signal.clone();
        let translation_preset = preset.clone();
        std::thread::spawn(move || {
            run_translation_loop(
                translation_preset,
                translation_stop,
                translation_send,
                translation_state,
            );
        });
    }

    // Main loop
    run_main_loop(
        socket,
        audio_buffer,
        stop_signal,
        overlay_hwnd,
        state,
        &gemini_api_key,
    )?;

    drop(_stream);
    Ok(())
}

fn run_main_loop(
    mut socket: tungstenite::WebSocket<native_tls::TlsStream<std::net::TcpStream>>,
    audio_buffer: Arc<Mutex<Vec<i16>>>,
    stop_signal: Arc<AtomicBool>,
    overlay_hwnd: HWND,
    state: SharedRealtimeState,
    gemini_api_key: &str,
) -> Result<()> {
    let mut last_send = Instant::now();
    let send_interval = Duration::from_millis(100);

    let mut audio_mode = AudioMode::Normal;
    let mut mode_start = Instant::now();
    let mut silence_buffer: Vec<i16> = Vec::new();

    const NORMAL_DURATION: Duration = Duration::from_secs(20);
    const SILENCE_DURATION: Duration = Duration::from_secs(2);
    const SAMPLES_PER_100MS: usize = 1600;

    let mut last_transcription_time = Instant::now();
    let mut consecutive_empty_reads: u32 = 0;
    const NO_RESULT_THRESHOLD_SECS: u64 = 8;
    const EMPTY_READ_CHECK_COUNT: u32 = 50;

    while !stop_signal.load(Ordering::Relaxed) {
        if overlay_hwnd.0 != 0 as _ && !unsafe { IsWindow(Some(overlay_hwnd)).as_bool() } {
            stop_signal.store(true, Ordering::SeqCst);
            break;
        }

        {
            use crate::overlay::realtime_webview::AUDIO_SOURCE_CHANGE;
            if AUDIO_SOURCE_CHANGE.load(Ordering::SeqCst) {
                break;
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
                if silence_buffer.is_empty() {
                    audio_mode = AudioMode::Normal;
                    mode_start = Instant::now();
                }
            }
        }

        // Send audio
        if last_send.elapsed() >= send_interval {
            let real_audio: Vec<i16> = {
                let mut buf = audio_buffer.lock().unwrap();
                std::mem::take(&mut *buf)
            };

            match audio_mode {
                AudioMode::Normal => {
                    if !real_audio.is_empty() {
                        if send_audio_chunk(&mut socket, &real_audio).is_err() {
                            break;
                        }
                    }
                }
                AudioMode::Silence => {
                    silence_buffer.extend(real_audio);
                    let silence: Vec<i16> = vec![0i16; SAMPLES_PER_100MS];
                    if send_audio_chunk(&mut socket, &silence).is_err() {
                        break;
                    }
                }
                AudioMode::CatchUp => {
                    silence_buffer.extend(real_audio);
                    let chunk_size = SAMPLES_PER_100MS * 2;
                    let to_send: Vec<i16> = if silence_buffer.len() >= chunk_size {
                        silence_buffer.drain(..chunk_size).collect()
                    } else if !silence_buffer.is_empty() {
                        silence_buffer.drain(..).collect()
                    } else {
                        Vec::new()
                    };
                    if !to_send.is_empty() {
                        if send_audio_chunk(&mut socket, &to_send).is_err() {
                            break;
                        }
                    }
                }
            }
            last_send = Instant::now();
            unsafe {
                let _ = PostMessageW(Some(overlay_hwnd), WM_VOLUME_UPDATE, WPARAM(0), LPARAM(0));
            }
        }

        // Receive transcriptions
        match socket.read() {
            Ok(tungstenite::Message::Text(msg)) => {
                let msg = msg.as_str();
                if let Some(transcript) = parse_input_transcription(msg) {
                    if !transcript.is_empty() {
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
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if let Some(transcript) = parse_input_transcription(&text) {
                        if !transcript.is_empty() {
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
            }
            Ok(tungstenite::Message::Close(_)) => {
                if !try_reconnect(
                    &mut socket,
                    gemini_api_key,
                    &audio_buffer,
                    &mut silence_buffer,
                    &mut audio_mode,
                    &mut mode_start,
                    &mut last_transcription_time,
                    &mut consecutive_empty_reads,
                ) {
                    break;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                consecutive_empty_reads += 1;
                if consecutive_empty_reads >= EMPTY_READ_CHECK_COUNT
                    && last_transcription_time.elapsed()
                        > Duration::from_secs(NO_RESULT_THRESHOLD_SECS)
                {
                    if !try_reconnect(
                        &mut socket,
                        gemini_api_key,
                        &audio_buffer,
                        &mut silence_buffer,
                        &mut audio_mode,
                        &mut mode_start,
                        &mut last_transcription_time,
                        &mut consecutive_empty_reads,
                    ) {
                        break;
                    }
                }
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("reset")
                    || error_str.contains("closed")
                    || error_str.contains("broken")
                {
                    if !try_reconnect(
                        &mut socket,
                        gemini_api_key,
                        &audio_buffer,
                        &mut silence_buffer,
                        &mut audio_mode,
                        &mut mode_start,
                        &mut last_transcription_time,
                        &mut consecutive_empty_reads,
                    ) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    let _ = socket.close(None);
    Ok(())
}

fn try_reconnect(
    socket: &mut tungstenite::WebSocket<native_tls::TlsStream<std::net::TcpStream>>,
    api_key: &str,
    audio_buffer: &Arc<Mutex<Vec<i16>>>,
    silence_buffer: &mut Vec<i16>,
    audio_mode: &mut AudioMode,
    mode_start: &mut Instant,
    last_transcription_time: &mut Instant,
    consecutive_empty_reads: &mut u32,
) -> bool {
    let mut reconnect_buffer: Vec<i16> = Vec::new();
    let _ = socket.close(None);

    for _attempt in 1..=3 {
        {
            let mut buf = audio_buffer.lock().unwrap();
            reconnect_buffer.extend(std::mem::take(&mut *buf));
        }

        match connect_websocket(api_key) {
            Ok(mut new_socket) => {
                if send_setup_message(&mut new_socket).is_err() {
                    continue;
                }
                if set_socket_nonblocking(&mut new_socket).is_err() {
                    continue;
                }
                {
                    let mut buf = audio_buffer.lock().unwrap();
                    reconnect_buffer.extend(std::mem::take(&mut *buf));
                }
                silence_buffer.clear();
                silence_buffer.extend(reconnect_buffer);
                *audio_mode = AudioMode::CatchUp;
                *mode_start = Instant::now();
                *socket = new_socket;
                *last_transcription_time = Instant::now();
                *consecutive_empty_reads = 0;
                return true;
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
    false
}
