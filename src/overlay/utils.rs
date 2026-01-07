use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{
    CreateDIBitmap, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, CBM_INIT, DIB_RGB_COLORS,
};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn to_wstring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Global switch for the "context quote" displayed in result windows during refining.
/// Set to false to hide the quote and only show the glow animation.
pub const SHOW_REFINING_CONTEXT_QUOTE: bool = false;

pub fn get_context_quote(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let len = words.len();
    if len > 50 {
        format!("\"... {}\"", words[len - 50..].join(" "))
    } else {
        format!("\"... {}\"", words.join(" "))
    }
}

// --- CLIPBOARD SUPPORT ---
pub fn copy_to_clipboard(text: &str, hwnd: HWND) {
    unsafe {
        // Retry loop to handle temporary clipboard locks
        for attempt in 0..5 {
            if OpenClipboard(Some(hwnd)).is_ok() {
                let _ = EmptyClipboard();

                // Convert text to UTF-16
                let wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let mem_size = wide_text.len() * 2;

                // Allocate global memory
                if let Ok(h_mem) = GlobalAlloc(GMEM_MOVEABLE, mem_size) {
                    let ptr = GlobalLock(h_mem) as *mut u16;
                    std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr, wide_text.len());
                    let _ = GlobalUnlock(h_mem);

                    // Set clipboard data (CF_UNICODETEXT = 13)
                    let h_mem_handle = HANDLE(h_mem.0);
                    let _ = SetClipboardData(13u32, Some(h_mem_handle));
                }

                let _ = CloseClipboard();
                return; // Success
            }

            // If failed and not last attempt, wait before retrying
            if attempt < 4 {
                std::thread::sleep(std::time::Duration::from_millis(10));
            } else {
                eprintln!("Failed to copy to clipboard after 5 attempts");
            }
        }
    }
}

pub fn copy_image_to_clipboard(image_bytes: &[u8]) {
    // Convert PNG/etc bytes to BMP format using image crate
    // Clipboard expects CF_DIB which is BMP without the File Header (first 14 bytes)
    if let Ok(img) = image::load_from_memory(image_bytes) {
        let mut bmp_data = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut bmp_data);
        if img.write_to(&mut cursor, image::ImageFormat::Bmp).is_ok() {
            // Check if valid BMP (starts with BM)
            if bmp_data.len() > 14 && bmp_data[0] == 0x42 && bmp_data[1] == 0x4D {
                // Skip the 14-byte BITMAPFILEHEADER to get BITMAPINFOHEADER + Pixels (DIB)
                let dib_data = &bmp_data[14..];

                unsafe {
                    // Retry loop
                    for attempt in 0..5 {
                        if OpenClipboard(None).is_ok() {
                            let _ = EmptyClipboard();

                            let mem_size = dib_data.len();
                            if let Ok(h_mem) = GlobalAlloc(GMEM_MOVEABLE, mem_size) {
                                let ptr = GlobalLock(h_mem) as *mut u8;
                                std::ptr::copy_nonoverlapping(dib_data.as_ptr(), ptr, mem_size);
                                let _ = GlobalUnlock(h_mem);

                                // Set CF_DIB (8)
                                let h_mem_handle = HANDLE(h_mem.0);
                                let _ = SetClipboardData(8, Some(h_mem_handle));

                                // ALSO set CF_BITMAP (2) to ensure Windows Clipboard History picks it up.
                                // Many modern Windows apps/features prefer having a GDI handle or both formats.
                                {
                                    let hdc = GetDC(None);
                                    if !hdc.is_invalid() {
                                        // Read header size (first 4 bytes of DIB data)
                                        if dib_data.len() >= 4 {
                                            let header_size = u32::from_le_bytes(
                                                dib_data[0..4].try_into().unwrap_or([0; 4]),
                                            );
                                            // The bits usually start after the header.
                                            // Make sure we don't go out of bounds.
                                            if (header_size as usize) < dib_data.len() {
                                                let bits_ptr =
                                                    dib_data.as_ptr().add(header_size as usize);
                                                let pbmih =
                                                    dib_data.as_ptr() as *const BITMAPINFOHEADER;
                                                let pbmi = dib_data.as_ptr() as *const BITMAPINFO;

                                                let hbitmap = CreateDIBitmap(
                                                    hdc,
                                                    Some(pbmih),
                                                    CBM_INIT as u32,
                                                    Some(bits_ptr as *const std::ffi::c_void),
                                                    Some(pbmi),
                                                    DIB_RGB_COLORS,
                                                );

                                                if !hbitmap.is_invalid() {
                                                    // ownership transferred to system
                                                    let _ = SetClipboardData(
                                                        2, // CF_BITMAP
                                                        Some(HANDLE(hbitmap.0 as *mut _)),
                                                    );
                                                }
                                            }
                                        }
                                        ReleaseDC(None, hdc);
                                    }
                                }

                                let _ = CloseClipboard();
                                return;
                            }
                        }
                        if attempt < 4 {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                }
            }
        }
    }
}

/// Read image bytes from clipboard (returns PNG-encoded bytes)
/// Returns None if no image is available in clipboard
pub fn get_clipboard_image_bytes() -> Option<Vec<u8>> {
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;

    unsafe {
        // Try to open clipboard with retries
        for _attempt in 0..5 {
            if OpenClipboard(None).is_ok() {
                // Check if any image format is available
                // CF_DIB = 8, CF_DIBV5 = 17
                let has_dib = IsClipboardFormatAvailable(8).is_ok();
                let has_dibv5 = IsClipboardFormatAvailable(17).is_ok();

                if !has_dib && !has_dibv5 {
                    let _ = CloseClipboard();
                    return None;
                }

                // Try CF_DIB first (8), then CF_DIBV5 (17)
                let format_to_try = if has_dib { 8u32 } else { 17u32 };

                if let Ok(h_data) = GetClipboardData(format_to_try) {
                    let ptr = GlobalLock(HGLOBAL(h_data.0));
                    if !ptr.is_null() {
                        // Get the size of the global memory block
                        let size = GlobalSize(HGLOBAL(h_data.0));
                        if size > 0 {
                            // Read DIB data
                            let dib_data = std::slice::from_raw_parts(ptr as *const u8, size);

                            // Parse BITMAPINFOHEADER to get dimensions
                            if dib_data.len() >= std::mem::size_of::<BITMAPINFOHEADER>() {
                                let header = &*(dib_data.as_ptr() as *const BITMAPINFOHEADER);
                                let width = header.biWidth;
                                let height = header.biHeight.abs();
                                let bit_count = header.biBitCount;
                                let is_top_down = header.biHeight < 0;

                                // We support 24-bit and 32-bit images
                                if (bit_count == 24 || bit_count == 32) && width > 0 && height > 0 {
                                    // Calculate the offset to pixel data
                                    let header_size = header.biSize as usize;
                                    let color_table_size = if header.biClrUsed > 0 {
                                        (header.biClrUsed as usize) * 4
                                    } else if bit_count <= 8 {
                                        (1 << bit_count) * 4
                                    } else {
                                        0
                                    };
                                    let pixel_offset = header_size + color_table_size;

                                    if dib_data.len() > pixel_offset {
                                        let pixel_data = &dib_data[pixel_offset..];
                                        let bytes_per_pixel = (bit_count / 8) as usize;
                                        let row_size =
                                            ((width as usize * bytes_per_pixel + 3) / 4) * 4; // DWORD aligned

                                        // Create RGBA buffer
                                        let mut rgba_buffer =
                                            Vec::with_capacity((width * height * 4) as usize);

                                        for y in 0..height {
                                            let src_y =
                                                if is_top_down { y } else { height - 1 - y };
                                            let row_start = (src_y as usize) * row_size;

                                            for x in 0..width {
                                                let px_start =
                                                    row_start + (x as usize) * bytes_per_pixel;
                                                if px_start + bytes_per_pixel <= pixel_data.len() {
                                                    // DIB is BGR(A)
                                                    let b = pixel_data[px_start];
                                                    let g = pixel_data[px_start + 1];
                                                    let r = pixel_data[px_start + 2];
                                                    let a = if bytes_per_pixel == 4 {
                                                        pixel_data[px_start + 3]
                                                    } else {
                                                        255
                                                    };
                                                    rgba_buffer.push(r);
                                                    rgba_buffer.push(g);
                                                    rgba_buffer.push(b);
                                                    rgba_buffer.push(a);
                                                }
                                            }
                                        }

                                        let _ = GlobalUnlock(HGLOBAL(h_data.0));
                                        let _ = CloseClipboard();

                                        // Encode as PNG
                                        if let Some(img) =
                                            image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                                                width as u32,
                                                height as u32,
                                                rgba_buffer,
                                            )
                                        {
                                            let mut png_data = Vec::new();
                                            let mut cursor = std::io::Cursor::new(&mut png_data);
                                            if img
                                                .write_to(&mut cursor, image::ImageFormat::Png)
                                                .is_ok()
                                            {
                                                return Some(png_data);
                                            }
                                        }

                                        return None;
                                    }
                                }
                            }
                        }
                        let _ = GlobalUnlock(HGLOBAL(h_data.0));
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

// --- AUTO PASTE UTILS ---

/// Checks active window for caret OR keyboard focus and returns its HWND if found
pub fn get_target_window_for_paste() -> Option<HWND> {
    unsafe {
        let hwnd_foreground = GetForegroundWindow();
        if hwnd_foreground.is_invalid() {
            return None;
        }

        let thread_id = GetWindowThreadProcessId(hwnd_foreground, None);
        if thread_id == 0 {
            return None;
        }

        let mut gui_info = GUITHREADINFO::default();
        gui_info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

        if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok() {
            // Check legacy caret
            let has_caret = !gui_info.hwndCaret.is_invalid();
            let blinking = (gui_info.flags & GUI_CARETBLINKING).0 != 0;

            // Check keyboard focus (Fix for Chrome/Electron/WPF)
            let has_focus = !gui_info.hwndFocus.is_invalid();

            if has_caret || blinking || has_focus {
                return Some(hwnd_foreground);
            }
        }

        None
    }
}

pub fn force_focus_and_paste(hwnd_target: HWND) {
    unsafe {
        // 1. Force focus back to the target window
        if IsWindow(Some(hwnd_target)).as_bool() {
            let cur_thread = GetCurrentThreadId();
            let target_thread = GetWindowThreadProcessId(hwnd_target, None);

            if cur_thread != target_thread {
                let _ = AttachThreadInput(cur_thread, target_thread, true);
                let _ = SetForegroundWindow(hwnd_target);
                // Important: Bring window to top so it receives input
                let _ = BringWindowToTop(hwnd_target);
                let _ = SetFocus(Some(hwnd_target));
                let _ = AttachThreadInput(cur_thread, target_thread, false);
            } else {
                let _ = SetForegroundWindow(hwnd_target);
            }
        } else {
            return;
        }

        // 2. Wait for focus to settle
        std::thread::sleep(std::time::Duration::from_millis(350));

        // 3. CLEANUP MODIFIERS SMARTLY
        // Only send KeyUp if the key is actually physically pressed to avoid side effects
        let release_if_pressed = |vk: u16| {
            let state = GetAsyncKeyState(vk as i32);
            if (state as u16 & 0x8000) != 0 {
                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(vk),
                            dwFlags: KEYEVENTF_KEYUP,
                            ..Default::default()
                        },
                    },
                };
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
        };

        release_if_pressed(VK_MENU.0); // Alt
        release_if_pressed(VK_SHIFT.0); // Shift
        release_if_pressed(VK_LWIN.0); // Win Left
        release_if_pressed(VK_RWIN.0); // Win Right
        release_if_pressed(VK_CONTROL.0); // Ctrl

        std::thread::sleep(std::time::Duration::from_millis(50));

        // 4. Send Ctrl+V Sequence
        let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
            let input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                        wScan: 0,
                    },
                },
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        };

        // Ctrl Down
        send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0));
        std::thread::sleep(std::time::Duration::from_millis(50));

        // V Down
        send_input_event(VK_V.0, KEYBD_EVENT_FLAGS(0));
        std::thread::sleep(std::time::Duration::from_millis(50));

        // V Up
        send_input_event(VK_V.0, KEYEVENTF_KEYUP);
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Ctrl Up
        send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);
    }
}

pub fn type_text_to_window(hwnd_target: HWND, text: &str) {
    if text.is_empty() {
        return;
    }
    unsafe {
        // Ensure target is foreground
        let fg_window = GetForegroundWindow();
        if fg_window != hwnd_target {
            let cur_thread = GetCurrentThreadId();
            let target_thread = GetWindowThreadProcessId(hwnd_target, None);
            if cur_thread != target_thread {
                let _ = AttachThreadInput(cur_thread, target_thread, true);
                let _ = SetForegroundWindow(hwnd_target);
                let _ = AttachThreadInput(cur_thread, target_thread, false);
            } else {
                let _ = SetForegroundWindow(hwnd_target);
            }
        }

        // Send Chars
        for c in text.chars() {
            let mut buffer = [0u16; 2];
            let encoded = c.encode_utf16(&mut buffer);

            for utf16_val in encoded.iter() {
                let val = *utf16_val;
                let input_down = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: val,
                            dwFlags: KEYEVENTF_UNICODE,
                            ..Default::default()
                        },
                    },
                };
                let input_up = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: val,
                            dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                            ..Default::default()
                        },
                    },
                };
                SendInput(&[input_down, input_up], std::mem::size_of::<INPUT>() as i32);
            }
        }
    }
}

pub fn get_error_message(error: &str, lang: &str, model_name: Option<&str>) -> String {
    // Parse NO_API_KEY:provider format
    if error.starts_with("NO_API_KEY") {
        let provider = if error.contains(':') {
            let parts: Vec<&str> = error.split(':').collect();
            if parts.len() > 1 {
                match parts[1] {
                    "groq" => "Groq",
                    "google" => "Google Gemini",
                    "openai" => "OpenAI",
                    other => other,
                }
            } else {
                "API"
            }
        } else {
            "API"
        };

        return match lang {
            "vi" => format!("Bạn chưa nhập {} API key!", provider),
            "ko" => format!("{} API 키를 입력하지 않았습니다!", provider),
            "ja" => format!("{} APIキーが入力されていません!", provider),
            "zh" => format!("您还没有输入 {} API key!", provider),
            _ => format!("You haven't entered a {} API key!", provider),
        };
    }

    // Parse INVALID_API_KEY:provider format
    if error.starts_with("INVALID_API_KEY") {
        let provider = if error.contains(':') {
            let parts: Vec<&str> = error.split(':').collect();
            if parts.len() > 1 {
                match parts[1] {
                    "groq" => "Groq",
                    "google" => "Google Gemini",
                    "openai" => "OpenAI",
                    other => other,
                }
            } else {
                "API"
            }
        } else {
            "API"
        };

        return match lang {
            "vi" => format!("{} API key không hợp lệ!", provider),
            "ko" => format!("{} API 키가 유효하지 않습니다!", provider),
            "ja" => format!("{} APIキーが無効です!", provider),
            "zh" => format!("{} API key 无效!", provider),
            _ => format!("Invalid {} API key!", provider),
        };
    }

    // Parse HTTP status codes from API error messages
    // Example: "Error: https://api.groq.com/openai/v1/chat/completions: status code 429"
    if let Some(status_code) = extract_http_status_code(error) {
        let provider = extract_provider_from_error(error);
        return format_http_error(status_code, &provider, model_name, lang);
    }

    // Fallback for other errors
    match lang {
        "vi" => format!("Lỗi: {}", error),
        "ko" => format!("오류: {}", error),
        "ja" => format!("エラー: {}", error),
        "zh" => format!("错误: {}", error),
        _ => format!("Error: {}", error),
    }
}

/// Extracts HTTP status code from error message
fn extract_http_status_code(error: &str) -> Option<u16> {
    // Pattern: "status code XXX" or just a 3-digit code at the end
    if let Some(pos) = error.find("status code ") {
        let after = &error[pos + 12..];
        let code_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        return code_str.parse().ok();
    }

    // Also check for patterns like ": 429" at the end
    let trimmed = error.trim();
    if trimmed.len() >= 3 {
        let last_3: String = trimmed
            .chars()
            .rev()
            .take(3)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if last_3.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(code) = last_3.parse::<u16>() {
                if (400..=599).contains(&code) {
                    return Some(code);
                }
            }
        }
    }

    // Check for "XXX" anywhere (common error codes)
    for code in [429, 400, 401, 403, 404, 500, 502, 503, 504] {
        if error.contains(&code.to_string()) {
            return Some(code);
        }
    }

    None
}

/// Extracts provider name from error URL
fn extract_provider_from_error(error: &str) -> String {
    if error.contains("api.groq.com") {
        "Groq".to_string()
    } else if error.contains("generativelanguage.googleapis.com") || error.contains("gemini") {
        "Google Gemini".to_string()
    } else if error.contains("api.openai.com") {
        "OpenAI".to_string()
    } else if error.contains("api.anthropic.com") || error.contains("claude") {
        "Anthropic".to_string()
    } else {
        "API".to_string()
    }
}

/// Formats HTTP error with localized message
fn format_http_error(
    status_code: u16,
    provider: &str,
    model_name: Option<&str>,
    lang: &str,
) -> String {
    // Format the model/provider info for display
    let model_info = if let Some(model) = model_name {
        format!("{} ({})", model, provider)
    } else {
        provider.to_string()
    };

    match status_code {
        429 => match lang {
            "vi" => format!("Lỗi 429: Đã vượt quá hạn mức của mô hình {} (Rate Limit). Vui lòng chờ một lát rồi thử lại.", model_info),
            "ko" => format!("오류 429: {} 모델의 요청 제한 초과 (Rate Limit). 잠시 후 다시 시도해 주세요.", model_info),
            "ja" => format!("エラー 429: {} のレート制限を超えました。しばらくしてから再試行してください。", model_info),
            "zh" => format!("错误 429: {} 模型请求超出限制 (Rate Limit)。请稍后再试。", model_info),
            _ => format!("Error 429: Rate limit exceeded for model {}. Please wait a moment and try again.", model_info),
        },
        400 => match lang {
            "vi" => format!("Lỗi 400: Yêu cầu không hợp lệ đến {}. Vui lòng kiểm tra lại cài đặt.", model_info),
            "ko" => format!("오류 400: {}에 대한 잘못된 요청입니다. 설정을 확인해 주세요.", model_info),
            "ja" => format!("エラー 400: {} へのリクエストが無効です。設定を確認してください。", model_info),
            "zh" => format!("错误 400: {} 请求无效。请检查设置。", model_info),
            _ => format!("Error 400: Bad request to {}. Please check your settings.", model_info),
        },
        401 => match lang {
            "vi" => format!("Lỗi 401: API key của {} không hợp lệ hoặc đã hết hạn.", provider),
            "ko" => format!("오류 401: {} API 키가 유효하지 않거나 만료되었습니다.", provider),
            "ja" => format!("エラー 401: {} の API キーが無効または期限切れです。", provider),
            "zh" => format!("错误 401: {} API 密钥无效或已过期。", provider),
            _ => format!("Error 401: {} API key is invalid or expired.", provider),
        },
        403 => match lang {
            "vi" => format!("Lỗi 403: Không có quyền truy cập {}. Vui lòng kiểm tra API key.", provider),
            "ko" => format!("오류 403: {}에 대한 접근 권한이 없습니다. API 키를 확인해 주세요.", provider),
            "ja" => format!("エラー 403: {} へのアクセス権限がありません。API キーを確認してください。", provider),
            "zh" => format!("错误 403: 无权访问 {}。请检查 API 密钥。", provider),
            _ => format!("Error 403: Access forbidden to {}. Please check your API key.", provider),
        },
        404 => match lang {
            "vi" => format!("Lỗi 404: Không tìm thấy mô hình {} trên {}.", model_name.unwrap_or("này"), provider),
            "ko" => format!("오류 404: {}에서 {} 모델을 찾을 수 없습니다.", provider, model_name.unwrap_or("해당")),
            "ja" => format!("エラー 404: {} で {} が見つかりません。", provider, model_name.unwrap_or("このモデル")),
            "zh" => format!("错误 404: 在 {} 上找不到模型 {}。", provider, model_name.unwrap_or("此")),
            _ => format!("Error 404: Model {} not found on {}.", model_name.unwrap_or("this"), provider),
        },
        500 => match lang {
            "vi" => format!("Lỗi 500: Máy chủ {} gặp lỗi nội bộ. Vui lòng thử lại sau.", provider),
            "ko" => format!("오류 500: {} 서버 내부 오류입니다. 나중에 다시 시도해 주세요.", provider),
            "ja" => format!("エラー 500: {} サーバー内部エラー。後で再試行してください。", provider),
            "zh" => format!("错误 500: {} 服务器内部错误。请稍后再试。", provider),
            _ => format!("Error 500: {} internal server error. Please try again later.", provider),
        },
        502 => match lang {
            "vi" => format!("Lỗi 502: Bad Gateway - {} đang gặp sự cố. Vui lòng thử lại sau.", provider),
            "ko" => format!("오류 502: Bad Gateway - {}에 문제가 발생했습니다. 나중에 다시 시도해 주세요.", provider),
            "ja" => format!("エラー 502: Bad Gateway - {} に問題が発生しています。後で再試行してください。", provider),
            "zh" => format!("错误 502: Bad Gateway - {} 遇到问题。请稍后再试。", provider),
            _ => format!("Error 502: Bad Gateway - {} is having issues. Please try again later.", provider),
        },
        503 => match lang {
            "vi" => format!("Lỗi 503: Dịch vụ {} đang quá tải hoặc bảo trì. Vui lòng thử lại sau.", provider),
            "ko" => format!("오류 503: {} 서비스가 과부하 상태이거나 점검 중입니다. 나중에 다시 시도해 주세요.", provider),
            "ja" => format!("エラー 503: {} サービスが過負荷またはメンテナンス中です。後で再試行してください。", provider),
            "zh" => format!("错误 503: {} 服务过载或维护中。请稍后再试。", provider),
            _ => format!("Error 503: {} service is overloaded or under maintenance. Please try again later.", provider),
        },
        504 => match lang {
            "vi" => format!("Lỗi 504: Hết thời gian chờ phản hồi từ {}. Vui lòng thử lại.", model_info),
            "ko" => format!("오류 504: {} 응답 시간 초과. 다시 시도해 주세요.", model_info),
            "ja" => format!("エラー 504: {} からの応答がタイムアウトしました。再試行してください。", model_info),
            "zh" => format!("错误 504: {} 响应超时。请重试。", model_info),
            _ => format!("Error 504: Gateway timeout from {}. Please try again.", model_info),
        },
        _ => match lang {
            "vi" => format!("Lỗi {}: Có lỗi xảy ra với {} (HTTP {}).", status_code, model_info, status_code),
            "ko" => format!("오류 {}: {}에서 오류가 발생했습니다 (HTTP {}).", status_code, model_info, status_code),
            "ja" => format!("エラー {}: {} でエラーが発生しました (HTTP {}).", status_code, model_info, status_code),
            "zh" => format!("错误 {}: {} 发生错误 (HTTP {}).", status_code, model_info, status_code),
            _ => format!("Error {}: An error occurred with {} (HTTP {}).", status_code, model_info, status_code),
        },
    }
}

pub fn is_retryable_error(error: &str) -> bool {
    // 1. Check for explicit Auth errors (Never retry)
    if error.contains("NO_API_KEY") || error.contains("INVALID_API_KEY") {
        return false;
    }

    // 2. Check HTTP status if present
    if let Some(code) = extract_http_status_code(error) {
        // 429: Rate Limit (Retry!)
        if code == 429 {
            return true;
        }
        // 5xx: Server Errors (Retry!)
        if code >= 500 && code <= 599 {
            return true;
        }
        return false;
    }

    // 3. Fallback text checks
    let lower_err = error.to_lowercase();
    if lower_err.contains("rate limit")
        || lower_err.contains("too many requests")
        || lower_err.contains("quota exceeded")
    {
        return true;
    }

    false
}
