use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use std::time::Duration;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

use super::manager::TtsManager;
use super::types::*;
use super::utils::{clear_tts_loading_state, clear_tts_state};
use super::wsola::WsolaStretcher;

/// Main Player thread - consumes audio streams sequentially
pub fn run_player_thread(manager: Arc<TtsManager>) {
    // Create ONE persistent audio player
    // This avoids the overhead of opening the audio device for every request
    // We pass manager to AudioPlayer so it can check interrupts
    let audio_player = AudioPlayer::new(PLAYBACK_SAMPLE_RATE, manager.clone());

    loop {
        if manager.shutdown.load(Ordering::SeqCst) {
            break;
        }

        let playback_job = {
            let mut pq = manager.playback_queue.lock().unwrap();
            while pq.is_empty() && !manager.shutdown.load(Ordering::SeqCst) {
                let result = manager.playback_signal.wait(pq).unwrap();
                pq = result;
            }
            if manager.shutdown.load(Ordering::SeqCst) {
                return;
            }
            pq.pop_front()
        };

        if let Some((rx, hwnd, _req_id, generation, is_realtime)) = playback_job {
            let mut loading_cleared = false;

            // Loop reading chunks from this channel
            loop {
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
                        if generation < manager.interrupt_generation.load(Ordering::SeqCst) {
                            audio_player.stop();
                        } else {
                            audio_player.drain();
                        }
                        clear_tts_state(hwnd);
                        break;
                    }
                }

                if manager.shutdown.load(Ordering::SeqCst) {
                    return;
                }

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

/// Simple audio player using Windows WASAPI with loopback exclusion
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
    fn new(sample_rate: u32, manager: Arc<TtsManager>) -> Self {
        let shared_buffer: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
        let buffer_clone = shared_buffer.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        // Read config for device ID
        let target_device_id = {
            if let Ok(app) = crate::APP.lock() {
                let id = app.config.tts_output_device.clone();
                if id.is_empty() {
                    None
                } else {
                    Some(id)
                }
            } else {
                None
            }
        };

        // Spawn a dedicated thread for WASAPI playback
        let thread = std::thread::spawn(move || {
            // Initialize COM for this thread
            if wasapi::initialize_mta().is_err() {
                eprintln!("TTS: Failed to initialize COM");
                return;
            }

            // Try to create an AudioClient with loopback exclusion
            let result = Self::create_excluded_stream(
                sample_rate,
                buffer_clone.clone(),
                shutdown_clone.clone(),
                target_device_id,
                manager,
            );

            if let Err(e) = result {
                eprintln!(
                    "TTS: WASAPI with exclusion failed ({}), falling back to cpal",
                    e
                );
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

    fn create_excluded_stream(
        _sample_rate: u32,
        shared_buffer: Arc<Mutex<VecDeque<i16>>>,
        shutdown: Arc<AtomicBool>,
        target_device_id: Option<String>,
        manager: Arc<TtsManager>,
    ) -> anyhow::Result<()> {
        let buffer_clone = shared_buffer.clone();
        let shutdown_clone = shutdown.clone();

        // Attempt WASAPI with exclusion
        std::thread::spawn(move || {
            if let Err(e) = unsafe {
                Self::run_wasapi_excluded(
                    _sample_rate,
                    buffer_clone.clone(),
                    shutdown_clone.clone(),
                    target_device_id,
                    manager,
                )
            } {
                eprintln!(
                    "TTS: WASAPI exclusion FAILED with error: {:?}. Call ended.",
                    e
                );
            }
        });

        Ok(())
    }

    unsafe fn run_wasapi_excluded(
        _sample_rate: u32,
        shared_buffer: Arc<Mutex<VecDeque<i16>>>,
        shutdown: Arc<AtomicBool>,
        target_device_id: Option<String>,
        manager: Arc<TtsManager>,
    ) -> anyhow::Result<()> {
        // Use STA for better compatibility with audio drivers
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

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

        // Note: We no longer try to exclude from loopback
        let mix_format_ptr = client.GetMixFormat()?;
        let mix_format = *mix_format_ptr;

        // Initialize (Shared Mode)
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,       // flags
            1000000, // 100ms buffer
            0,
            mix_format_ptr,
            None,
        )?;

        let buffer_size = client.GetBufferSize()?;
        let render_client: IAudioRenderClient = client.GetService()?;

        client.Start()?;

        let channels = mix_format.nChannels as usize;
        let is_float = mix_format.wFormatTag == 3 // WAVE_FORMAT_IEEE_FLOAT
                       || (mix_format.wFormatTag == 65534 // WAVE_FORMAT_EXTENSIBLE 
                          && (mix_format.cbSize >= 22));

        let _frames_written = 0;

        let mut last_gen = manager.interrupt_generation.load(Ordering::SeqCst);

        while !shutdown.load(Ordering::Relaxed) {
            let current_gen = manager.interrupt_generation.load(Ordering::SeqCst);
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
                    let out_slice = std::slice::from_raw_parts_mut(
                        buffer_ptr as *mut f32,
                        (available as usize) * channels,
                    );

                    for i in 0..available as usize {
                        if let Some(sample) = deck.pop_front() {
                            let s = (sample as f32) / 32768.0;
                            for c in 0..channels {
                                out_slice[i * channels + c] = s;
                            }
                        } else {
                            // Silence when buffer is empty
                            for c in 0..channels {
                                out_slice[i * channels + c] = 0.0;
                            }
                        }
                    }
                } else {
                    // PCM i16
                    let out_slice = std::slice::from_raw_parts_mut(
                        buffer_ptr as *mut i16,
                        (available as usize) * channels,
                    );
                    for i in 0..available as usize {
                        if let Some(sample) = deck.pop_front() {
                            for c in 0..channels {
                                out_slice[i * channels + c] = sample;
                            }
                        } else {
                            for c in 0..channels {
                                out_slice[i * channels + c] = 0;
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
        // Get effective speed
        let effective_speed = if is_realtime {
            use crate::overlay::realtime_webview::state::{
                COMMITTED_TRANSLATION_QUEUE, CURRENT_TTS_SPEED, REALTIME_HWND,
                REALTIME_TTS_AUTO_SPEED, REALTIME_TTS_SPEED, WM_UPDATE_TTS_SPEED,
            };

            let base_speed = REALTIME_TTS_SPEED.load(Ordering::Relaxed);
            let auto_enabled = REALTIME_TTS_AUTO_SPEED.load(Ordering::Relaxed);

            // Auto-catchup: boost speed if queue is building up
            let queue_len = COMMITTED_TRANSLATION_QUEUE
                .lock()
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
                    use windows::Win32::Foundation::{LPARAM, WPARAM};
                    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
                    if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
                        let _ = PostMessageW(
                            Some(REALTIME_HWND),
                            WM_UPDATE_TTS_SPEED,
                            WPARAM(speed as usize),
                            LPARAM(0),
                        );
                    }
                    if !std::ptr::addr_of!(TRANSLATION_HWND).read().is_invalid() {
                        let _ = PostMessageW(
                            Some(TRANSLATION_HWND),
                            WM_UPDATE_TTS_SPEED,
                            WPARAM(speed as usize),
                            LPARAM(0),
                        );
                    }
                }
            }
            speed
        } else {
            100 // Normal speed for non-realtime TTS
        };

        let speed_ratio = effective_speed as f64 / 100.0;

        // Convert raw PCM bytes to i16 samples (little-endian)
        let input_samples: Vec<i16> = audio_data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        if input_samples.is_empty() {
            return;
        }

        // Apply WSOLA time-stretching
        let stretched_samples = if (speed_ratio - 1.0).abs() < 0.05 {
            input_samples
        } else {
            if let Ok(mut wsola) = self.wsola.lock() {
                let result = wsola.stretch(&input_samples, speed_ratio);
                if result.is_empty() {
                    return;
                }
                result
            } else {
                input_samples
            }
        };

        // Upsample from 24kHz to 48kHz (duplicate each sample)
        let output_samples: Vec<i16> = stretched_samples.iter().flat_map(|&s| [s, s]).collect();

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
    }
}
