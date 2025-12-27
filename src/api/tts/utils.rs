use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

use crate::overlay::result::state::WINDOW_STATES;

/// Clear the TTS loading state for a window and trigger repaint
pub fn clear_tts_loading_state(hwnd: isize) {
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd) {
            state.tts_loading = false;
        }
    }

    // Trigger repaint to update button appearance
    unsafe {
        let _ = InvalidateRect(Some(HWND(hwnd as *mut std::ffi::c_void)), None, false);
    }
}

/// Clear TTS state completely when speech ends
pub fn clear_tts_state(hwnd: isize) {
    {
        let mut states = WINDOW_STATES.lock().unwrap();
        if let Some(state) = states.get_mut(&hwnd) {
            state.tts_loading = false;
            state.tts_request_id = 0;
        }
    }

    // Trigger repaint to update button appearance
    unsafe {
        let _ = InvalidateRect(Some(HWND(hwnd as *mut std::ffi::c_void)), None, false);
    }
}

/// Detect language of text and get matching TTS instruction from config conditions
pub fn get_language_instruction_for_text(
    text: &str,
    conditions: &[crate::config::TtsLanguageCondition],
) -> Option<String> {
    // Use whatlang for fast language detection (70 languages supported)
    // Returns None if text is too short or language is unclear
    let detected = whatlang::detect_lang(text)?;
    let detected_code = detected.code(); // ISO 639-3 code (e.g., "vie", "kor", "eng")

    // Find matching condition
    for condition in conditions {
        if condition.language_code.eq_ignore_ascii_case(detected_code) {
            return Some(condition.instruction.clone());
        }
    }
    None
}

/// List available audio output devices (ID, Name)
pub fn get_output_devices() -> Vec<(String, String)> {
    let mut devices = Vec::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        {
            if let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                if let Ok(count) = collection.GetCount() {
                    for i in 0..count {
                        if let Ok(device) = collection.Item(i) {
                            if let Ok(id) = device.GetId() {
                                let id_str = id.to_string().unwrap_or_default();
                                // Try to get friendly name
                                let name = if let Ok(_props) = device.OpenPropertyStore(STGM_READ) {
                                    // PKEY_Device_FriendlyName would be ideal but requires property key definition
                                    // For now, let's just use the ID or partial ID if needed,
                                    // but usually we rely on the ID.
                                    // If we really need the name, we would need to implement property getters.
                                    // Given the original code had this logic, we keep it simple or reuse if possible.
                                    // The original code comment said: "In windows 0.62, PropVariant access is verbose."
                                    // and "Let's rely on the ID matching...".
                                    id_str.clone()
                                } else {
                                    id_str.clone()
                                };
                                devices.push((id_str, name));
                            }
                        }
                    }
                }
            }
        }
    }
    devices
}
