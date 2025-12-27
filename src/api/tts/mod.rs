//! Text-to-Speech using Gemini Live API
//!
//! This module provides persistent TTS capabilities using Gemini's native
//! audio model. The WebSocket connection is maintained at app startup
//! for instant speech synthesis with minimal latency.

pub mod edge_voices;
pub mod instance;
pub mod manager;
pub mod player;
pub mod types;
pub mod utils;
pub mod websocket;
pub mod worker;
pub mod wsola;

// Re-export public API for backward compatibility
pub use instance::TTS_MANAGER;
pub use manager::TtsManager;

/// Initialize the TTS system - call this at app startup
pub fn init_tts() {
    // Spawn 1 Player Thread
    let manager = TTS_MANAGER.clone();
    std::thread::spawn(move || {
        player::run_player_thread(manager);
    });

    // Spawn 2 Socket Worker Threads (Parallel Fetching)
    for _ in 0..2 {
        let manager = TTS_MANAGER.clone();
        std::thread::spawn(move || {
            worker::run_socket_worker(manager);
        });
    }
}
