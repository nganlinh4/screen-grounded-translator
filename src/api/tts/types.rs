/// Model for TTS (same native audio model, configured for output only)
pub const TTS_MODEL: &str = "gemini-2.5-flash-native-audio-preview-12-2025";

/// Output audio sample rate from Gemini (24kHz)
pub const SOURCE_SAMPLE_RATE: u32 = 24000;

/// Playback sample rate (48kHz - most devices support this)
pub const PLAYBACK_SAMPLE_RATE: u32 = 48000;

/// Events passed from socket workers to the player thread
pub enum AudioEvent {
    Data(Vec<u8>),
    End,
}

/// Request paired with its generation ID (to handle interrupts)
#[derive(Clone)]
pub struct QueuedRequest {
    pub req: TtsRequest,
    pub generation: u64,
}

/// TTS request with unique ID for cancellation
#[derive(Clone)]
pub struct TtsRequest {
    pub _id: u64,
    pub text: String,
    pub hwnd: isize,       // Window handle to update state when audio starts
    pub is_realtime: bool, // True if this is from realtime translation (uses REALTIME_TTS_SPEED)
}
