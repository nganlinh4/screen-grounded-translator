//! Real-time audio transcription using Gemini Live API
//!
//! This module handles streaming audio to Gemini's native audio model
//! and receives real-time transcriptions via WebSocket.
//!
//! Translation is handled separately via Cerebras' gpt-oss-120b model
//! every 2 seconds for new sentence chunks.

mod capture;
mod state;
mod transcription;
mod translation;
mod utils;
mod websocket;

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

// Re-export public items
pub use state::{RealtimeState, SharedRealtimeState};
pub use transcription::start_realtime_transcription;
pub use translation::translate_with_google_gtx;

/// Interval for triggering translation (milliseconds)
pub const TRANSLATION_INTERVAL_MS: u64 = 1500;

/// Model for realtime audio transcription
pub const REALTIME_MODEL: &str = "gemini-2.5-flash-native-audio-preview-12-2025";

/// Custom message for updating overlay text
pub const WM_REALTIME_UPDATE: u32 = WM_APP + 200;
pub const WM_TRANSLATION_UPDATE: u32 = WM_APP + 201;
pub const WM_VOLUME_UPDATE: u32 = WM_APP + 202;
pub const WM_MODEL_SWITCH: u32 = WM_APP + 203;

// Shared RMS value for volume visualization
pub static REALTIME_RMS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
