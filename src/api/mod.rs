pub mod audio;
pub mod client;
pub mod gemini_live;
pub mod ollama;
pub mod realtime_audio;
pub mod text;
pub mod tts;
pub mod types;
pub mod vision;

pub use audio::record_and_stream_gemini_live;
pub use audio::record_audio_and_transcribe;
pub use text::{refine_text_streaming, translate_text_streaming};
pub use vision::translate_image_streaming;
// realtime_audio types/functions are used directly where needed via crate::api::realtime_audio::

/// Special prefix signal that tells callbacks to clear their accumulator before processing
/// When a chunk starts with this, the callback should: 1) Clear acc 2) Add the content after this prefix
pub const WIPE_SIGNAL: &str = "\x00WIPE\x00";
