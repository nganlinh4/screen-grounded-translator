pub mod types;
pub mod client;
pub mod vision;
pub mod audio;
pub mod text;
pub mod realtime_audio;
pub mod ollama;

pub use vision::translate_image_streaming;
pub use text::{translate_text_streaming, refine_text_streaming};
pub use audio::record_audio_and_transcribe;
// realtime_audio types/functions are used directly where needed via crate::api::realtime_audio::

/// Special prefix signal that tells callbacks to clear their accumulator before processing
/// When a chunk starts with this, the callback should: 1) Clear acc 2) Add the content after this prefix
pub const WIPE_SIGNAL: &str = "\x00WIPE\x00";
