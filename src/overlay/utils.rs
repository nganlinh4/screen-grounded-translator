use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn to_wstring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// --- CLIPBOARD SUPPORT ---
pub fn copy_to_clipboard(text: &str, hwnd: HWND) {
    unsafe {
        // Retry loop to handle temporary clipboard locks
        for attempt in 0..5 {
            if OpenClipboard(hwnd).as_bool() {
                EmptyClipboard();
                
                // Convert text to UTF-16
                let wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let mem_size = wide_text.len() * 2;
                
                // Allocate global memory
                if let Ok(h_mem) = GlobalAlloc(GMEM_MOVEABLE, mem_size) {
                    let ptr = GlobalLock(h_mem) as *mut u16;
                    std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr, wide_text.len());
                    GlobalUnlock(h_mem);
                    
                    // Set clipboard data (CF_UNICODETEXT = 13)
                    let h_mem_handle = HANDLE(h_mem.0);
                    let _ = SetClipboardData(13u32, h_mem_handle);
                }
                
                CloseClipboard();
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

// --- AUTO PASTE UTILS ---

/// Checks active window for caret OR keyboard focus and returns its HWND if found
pub fn get_target_window_for_paste() -> Option<HWND> {
    unsafe {
        let hwnd_foreground = GetForegroundWindow();
        if hwnd_foreground.0 == 0 { return None; }
        
        let thread_id = GetWindowThreadProcessId(hwnd_foreground, None);
        if thread_id == 0 { return None; }
        
        let mut gui_info = GUITHREADINFO::default();
        gui_info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        
        if GetGUIThreadInfo(thread_id, &mut gui_info).as_bool() {
            // Check legacy caret
            let has_caret = gui_info.hwndCaret.0 != 0;
            let blinking = (gui_info.flags & GUI_CARETBLINKING).0 != 0;
            
            // Check keyboard focus (Fix for Chrome/Electron/WPF)
            let has_focus = gui_info.hwndFocus.0 != 0;

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
        if IsWindow(hwnd_target).as_bool() {
            let cur_thread = GetCurrentThreadId();
            let target_thread = GetWindowThreadProcessId(hwnd_target, None);
            
            if cur_thread != target_thread {
                let _ = AttachThreadInput(cur_thread, target_thread, true);
                let _ = SetForegroundWindow(hwnd_target);
                // Important: Bring window to top so it receives input
                let _ = BringWindowToTop(hwnd_target);
                let _ = SetFocus(hwnd_target);
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
                        }
                    }
                };
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
             }
        };

        release_if_pressed(VK_MENU.0);    // Alt
        release_if_pressed(VK_SHIFT.0);   // Shift
        release_if_pressed(VK_LWIN.0);    // Win Left
        release_if_pressed(VK_RWIN.0);    // Win Right
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
                    }
                }
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

pub fn get_error_message(error: &str, lang: &str) -> String {
    match error {
        "NO_API_KEY" => {
            match lang {
                "vi" => "Bạn chưa nhập API key!".to_string(),
                _ => "You haven't entered an API key!".to_string(),
            }
        }
        "INVALID_API_KEY" => {
            match lang {
                "vi" => "API key không hợp lệ!".to_string(),
                _ => "Invalid API key!".to_string(),
            }
        }
        _ => {
            match lang {
                "vi" => format!("Lỗi: {}", error),
                _ => format!("Error: {}", error),
            }
        }
    }
}
