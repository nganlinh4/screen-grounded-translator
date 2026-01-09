//! Gemini Live LLM API
//!
//! This module provides access to Gemini's native audio model as a standard LLM,
//! using the bidirectional WebSocket API for low-latency streaming text responses.
//!
//! Unlike the standard REST API, this uses a connection pool for faster response times.
//! Supports text, image, and audio inputs with text-only output.

pub mod manager;
pub mod types;
pub mod websocket;
pub mod worker;

use std::sync::Arc;

pub use manager::GeminiLiveManager;
pub use types::{LiveEvent, LiveInputContent};

lazy_static::lazy_static! {
    /// Global Gemini Live manager instance
    pub static ref GEMINI_LIVE_MANAGER: Arc<GeminiLiveManager> = Arc::new(GeminiLiveManager::new());
}

/// Number of worker threads for the connection pool
const WORKER_COUNT: usize = 2;

/// Initialize the Gemini Live LLM system - call this at app startup
pub fn init_gemini_live() {
    for _ in 0..WORKER_COUNT {
        let manager = GEMINI_LIVE_MANAGER.clone();
        std::thread::spawn(move || {
            worker::run_live_worker(manager);
        });
    }
}

/// Streaming text generation using Gemini Live API
/// This is the main entry point for using Gemini Live as an LLM
///
/// Arguments:
/// - `text`: The user prompt text
/// - `instruction`: System instruction / prompt template
/// - `image_data`: Optional image data (bytes, mime_type)
/// - `audio_data`: Optional audio data (PCM 16-bit mono 16kHz)
/// - `streaming_enabled`: Whether to stream chunks or wait for complete response
/// - `ui_language`: UI language for thinking message
/// - `on_chunk`: Callback for each text chunk
///
/// Returns: Complete response text or error
pub fn gemini_live_generate<F>(
    text: String,
    instruction: String,
    image_data: Option<(Vec<u8>, String)>,
    audio_data: Option<Vec<u8>>,
    streaming_enabled: bool,
    ui_language: &str,
    mut on_chunk: F,
) -> anyhow::Result<String>
where
    F: FnMut(&str),
{
    // Log what we're sending
    let content_type = match (&image_data, &audio_data) {
        (Some((img, mime)), _) => format!("TextWithImage ({}bytes, {})", img.len(), mime),
        (None, Some(audio)) => format!("TextWithAudio ({}bytes)", audio.len()),
        (None, None) => format!("Text ({}chars)", text.len()),
    };
    println!("[GeminiLive] gemini_live_generate called: {}", content_type);
    println!(
        "[GeminiLive] instruction len: {}, streaming: {}",
        instruction.len(),
        streaming_enabled
    );

    // Build input content based on what's provided
    let content = match (image_data, audio_data) {
        (Some((img, mime)), _) => LiveInputContent::TextWithImage {
            text,
            image_data: img,
            mime_type: mime,
        },
        (None, Some(audio)) => {
            if text.trim().is_empty() {
                LiveInputContent::AudioOnly(audio)
            } else {
                LiveInputContent::TextWithAudio {
                    text,
                    audio_data: audio,
                }
            }
        }
        (None, None) => LiveInputContent::Text(text),
    };

    // Check if model supports thinking (always enabled for this model)
    let show_thinking = true;

    // Send request to the manager
    let (id, rx) = GEMINI_LIVE_MANAGER.request(content, instruction, show_thinking);
    println!("[GeminiLive] Request queued with ID: {}", id);

    let mut full_content = String::new();
    let mut thinking_shown = false;
    let mut content_started = false;
    let mut event_count = 0;

    let locale = crate::gui::locale::LocaleText::get(ui_language);

    // Process events from the worker
    loop {
        match rx.recv() {
            Ok(LiveEvent::Thinking) => {
                event_count += 1;
                println!("[GeminiLive] Event {}: Thinking", event_count);
                if !thinking_shown && !content_started {
                    if streaming_enabled {
                        on_chunk(locale.model_thinking);
                    }
                    thinking_shown = true;
                }
            }
            Ok(LiveEvent::TextChunk(chunk)) => {
                event_count += 1;
                println!(
                    "[GeminiLive] Event {}: TextChunk ({}bytes)",
                    event_count,
                    chunk.len()
                );
                if streaming_enabled {
                    // If we showed thinking, wipe it on first content
                    if !content_started && thinking_shown {
                        content_started = true;
                        full_content.push_str(&chunk);
                        let wipe_content = format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                        on_chunk(&wipe_content);
                    } else {
                        content_started = true;
                        full_content.push_str(&chunk);
                        on_chunk(&chunk);
                    }
                } else {
                    content_started = true;
                    full_content.push_str(&chunk);
                }
            }
            Ok(LiveEvent::TurnComplete) => {
                event_count += 1;
                println!(
                    "[GeminiLive] Event {}: TurnComplete (total content: {}bytes)",
                    event_count,
                    full_content.len()
                );
                if !streaming_enabled && !full_content.is_empty() {
                    on_chunk(&full_content);
                }
                break;
            }
            Ok(LiveEvent::Error(e)) => {
                event_count += 1;
                println!("[GeminiLive] Event {}: Error - {}", event_count, e);
                if e.contains("NO_API_KEY") {
                    return Err(anyhow::anyhow!("{}", e));
                }
                return Err(anyhow::anyhow!("Gemini Live error: {}", e));
            }
            Err(e) => {
                println!("[GeminiLive] Channel error: {:?}", e);
                // Channel closed - worker finished unexpectedly
                break;
            }
        }
    }

    println!(
        "[GeminiLive] gemini_live_generate complete: {}bytes result",
        full_content.len()
    );
    Ok(full_content)
}
