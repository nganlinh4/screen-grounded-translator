//! Shared state for realtime transcription overlay

use crate::api::realtime_audio::{RealtimeState, SharedRealtimeState};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::collections::HashMap;
use std::num::NonZeroIsize;
use std::sync::{atomic::AtomicBool, Arc, Mutex, Once};
use windows::Win32::Foundation::*;
pub const WM_CLOSE_TTS_MODAL: u32 = 0x0400 + 400; // WM_USER + 400
pub const WM_UPDATE_TTS_SPEED: u32 = 0x0400 + 401; // WM_USER + 401
pub const WM_APP_REALTIME_START: u32 = 0x0400 + 500; // WM_USER + 500
pub const WM_APP_REALTIME_HIDE: u32 = 0x0400 + 501; // WM_USER + 501

// Gap between realtime and translation overlays
pub const GAP: i32 = 20;

lazy_static::lazy_static! {
    pub static ref REALTIME_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref REALTIME_STATE: SharedRealtimeState = Arc::new(Mutex::new(RealtimeState::new()));
    /// Signal to change audio source (true = restart with new source)
    pub static ref AUDIO_SOURCE_CHANGE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// The new audio source to use ("mic" or "device")
    pub static ref NEW_AUDIO_SOURCE: Mutex<String> = Mutex::new(String::new());
    /// Signal to change target language
    pub static ref LANGUAGE_CHANGE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// The new target language to use
    pub static ref NEW_TARGET_LANGUAGE: Mutex<String> = Mutex::new(String::new());
    /// Signal to change translation model
    pub static ref TRANSLATION_MODEL_CHANGE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// The new translation model to use ("google-gemma" or "groq-llama")
    pub static ref NEW_TRANSLATION_MODEL: Mutex<String> = Mutex::new(String::new());
    /// Visibility state for windows
    pub static ref MIC_VISIBLE: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    pub static ref TRANS_VISIBLE: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));

    // --- Per-App Audio Capture State ---
    /// Selected app's Process ID for per-app audio capture (0 = not selected / use mic)
    pub static ref SELECTED_APP_PID: Arc<std::sync::atomic::AtomicU32> = Arc::new(std::sync::atomic::AtomicU32::new(0));
    /// Selected app's name for display in UI
    pub static ref SELECTED_APP_NAME: Mutex<String> = Mutex::new(String::new());

    // --- Realtime TTS State ---
    /// Enable/disable realtime TTS for committed translations
    pub static ref REALTIME_TTS_ENABLED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// TTS playback speed (100 = 1.0x, 50 = 0.5x, 150 = 1.5x, etc.)
    pub static ref REALTIME_TTS_SPEED: Arc<std::sync::atomic::AtomicU32> = Arc::new(std::sync::atomic::AtomicU32::new(100));
    /// Auto-speed mode: automatically adjust speed based on queue length
    pub static ref REALTIME_TTS_AUTO_SPEED: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    /// Queue of committed translation text segments to speak
    pub static ref COMMITTED_TRANSLATION_QUEUE: Mutex<std::collections::VecDeque<String>> = Mutex::new(std::collections::VecDeque::new());

    // --- Window Handle for App Selection ---
    pub static ref APP_SELECTION_HWND: Arc<std::sync::atomic::AtomicIsize> = Arc::new(std::sync::atomic::AtomicIsize::new(0));
    /// Track how much of the committed text has been sent to TTS
    pub static ref LAST_SPOKEN_LENGTH: Arc<std::sync::atomic::AtomicUsize> = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    /// Current effective TTS speed (including auto-speed boost) for UI display
    pub static ref CURRENT_TTS_SPEED: Arc<std::sync::atomic::AtomicU32> = Arc::new(std::sync::atomic::AtomicU32::new(100));
}

pub static mut REALTIME_HWND: HWND = HWND(std::ptr::null_mut());
pub static mut TRANSLATION_HWND: HWND = HWND(std::ptr::null_mut());
pub static mut IS_ACTIVE: bool = false;
pub static mut IS_WARMED_UP: bool = false;

pub static REGISTER_REALTIME_CLASS: Once = Once::new();
pub static REGISTER_TRANSLATION_CLASS: Once = Once::new();

// Thread-local storage for WebViews
thread_local! {
    pub static REALTIME_WEBVIEWS: std::cell::RefCell<HashMap<isize, wry::WebView>> = std::cell::RefCell::new(HashMap::new());
    // Shared WebContext for this thread using common data directory
    pub static REALTIME_WEB_CONTEXT: std::cell::RefCell<Option<wry::WebContext>> = std::cell::RefCell::new(None);
}

/// Wrapper for HWND to implement HasWindowHandle
pub struct HwndWrapper(pub HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.0 .0 as isize;
        if let Some(non_zero) = NonZeroIsize::new(hwnd) {
            let mut handle = Win32WindowHandle::new(non_zero);
            handle.hinstance = None;
            let raw = RawWindowHandle::Win32(handle);
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        } else {
            Err(HandleError::Unavailable)
        }
    }
}
