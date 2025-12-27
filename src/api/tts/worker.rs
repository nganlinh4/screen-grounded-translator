use std::io::Read;
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};
use tungstenite::Message;

use super::manager::TtsManager;
use super::types::AudioEvent;
use super::utils::{clear_tts_loading_state, clear_tts_state, get_language_instruction_for_text};
use super::websocket::{
    connect_tts_websocket, is_turn_complete, parse_audio_data, send_tts_setup, send_tts_text,
};
use crate::api::client::UREQ_AGENT;
use crate::APP;
use urlencoding;
use whatlang;

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

        // Check TTS Method
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

        if api_key.is_empty() {
            let _ = tx.send(AudioEvent::End);
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }

        // Check if request is still valid (not interrupted)
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            continue;
        }

        // Handle with Gemini Live
        handle_gemini_tts(&manager, &request, &tx, &api_key);
    }
}

fn handle_gemini_tts(
    manager: &Arc<TtsManager>,
    request: &super::types::QueuedRequest,
    tx: &std::sync::mpsc::Sender<AudioEvent>,
    api_key: &str,
) {
    // Connect to the TTS WebSocket
    let mut socket = match connect_tts_websocket(api_key) {
        Ok(s) => s,
        Err(_e) => {
            let _ = tx.send(AudioEvent::End);
            return;
        }
    };

    // Get voice and language instruction
    let (voice, language_instruction) = {
        match APP.lock() {
            Ok(app) => {
                let v = app.config.tts_voice.clone();
                let text = &request.req.text;
                let li =
                    get_language_instruction_for_text(text, &app.config.tts_language_conditions);
                (v, li)
            }
            Err(_) => {
                let _ = tx.send(AudioEvent::End);
                return;
            }
        }
    };

    // Get speed from config
    let speed = {
        match APP.lock() {
            Ok(app) => app.config.tts_speed.clone(),
            Err(_) => "Normal".to_string(),
        }
    };

    // Send setup message
    if send_tts_setup(&mut socket, &voice, &speed, language_instruction.as_deref()).is_err() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    // Send the text
    if send_tts_text(&mut socket, &request.req.text).is_err() {
        let _ = tx.send(AudioEvent::End);
        clear_tts_state(request.req.hwnd);
        return;
    }

    clear_tts_loading_state(request.req.hwnd);

    // Receive audio data
    let read_timeout = Duration::from_secs(30);
    let mut last_data = Instant::now();
    let mut got_any_data = false;

    loop {
        // Check for interrupt
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            break;
        }

        // Check timeout
        if last_data.elapsed() > read_timeout {
            break;
        }

        // Read message
        match socket.read() {
            Ok(Message::Text(text)) => {
                last_data = Instant::now();

                // Parse audio data
                if let Some(audio_bytes) = parse_audio_data(&text) {
                    if !audio_bytes.is_empty() {
                        got_any_data = true;
                        if tx.send(AudioEvent::Data(audio_bytes)).is_err() {
                            break;
                        }
                    }
                }

                // Check for turn complete
                if is_turn_complete(&text) {
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => {
                break;
            }
            _ => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    let _ = tx.send(AudioEvent::End);

    if !got_any_data {
        clear_tts_state(request.req.hwnd);
    } else {
        // Clear state happens after playback finishes (in player thread)
        // For now, just close the socket
        if request.generation < manager.interrupt_generation.load(Ordering::SeqCst) {
            clear_tts_state(request.req.hwnd);
        }
    }

    let _ = socket.close(None);
}

/// Google Translate TTS integrated with the existing audio pipeline
/// Downloads MP3, decodes to PCM, sends via AudioEvent channel for WSOLA speed control
fn handle_google_tts(
    manager: Arc<TtsManager>,
    request: super::types::QueuedRequest,
    tx: std::sync::mpsc::Sender<AudioEvent>,
) {
    let text = request.req.text.clone();
    let is_realtime = request.req.is_realtime;

    // Detect language for Google TTS TL parameter
    let lang_code = whatlang::detect_lang(&text).unwrap_or(whatlang::Lang::Eng);

    // Convert whatlang Lang to ISO 639-1 (best effort)
    let tl = match lang_code {
        whatlang::Lang::Vie => "vi",
        whatlang::Lang::Kor => "ko",
        whatlang::Lang::Jpn => "ja",
        whatlang::Lang::Cmn => "zh",
        whatlang::Lang::Fra => "fr",
        whatlang::Lang::Deu => "de",
        whatlang::Lang::Spa => "es",
        whatlang::Lang::Rus => "ru",
        whatlang::Lang::Ita => "it",
        _ => "en",
    };

    // Get API speed from config (for non-realtime calls)
    // For realtime calls, speed manipulation happens in the player via REALTIME_TTS_SPEED
    let api_speed = if is_realtime {
        1.0 // Let the player handle speed for realtime
    } else {
        match APP.lock() {
            Ok(app) => match app.config.tts_speed.as_str() {
                "Slow" => 0.3,
                _ => 1.0,
            },
            Err(_) => 1.0,
        }
    };

    let encoded = urlencoding::encode(&text);
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl={}&client=tw-ob&ttsspeed={}",
        encoded, tl, api_speed
    );

    // Clone manager for interrupt checking
    let manager_clone = manager.clone();
    let generation = request.generation;

    std::thread::spawn(move || {
        use minimp3::{Decoder, Frame};
        use std::io::Cursor;

        // Check for interrupt before starting
        if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            return;
        }

        // Download MP3 data with retry mechanism
        let mut mp3_data: Option<Vec<u8>> = None;
        let retry_delays = [200, 500, 1000]; // ms

        for (attempt, delay_ms) in retry_delays.iter().enumerate() {
            // Check for interrupt before each attempt
            if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
                let _ = tx.send(AudioEvent::End);
                return;
            }

            match UREQ_AGENT
                .get(&url)
                .set(
                    "User-Agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                )
                .call()
            {
                Ok(response) => {
                    let mut data = Vec::new();
                    if response.into_reader().read_to_end(&mut data).is_ok() && !data.is_empty() {
                        mp3_data = Some(data);
                        break;
                    }
                }
                Err(_) => {
                    // Log retry attempt (optional: could add logging here)
                }
            }

            // Wait before retry (except on last attempt)
            if attempt < retry_delays.len() - 1 {
                std::thread::sleep(Duration::from_millis(*delay_ms as u64));
            }
        }

        let mp3_data = match mp3_data {
            Some(data) => data,
            None => {
                let _ = tx.send(AudioEvent::End);
                return;
            }
        };

        // Check for interrupt after download
        if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            return;
        }

        // Decode MP3 to PCM
        let mut decoder = Decoder::new(Cursor::new(mp3_data));
        let mut all_samples: Vec<i16> = Vec::new();
        let mut source_sample_rate = 24000u32;

        loop {
            match decoder.next_frame() {
                Ok(Frame {
                    data,
                    sample_rate,
                    channels,
                    ..
                }) => {
                    source_sample_rate = sample_rate as u32;

                    // Convert stereo to mono if needed
                    let mono_samples: Vec<i16> = if channels == 2 {
                        data.chunks(2)
                            .map(|chunk| ((chunk[0] as i32 + chunk[1] as i32) / 2) as i16)
                            .collect()
                    } else {
                        data
                    };

                    all_samples.extend(mono_samples);
                }
                Err(minimp3::Error::Eof) => break,
                Err(_) => break,
            }
        }

        if all_samples.is_empty() {
            let _ = tx.send(AudioEvent::End);
            return;
        }

        // Resample to 24kHz if needed (Gemini uses 24kHz)
        let target_rate = 24000u32;
        let resampled = if source_sample_rate != target_rate {
            resample_linear(&all_samples, source_sample_rate, target_rate)
        } else {
            all_samples
        };

        // Convert i16 samples to bytes (little-endian, like Gemini sends)
        let audio_bytes: Vec<u8> = resampled.iter().flat_map(|&s| s.to_le_bytes()).collect();

        // Check for interrupt before sending
        if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
            let _ = tx.send(AudioEvent::End);
            return;
        }

        // Send audio data in chunks (like Gemini streaming)
        // This allows the player to start playing while we're "streaming"
        let chunk_size = 4800; // 100ms at 24kHz mono 16-bit = 4800 bytes
        for chunk in audio_bytes.chunks(chunk_size) {
            if generation < manager_clone.interrupt_generation.load(Ordering::SeqCst) {
                break;
            }
            if tx.send(AudioEvent::Data(chunk.to_vec())).is_err() {
                break;
            }
            // Small delay to simulate streaming (prevents buffer overload)
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = tx.send(AudioEvent::End);
    });
}

/// Simple linear resampling (good enough for TTS)
fn resample_linear(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = src_pos - src_idx as f64;

        let s1 = samples.get(src_idx).copied().unwrap_or(0);
        let s2 = samples.get(src_idx + 1).copied().unwrap_or(s1);

        let interpolated = s1 as f64 * (1.0 - frac) + s2 as f64 * frac;
        output.push(interpolated as i16);
    }

    output
}
