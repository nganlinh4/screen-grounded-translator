// Input Handler - Drag-and-Drop and Paste handling for the main app UI
//
// When files/images are dropped or pasted (Ctrl+V), this module:
// 1. Detects the content type (image vs text)
// 2. Shows the appropriate preset wheel
// 3. Triggers the processing pipeline with the selected preset

use crate::overlay::preset_wheel::show_preset_wheel;
use crate::overlay::process::pipeline::{start_processing_pipeline, start_text_processing};
use crate::overlay::utils::get_clipboard_image_bytes;
use crate::APP;
use eframe::egui;
use image::{ImageBuffer, Rgba};
use std::path::Path;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Content types we can handle
pub enum DroppedContent {
    Image(ImageBuffer<Rgba<u8>, Vec<u8>>),
    Text(String),
}

/// Image file extensions we support
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tiff", "tif",
];

/// Text file extensions we support
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "json", "xml", "csv", "log", "ini", "cfg", "yaml", "yml", "toml", "html", "htm",
    "css", "js", "ts", "rs", "py", "java", "c", "cpp", "h", "hpp", "go", "rb", "php", "sql", "sh",
    "bat", "ps1", "swift", "kt",
];

/// Check if a file extension is an image type
fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Check if a file extension is a text type
fn is_text_extension(ext: &str) -> bool {
    TEXT_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// Load an image file and convert to ImageBuffer
fn load_image_file(path: &Path) -> Option<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    let img = image::open(path).ok()?;
    Some(img.to_rgba8())
}

/// Load a text file content
fn load_text_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Try to load file as image first, then as text
fn load_file_content(path: &Path) -> Option<DroppedContent> {
    let ext = path.extension()?.to_str()?;

    if is_image_extension(ext) {
        load_image_file(path).map(DroppedContent::Image)
    } else if is_text_extension(ext) {
        load_text_file(path).map(DroppedContent::Text)
    } else {
        // Try to load as text anyway for unknown extensions
        load_text_file(path).map(DroppedContent::Text)
    }
}

/// Get cursor position for wheel placement
fn get_cursor_pos() -> POINT {
    let mut pos = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pos);
    }
    pos
}

/// Get screen rect centered around cursor for result window placement
fn get_screen_rect_at_cursor() -> RECT {
    let pos = get_cursor_pos();
    RECT {
        left: pos.x - 200,
        top: pos.y - 100,
        right: pos.x + 200,
        bottom: pos.y + 100,
    }
}

/// Process dropped/pasted image content
fn process_image_content(img: ImageBuffer<Rgba<u8>, Vec<u8>>) {
    let cursor_pos = get_cursor_pos();

    // Show image preset wheel (no filter_mode = all image presets)
    let selected = show_preset_wheel("image", None, cursor_pos);

    if let Some(preset_idx) = selected {
        let (config, preset) = {
            let mut app = APP.lock().unwrap();
            // Update active preset for auto-paste to work correctly
            app.config.active_preset_idx = preset_idx;
            (app.config.clone(), app.config.presets[preset_idx].clone())
        };

        let rect = get_screen_rect_at_cursor();

        // Spawn processing in background thread
        std::thread::spawn(move || {
            start_processing_pipeline(img, rect, config, preset);
        });
    }
}

/// Process dropped/pasted text content
fn process_text_content(text: String) {
    let cursor_pos = get_cursor_pos();

    // Show text preset wheel without mode filter (shows both select and type presets)
    let selected = show_preset_wheel("text", None, cursor_pos);

    if let Some(preset_idx) = selected {
        let (config, preset) = {
            let mut app = APP.lock().unwrap();
            // Update active preset for auto-paste to work correctly
            app.config.active_preset_idx = preset_idx;
            (app.config.clone(), app.config.presets[preset_idx].clone())
        };

        let rect = get_screen_rect_at_cursor();
        let ui_lang = config.ui_language.clone();
        let localized_name =
            crate::gui::settings_ui::get_localized_preset_name(&preset.id, &ui_lang);
        let cancel_hotkey = preset
            .hotkeys
            .first()
            .map(|h| h.name.clone())
            .unwrap_or_default();

        // Spawn processing in background thread
        std::thread::spawn(move || {
            start_text_processing(text, rect, config, preset, localized_name, cancel_hotkey);
        });
    }
}

/// Handle dropped files from egui
pub fn handle_dropped_files(ctx: &egui::Context) -> bool {
    let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());

    if dropped_files.is_empty() {
        return false;
    }

    // Process the first dropped file
    if let Some(file) = dropped_files.first() {
        // Try to get the file path
        if let Some(path) = &file.path {
            if let Some(content) = load_file_content(path) {
                match content {
                    DroppedContent::Image(img) => {
                        std::thread::spawn(move || {
                            process_image_content(img);
                        });
                        return true;
                    }
                    DroppedContent::Text(text) => {
                        std::thread::spawn(move || {
                            process_text_content(text);
                        });
                        return true;
                    }
                }
            }
        }
        // If path is not available, check for bytes (e.g., from some drag sources)
        else if let Some(bytes) = &file.bytes {
            // Try to interpret as image first
            if let Ok(img) = image::load_from_memory(bytes) {
                let rgba = img.to_rgba8();
                std::thread::spawn(move || {
                    process_image_content(rgba);
                });
                return true;
            }
            // Try as text
            if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                std::thread::spawn(move || {
                    process_text_content(text);
                });
                return true;
            }
        }
    }

    false
}

/// Check if files are currently being dragged over the window (not yet dropped)
pub fn is_files_hovered(ctx: &egui::Context) -> bool {
    ctx.input(|i| !i.raw.hovered_files.is_empty())
}

/// Get text from Windows clipboard
fn get_clipboard_text() -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    unsafe {
        // Try to open clipboard
        for _attempt in 0..5 {
            if OpenClipboard(None).is_ok() {
                // CF_UNICODETEXT = 13
                if let Ok(h_data) = GetClipboardData(13) {
                    let ptr = GlobalLock(HGLOBAL(h_data.0));
                    if !ptr.is_null() {
                        // Read as wide string
                        let wide_ptr = ptr as *const u16;
                        let mut len = 0;
                        while *wide_ptr.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(wide_ptr, len);
                        let text = String::from_utf16_lossy(slice);

                        let _ = GlobalUnlock(HGLOBAL(h_data.0));
                        let _ = CloseClipboard();

                        if !text.is_empty() {
                            return Some(text);
                        }
                        return None;
                    }
                }
                let _ = CloseClipboard();
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }
}

/// Handle Ctrl+V paste - uses Windows API for keyboard detection
pub fn handle_paste(ctx: &egui::Context) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_V};

    // Skip paste handling if help assistant modal is open
    // This allows normal Ctrl+V paste into the text input field
    if crate::gui::settings_ui::help_assistant::is_modal_open() {
        return false;
    }

    // Only process if our window has focus
    let has_focus = ctx.input(|i| i.focused);
    if !has_focus {
        return false;
    }

    // Debounce: prevent multiple triggers per key press
    static LAST_V_STATE: AtomicBool = AtomicBool::new(false);

    // Check keyboard state using Windows API
    let ctrl_down = unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
    let v_down = unsafe { (GetAsyncKeyState(VK_V.0 as i32) as u16 & 0x8000) != 0 };
    let v_was_down = LAST_V_STATE.swap(v_down, Ordering::SeqCst);

    // Trigger on V key press (not release)
    let ctrl_v_just_pressed = ctrl_down && v_down && !v_was_down;

    // Also check egui events as fallback
    let paste_event = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .any(|e| matches!(e, egui::Event::Paste(_)))
    });

    if !ctrl_v_just_pressed && !paste_event {
        return false;
    }

    // First try to get image from clipboard (images take priority)
    if let Some(img_bytes) = get_clipboard_image_bytes() {
        if let Ok(img) = image::load_from_memory(&img_bytes) {
            let rgba = img.to_rgba8();
            std::thread::spawn(move || {
                process_image_content(rgba);
            });
            return true;
        }
    }

    // Try to get text from clipboard via Windows API
    if let Some(text) = get_clipboard_text() {
        if !text.is_empty() {
            std::thread::spawn(move || {
                process_text_content(text);
            });
            return true;
        }
    }

    false
}
