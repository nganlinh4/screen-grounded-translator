use minimp3::{Decoder, Frame};
use std::io::{Cursor, Read};
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};
use tungstenite::{client, Message};

use super::manager::TtsManager;
use super::types::AudioEvent;
use super::utils::{clear_tts_loading_state, clear_tts_state, get_language_instruction_for_text};
use super::websocket::{
    connect_tts_websocket, is_turn_complete, parse_audio_data, send_tts_setup, send_tts_text,
};
use crate::api::client::UREQ_AGENT;

use crate::APP;
use isolang::Language;

/// Socket Worker thread - fetches audio data and pipes it to the player
pub fn run_socket_worker(manager: Arc<TtsManager>) {
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

        // Check if this request is stale
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            continue;
        }

        // Check TTS Method - route to alternative handlers if not Gemini
        let tts_method = {
            match APP.lock() {
                Ok(app) => app.config.tts_method.clone(),
                Err(_) => {
                    let _ = tx.send(AudioEvent::End);
                    continue;
                }
            }
        };

        if tts_method == crate::config::TtsMethod::GoogleTranslate {
            handle_google_tts(manager.clone(), request, tx);
            continue;
        }

        if tts_method == crate::config::TtsMethod::EdgeTTS {
            handle_edge_tts(manager.clone(), request, tx);
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
            eprintln!("TTS: No Gemini API key configured");
            let _ = tx.send(AudioEvent::End);
            clear_tts_loading_state(request.req.hwnd);
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
                clear_tts_loading_state(request.req.hwnd);
                clear_tts_state(request.req.hwnd);
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
        };

        // Read config for setup
        let (current_voice, current_speed, language_instruction) = {
            let app = APP.lock().unwrap();
            let voice = app.config.tts_voice.clone();
            let conditions = app.config.tts_language_conditions.clone();

            let instruction = get_language_instruction_for_text(&request.req.text, &conditions);

            if request.req.is_realtime {
                (voice, "Normal".to_string(), instruction)
            } else {
                (voice, app.config.tts_speed.clone(), instruction)
            }
        };

        // Send setup
        if let Err(e) = send_tts_setup(
            &mut socket,
            &current_voice,
            &current_speed,
            language_instruction.as_deref(),
        ) {
            eprintln!("TTS: Failed to send setup: {}", e);
            let _ = socket.close(None);
            let _ = tx.send(AudioEvent::End);
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        // Wait for setup acknowledgment
        let setup_start = Instant::now();
        let mut setup_complete = false;
        loop {
            if request.generation < manager.interrupt_generation.load(Ordering::SeqCst)
                || manager.shutdown.load(Ordering::SeqCst)
            {
                let _ = socket.close(None);
                let _ = tx.send(AudioEvent::End);
                break;
            }

            match socket.read() {
                Ok(Message::Text(msg)) => {
                    if msg.contains("setupComplete") {
                        setup_complete = true;
                        break;
                    }
                    if msg.contains("error") || msg.contains("Error") {
                        eprintln!("TTS: Setup error: {}", msg);
                        break;
                    }
                }
                Ok(Message::Close(_)) => {
                    break;
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data) {
                        if text.contains("setupComplete") {
                            setup_complete = true;
                            break;
                        }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    if setup_start.elapsed() > Duration::from_secs(10) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    break;
                }
            }
        }

        if manager.shutdown.load(Ordering::SeqCst) {
            return;
        }

        if !setup_complete {
            let _ = socket.close(None);
            let _ = tx.send(AudioEvent::End);
            continue;
        }

        // Send request text
        if let Err(e) = send_tts_text(&mut socket, &request.req.text) {
            eprintln!("TTS: Failed to send text: {}", e);
            let _ = tx.send(AudioEvent::End);
            let _ = socket.close(None);
            continue;
        }

        // Read loop
        loop {
            if request.generation < manager.interrupt_generation.load(Ordering::SeqCst)
                || manager.shutdown.load(Ordering::SeqCst)
            {
                let _ = socket.close(None);
                let _ = tx.send(AudioEvent::End);
                break;
            }

            match socket.read() {
                Ok(Message::Text(msg)) => {
                    if let Some(audio_data) = parse_audio_data(&msg) {
                        let _ = tx.send(AudioEvent::Data(audio_data));
                    }
                    if is_turn_complete(&msg) {
                        let _ = tx.send(AudioEvent::End);
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
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
                Ok(Message::Close(_)) => {
                    let _ = tx.send(AudioEvent::End);
                    break;
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => {
                    eprintln!("TTS: Read error: {}", e);
                    let _ = tx.send(AudioEvent::End);
                    break;
                }
            }
        }

        let _ = socket.close(None);
    }
}

/// Google Translate TTS integrated with the existing audio pipeline
/// Downloads MP3, decodes to PCM, sends via AudioEvent channel for WSOLA speed control
fn handle_google_tts(
    manager: Arc<TtsManager>,
    request: super::types::QueuedRequest,
    tx: std::sync::mpsc::Sender<AudioEvent>,
) {
    let text = request.req.text.clone();

    // Detect language for Google TTS TL parameter
    let lang_code = whatlang::detect_lang(&text).unwrap_or(whatlang::Lang::Eng);

    // Convert whatlang Lang to ISO 639-1 (best effort)
    // Convert whatlang Lang to ISO 639-1 via isolang for Google TTS
    let tl = Language::from_639_3(lang_code.code())
        .and_then(|l| l.to_639_1())
        .unwrap_or("en");

    // Google TTS URL
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl={}&client=tw-ob",
        urlencoding::encode(&text),
        tl
    );

    // Download audio (blocking)
    let resp = match UREQ_AGENT.get(&url).call() {
        Ok(r) => r,
        Err(_) => {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }
    };

    let mut mp3_data = Vec::new();
    if resp.into_reader().read_to_end(&mut mp3_data).is_err() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    // Decode MP3 to PCM
    let mut decoder = Decoder::new(Cursor::new(mp3_data));
    let mut source_sample_rate = 24000u32;
    let mut all_samples: Vec<i16> = Vec::new();

    loop {
        // Check interrupt
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }

        match decoder.next_frame() {
            Ok(Frame {
                data,
                sample_rate,
                channels,
                ..
            }) => {
                source_sample_rate = sample_rate as u32;
                // If stereo, mix to mono
                if channels == 2 {
                    for chunk in data.chunks(2) {
                        let sample = ((chunk[0] as i32 + chunk[1] as i32) / 2) as i16;
                        all_samples.push(sample);
                    }
                } else {
                    all_samples.extend_from_slice(&data);
                }
            }
            Err(minimp3::Error::Eof) => break,
            Err(_) => break,
        }
    }

    if all_samples.is_empty() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    // Clear loading state as soon as we have audio
    clear_tts_loading_state(request.req.hwnd);

    // Resample if needed to 24kHz (Gemini standard for our pipeline)
    let audio_bytes = if source_sample_rate != 24000 {
        let resampled = resample_audio(&all_samples, source_sample_rate, 24000);
        let mut bytes = Vec::with_capacity(resampled.len() * 2);
        for sample in resampled {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    } else {
        let mut bytes = Vec::with_capacity(all_samples.len() * 2);
        for sample in all_samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    };

    // Send in chunks
    let chunk_size = 24000;
    for chunk in audio_bytes.chunks(chunk_size) {
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            break;
        }
        let _ = tx.send(AudioEvent::Data(chunk.to_vec()));
    }

    let _ = tx.send(AudioEvent::End);
    clear_tts_state(request.req.hwnd);
}

fn handle_edge_tts(
    manager: Arc<TtsManager>,
    request: super::types::QueuedRequest,
    tx: std::sync::mpsc::Sender<AudioEvent>,
) {
    let text = request.req.text.clone();
    let generation = request.generation;
    let manager_clone = manager.clone();

    // Get Settings
    let (voice_name, pitch, rate) = {
        let app = APP.lock().unwrap();
        let settings = &app.config.edge_tts_settings;

        let lang_detect = whatlang::detect(&text);

        let mut voice = "en-US-AriaNeural".to_string();

        // Convert detected language to ISO 639-1 (2-letter) code for config lookup
        let code_2 = lang_detect
            .and_then(|info| Language::from_639_3(info.lang().code()))
            .and_then(|l| l.to_639_1())
            .unwrap_or("en");

        for config in &settings.voice_configs {
            if config.language_code == code_2 {
                voice = config.voice_name.clone();
                break;
            }
        }

        (voice, settings.pitch, settings.rate)
    };

    // Edge TTS WebSocket constants
    let trusted_token = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
    let connection_id = format!(
        "{:032x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let wss_url = format!(
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1?TrustedClientToken={}&ConnectionId={}",
        trusted_token, connection_id
    );

    let connector = match native_tls::TlsConnector::new() {
        Ok(c) => c,
        Err(_) => {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }
    };

    let host = "speech.platform.bing.com";
    let stream = match std::net::TcpStream::connect(format!("{}:443", host)) {
        Ok(s) => s,
        Err(_) => {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }
    };

    let tls_stream = match connector.connect(host, stream) {
        Ok(s) => s,
        Err(_) => {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }
    };

    let (mut socket, _) = match client(&wss_url, tls_stream) {
        Ok(s) => s,
        Err(_) => {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }
    };

    let request_id = format!(
        "{:032x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );

    let config_msg = format!(
        "X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}",
        chrono::Utc::now().format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)")
    );

    if socket.send(Message::Text(config_msg)).is_err() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    let pitch_str = if pitch >= 0 {
        format!("+{}Hz", pitch)
    } else {
        format!("{}Hz", pitch)
    };
    let rate_str = if rate >= 0 {
        format!("+{}%", rate)
    } else {
        format!("{}%", rate)
    };

    let escaped_text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");

    let ssml = format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'>\
        <voice name='{}'>\
        <prosody pitch='{}' rate='{}' volume='+0%'>{}</prosody>\
        </voice></speak>",
        voice_name, pitch_str, rate_str, escaped_text
    );

    let ssml_msg = format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}Z\r\nPath:ssml\r\n\r\n{}",
        request_id,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
        ssml
    );

    if socket.send(Message::Text(ssml_msg)).is_err() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    clear_tts_loading_state(request.req.hwnd);

    let mut mp3_data: Vec<u8> = Vec::new();

    loop {
        if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
            break;
        }

        match socket.read() {
            Ok(Message::Binary(data)) => {
                if data.len() >= 2 {
                    let header_len = u16::from_be_bytes([data[0], data[1]]) as usize;
                    let audio_start = 2 + header_len;
                    if data.len() > audio_start {
                        let header = &data[2..audio_start];
                        if header.windows(11).any(|w| w == b"Path:audio\r") {
                            mp3_data.extend_from_slice(&data[audio_start..]);
                        }
                    }
                }
            }
            Ok(Message::Text(text)) => {
                if text.contains("Path:turn.end") {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
            _ => {}
        }
    }

    let _ = socket.close(None);

    if mp3_data.is_empty() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    let mut decoder = Decoder::new(Cursor::new(mp3_data));
    let mut all_samples: Vec<i16> = Vec::new();
    let mut source_sample_rate = 24000u32;

    loop {
        if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            clear_tts_state(request.req.hwnd);
            return;
        }
        match decoder.next_frame() {
            Ok(Frame {
                data,
                sample_rate,
                channels,
                ..
            }) => {
                source_sample_rate = sample_rate as u32;
                if channels == 2 {
                    for chunk in data.chunks(2) {
                        let sample = ((chunk[0] as i32 + chunk[1] as i32) / 2) as i16;
                        all_samples.push(sample);
                    }
                } else {
                    all_samples.extend_from_slice(&data);
                }
            }
            Err(minimp3::Error::Eof) => break,
            Err(_) => break,
        }
    }

    let audio_bytes = if source_sample_rate != 24000 {
        let resampled = resample_audio(&all_samples, source_sample_rate, 24000);
        let mut bytes = Vec::with_capacity(resampled.len() * 2);
        for sample in resampled {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    } else {
        let mut bytes = Vec::with_capacity(all_samples.len() * 2);
        for sample in all_samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    };

    let chunk_size = 24000;
    for chunk in audio_bytes.chunks(chunk_size) {
        if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
            break;
        }
        let _ = tx.send(AudioEvent::Data(chunk.to_vec()));
    }

    let _ = tx.send(AudioEvent::End);
    clear_tts_state(request.req.hwnd);
}

fn resample_audio(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = to_rate as f32 / from_rate as f32;
    let new_len = (samples.len() as f32 * ratio) as usize;
    let mut result = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx_f = i as f32 / ratio;
        let src_idx = src_idx_f as usize;

        if src_idx >= samples.len() - 1 {
            result.push(samples[src_idx.min(samples.len() - 1)]);
        } else {
            let t = src_idx_f - src_idx as f32;
            let s1 = samples[src_idx] as f32;
            let s2 = samples[src_idx + 1] as f32;
            let val = s1 + t * (s2 - s1);
            result.push(val as i16);
        }
    }

    result
}
