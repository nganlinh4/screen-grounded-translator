use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Once, Mutex};
use crate::APP;

struct TextSelectionState {
    hwnd: HWND,
    preset_idx: usize,
    is_selecting: bool,
    is_processing: bool,
    animation_offset: f32,
    current_alpha: i32,
    cached_bitmap: HBITMAP,
    cached_bits: *mut u32,
    cached_font: HFONT,
    cached_lang: Option<String>,
}
unsafe impl Send for TextSelectionState {}

static SELECTION_STATE: Mutex<TextSelectionState> = Mutex::new(TextSelectionState {
    hwnd: HWND(std::ptr::null_mut()),
    preset_idx: 0,
    is_selecting: false,
    is_processing: false,
    animation_offset: 0.0,
    current_alpha: 0,
    cached_bitmap: HBITMAP(std::ptr::null_mut()),
    cached_bits: std::ptr::null_mut(),
    cached_font: HFONT(std::ptr::null_mut()),
    cached_lang: None,
});

static REGISTER_TAG_CLASS: Once = Once::new();

lazy_static::lazy_static! {
    pub static ref TAG_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub fn is_active() -> bool {
    !SELECTION_STATE.lock().unwrap().hwnd.is_invalid()
}

/// Try to process already-selected text instantly.
/// Returns true if text was found and processing started (caller should NOT show selection tag).
/// Returns false if no text was selected (caller should show selection tag for manual selection).
pub fn try_instant_process(preset_idx: usize) -> bool {
    unsafe {
        // Step 1: Save current clipboard content (we'll restore if empty selection)
        let original_clipboard = get_clipboard_text();
        
        // Step 2: Clear clipboard and send Ctrl+C to copy current selection
        if OpenClipboard(Some(HWND::default())).is_ok() { 
            let _ = EmptyClipboard(); 
            let _ = CloseClipboard(); 
        }
        
        // Small delay to ensure clipboard is clear
        std::thread::sleep(std::time::Duration::from_millis(30));
        
        // Send Ctrl+C
        let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
            let input = INPUT { 
                r#type: INPUT_KEYBOARD, 
                Anonymous: INPUT_0 { 
                    ki: KEYBDINPUT { 
                        wVk: VIRTUAL_KEY(vk), 
                        dwFlags: flags, 
                        time: 0, 
                        dwExtraInfo: 0, 
                        wScan: 0 
                    } 
                } 
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        };
        
        send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0));
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(0x43, KEYBD_EVENT_FLAGS(0)); // 'C'
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(0x43, KEYEVENTF_KEYUP);
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);
        
        // Step 3: Wait for clipboard to update and check for text
        let mut clipboard_text = String::new();
        for _ in 0..6 { // Fewer retries since we need to be quick
            std::thread::sleep(std::time::Duration::from_millis(20));
            clipboard_text = get_clipboard_text();
            if !clipboard_text.is_empty() { break; }
        }
        
        // Step 4: Check if we got any text
        if clipboard_text.trim().is_empty() {
            // No text was selected - restore original clipboard if we had content
            if !original_clipboard.is_empty() {
                crate::overlay::utils::copy_to_clipboard(&original_clipboard, HWND::default());
            }
            return false; // Signal caller to show selection tag
        }
        
        // Step 5: Text found! Process it immediately
        process_selected_text(preset_idx, clipboard_text);
        true // Signal caller that we handled it
    }
}

/// Get text from clipboard (returns empty string if no text available)
unsafe fn get_clipboard_text() -> String {
    let mut result = String::new();
    if OpenClipboard(Some(HWND::default())).is_ok() {
        if let Ok(h_data) = GetClipboardData(13u32) { // CF_UNICODETEXT
            let h_global: HGLOBAL = std::mem::transmute(h_data);
            let ptr = GlobalLock(h_global);
            if !ptr.is_null() {
                let size = GlobalSize(h_global);
                let wide_slice = std::slice::from_raw_parts(ptr as *const u16, size / 2);
                if let Some(end) = wide_slice.iter().position(|&c| c == 0) { 
                    result = String::from_utf16_lossy(&wide_slice[..end]); 
                }
            }
            let _ = GlobalUnlock(h_global);
        }
        let _ = CloseClipboard();
    }
    result
}

/// Process selected text with the given preset (shared logic for both instant and manual selection)
fn process_selected_text(preset_idx: usize, clipboard_text: String) {
    unsafe {
        // Check if this is a MASTER preset
        let (is_master, _original_mode) = {
            let app = APP.lock().unwrap();
            let p = &app.config.presets[preset_idx];
            (p.is_master, p.text_input_mode.clone())
        };
        
        let final_preset_idx = if is_master {
            // Get cursor position for wheel center
            let mut cursor_pos = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            
            // Show preset wheel
            let selected = super::preset_wheel::show_preset_wheel("text", Some("select"), cursor_pos);
            
            if let Some(idx) = selected {
                idx
            } else {
                // User dismissed wheel - cancel operation
                return;
            }
        } else {
            preset_idx
        };
        
        // Process with the selected preset
        let (config, mut preset, screen_w, screen_h) = {
            let mut app = APP.lock().unwrap();
            // CRITICAL: Update active_preset_idx so auto_paste logic works!
            app.config.active_preset_idx = final_preset_idx;
            (
                app.config.clone(),
                app.config.presets[final_preset_idx].clone(),
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN)
            )
        };
        
        // CRITICAL FIX: Force text_input_mode to "select" so the text is processed
        // directly, not re-opened in a text input modal
        preset.text_input_mode = "select".to_string();
        
        let center_rect = RECT { 
            left: (screen_w - 700) / 2, 
            top: (screen_h - 300) / 2, 
            right: (screen_w + 700) / 2, 
            bottom: (screen_h + 300) / 2 
        };
        // Get localized preset name and hotkey for the text input header
        let localized_name = crate::gui::settings_ui::get_localized_preset_name(&preset.id, &config.ui_language);
        let cancel_hotkey = preset.hotkeys.first().map(|h| h.name.clone()).unwrap_or_default();
        
        super::process::start_text_processing(clipboard_text, center_rect, config, preset, localized_name, cancel_hotkey);
    }
}

pub fn cancel_selection() {
    TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
    let hwnd = SELECTION_STATE.lock().unwrap().hwnd;
    unsafe {
        if !hwnd.is_invalid() {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn show_text_selection_tag(preset_idx: usize) {
    unsafe {
        // Scope 1: Check and Init
        {
            let mut state = SELECTION_STATE.lock().unwrap();
            if !state.hwnd.is_invalid() { return; } 

            state.preset_idx = preset_idx;
            state.is_selecting = false;
            state.is_processing = false;
            state.animation_offset = 0.0;
            state.current_alpha = 0;
            TAG_ABORT_SIGNAL.store(false, Ordering::SeqCst);
            
            // Cleanup old cache
            if !state.cached_bitmap.is_invalid() { let _ = DeleteObject(state.cached_bitmap.into()); state.cached_bitmap = HBITMAP::default(); }
            if !state.cached_font.is_invalid() { let _ = DeleteObject(state.cached_font.into()); state.cached_font = HFONT(std::ptr::null_mut()); }
            state.cached_bits = std::ptr::null_mut();
        }

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextTag");

        REGISTER_TAG_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(tag_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassW(&wc);
        });

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, 
            class_name, w!("SGT Tag"), WS_POPUP, -1000, -1000, 200, 50, None, None, Some(instance.into()), None
        ).unwrap_or_default();
        SELECTION_STATE.lock().unwrap().hwnd = hwnd;
        SetTimer(Some(hwnd), 1, 16, None); 
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let mut msg = MSG::default(); while GetMessageW(&mut msg, None, 0, 0).into() { let _ = TranslateMessage(&msg); DispatchMessageW(&msg); if msg.message == WM_QUIT { break; } }
        
        // Cleanup cache on exit
        {
            let mut state = SELECTION_STATE.lock().unwrap();
            if !state.cached_bitmap.is_invalid() { let _ = DeleteObject(state.cached_bitmap.into()); state.cached_bitmap = HBITMAP::default(); }
            if !state.cached_font.is_invalid() { let _ = DeleteObject(state.cached_font.into()); state.cached_font = HFONT(std::ptr::null_mut()); }
            state.cached_bits = std::ptr::null_mut();
            state.hwnd = HWND::default();
        }
    }
}

unsafe extern "system" fn tag_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if TAG_ABORT_SIGNAL.load(Ordering::SeqCst) { let _ = DestroyWindow(hwnd); PostQuitMessage(0); return LRESULT(0); }
            let lbutton_down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0;
            
            let mut should_spawn_thread = false;
            let mut preset_idx_for_thread = 0;
            let (is_selecting, current_alpha) = {
                let mut state = SELECTION_STATE.lock().unwrap();
                
                if !state.is_selecting && lbutton_down { 
                    state.is_selecting = true; 
                } else if state.is_selecting && !lbutton_down && !state.is_processing {
                    state.is_processing = true;
                    should_spawn_thread = true;
                    preset_idx_for_thread = state.preset_idx;
                }
                
                let mut pt = POINT::default(); let _ = GetCursorPos(&mut pt);
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), pt.x - 30, pt.y - 60, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                
                if state.is_selecting { state.animation_offset -= 15.0; } else { state.animation_offset += 5.0; }
                if state.animation_offset > 3600.0 { state.animation_offset -= 3600.0; } 
                if state.animation_offset < -3600.0 { state.animation_offset += 3600.0; }
                
                if state.is_processing || crate::overlay::preset_wheel::is_wheel_active() {
                    if state.current_alpha > 0 { 
                        state.current_alpha -= 50; 
                        if state.current_alpha < 0 { state.current_alpha = 0; } 
                    }
                } else if state.current_alpha < 255 { 
                    state.current_alpha += 25; 
                    if state.current_alpha > 255 { state.current_alpha = 255; } 
                }
                
                (state.is_selecting, state.current_alpha as u8)
            };

            if should_spawn_thread {
                let hwnd_val = hwnd.0 as usize;
                std::thread::spawn(move || {
                    let hwnd_copy = HWND(hwnd_val as *mut std::ffi::c_void);
                    unsafe {
                        if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) { return; }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        
                        if OpenClipboard(Some(HWND::default())).is_ok() { let _ = EmptyClipboard(); let _ = CloseClipboard(); }

                        let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
                            let input = INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), dwFlags: flags, time: 0, dwExtraInfo: 0, wScan: 0 } } };
                            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                        };

                        send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0)); 
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        send_input_event(0x43, KEYBD_EVENT_FLAGS(0)); 
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        send_input_event(0x43, KEYEVENTF_KEYUP);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);
                        
                        let mut clipboard_text = String::new();
                        for _ in 0..10 {
                            if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) { return; }
                            std::thread::sleep(std::time::Duration::from_millis(25));
                            clipboard_text = get_clipboard_text();
                            if !clipboard_text.is_empty() { break; }
                        }

                        if !clipboard_text.trim().is_empty() && !TAG_ABORT_SIGNAL.load(Ordering::Relaxed) {
                            process_selected_text(preset_idx_for_thread, clipboard_text);
                            let _ = PostMessageW(Some(hwnd_copy), WM_CLOSE, WPARAM(0), LPARAM(0));
                        } else {
                            // Reset state logic - scope the lock
                            let mut state = SELECTION_STATE.lock().unwrap();
                            state.is_selecting = false;
                            state.is_processing = false;
                        }
                    }
                });
                return LRESULT(0);
            }
            
            paint_tag_window(hwnd, 200, 40, current_alpha, is_selecting);
            LRESULT(0)
        }
        WM_CLOSE => { 
            TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst); 
            let _ = DestroyWindow(hwnd); 
            PostQuitMessage(0); 
            LRESULT(0) 
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_tag_window(hwnd: HWND, width: i32, height: i32, alpha: u8, is_selecting: bool) {
    if alpha == 0 { return; }
    
    let screen_dc = GetDC(None); 
    let mem_dc = CreateCompatibleDC(Some(screen_dc));
    
    let mut state = SELECTION_STATE.lock().unwrap();
    let animation_offset = state.animation_offset;
    
    // Cached lang check
    if state.cached_lang.is_none() {
         let app = APP.lock().unwrap();
         state.cached_lang = Some(app.config.ui_language.clone());
    }
    
    if state.cached_bitmap.is_invalid() {
        let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default() }, ..Default::default() };
        let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(bmp) = CreateDIBSection(Some(screen_dc), &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0) {
            state.cached_bitmap = bmp;
            state.cached_bits = p_bits as *mut u32;
        }
    }
    
    if state.cached_font.is_invalid() {
       state.cached_font = CreateFontW(15, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, FONT_CHARSET(DEFAULT_CHARSET.0 as u8), FONT_OUTPUT_PRECISION(OUT_DEFAULT_PRECIS.0 as u8), FONT_CLIP_PRECISION(CLIP_DEFAULT_PRECIS.0 as u8), FONT_QUALITY(CLEARTYPE_QUALITY.0 as u8), std::mem::transmute((VARIABLE_PITCH.0 | FF_SWISS.0) as u32), w!("Segoe UI"));
    }
    
    let old_bitmap = SelectObject(mem_dc, state.cached_bitmap.into());
    
    if !state.cached_bits.is_null() {
        let pixels = std::slice::from_raw_parts_mut(state.cached_bits, (width * height) as usize);
        let bx = width as f32 / 2.0; 
        let by = height as f32 / 2.0; 
        let time_rad = animation_offset.to_radians();
        
        let inner_margin = 20.0;
        let glow_base = if is_selecting { 8.0 } else { 5.0 };
        
        for y in 0..height {
            let py = y as f32 - by;
            let is_y_interior = y > inner_margin as i32 && y < height - inner_margin as i32;
            
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let px = x as f32 - bx;
                
                let is_x_interior = x > inner_margin as i32 && x < width - inner_margin as i32;
                if is_y_interior && is_x_interior {
                    pixels[idx] = 0xD9101010; 
                    continue;
                }
                
                let d = crate::overlay::paint_utils::sd_rounded_box(px, py, bx - 6.0, by - 6.0, 14.0);
                let mut final_col = 0x000000; 
                let mut final_alpha = 0.0f32;
                let aa_half = 0.75;
                
                if d < -aa_half { 
                    final_alpha = 0.85; 
                    final_col = 0x00101010; 
                } else if d < aa_half {
                    let t = (d + aa_half) / (aa_half * 2.0); 
                    let blend = t * t * (3.0 - 2.0 * t);
                    let angle = py.atan2(px); 
                    let noise = if is_selecting { (angle * 10.0 - time_rad * 8.0).sin() * 0.5 } else { (angle * 2.0 + time_rad * 3.0).sin() * 0.2 };
                    let glow_width = glow_base + (noise * 3.0);
                    let glow_t = (d.max(0.0) / glow_width).clamp(0.0, 1.0); 
                    let glow_intensity = (1.0 - glow_t).powi(2);
                    let hue = (angle.to_degrees() + animation_offset * 2.0).rem_euclid(360.0); 
                    let glow_rgb = crate::overlay::paint_utils::hsv_to_rgb(hue, 0.9, 1.0);
                    let fill_alpha = 0.85 * (1.0 - blend); 
                    let glow_alpha = glow_intensity * blend;
                    final_alpha = fill_alpha + glow_alpha;
                    let glow_r = ((glow_rgb >> 16) & 0xFF) as f32 * blend; 
                    let glow_g = ((glow_rgb >> 8) & 0xFF) as f32 * blend; 
                    let glow_b = (glow_rgb & 0xFF) as f32 * blend;
                    if final_alpha > 0.001 {
                        let r = (glow_r / final_alpha + 0x10 as f32 * (1.0 - blend)).min(255.0) as u32; 
                        let g = (glow_g / final_alpha + 0x10 as f32 * (1.0 - blend)).min(255.0) as u32; 
                        let b = (glow_b / final_alpha + 0x10 as f32 * (1.0 - blend)).min(255.0) as u32;
                        final_col = (r << 16) | (g << 8) | b;
                    }
                } else {
                    let angle = py.atan2(px); 
                    let noise = if is_selecting { (angle * 10.0 - time_rad * 8.0).sin() * 0.5 } else { (angle * 2.0 + time_rad * 3.0).sin() * 0.2 };
                    let glow_width = glow_base + (noise * 3.0);
                    let t = (d / glow_width).clamp(0.0, 1.0); 
                    let glow_intensity = (1.0 - t).powi(2);
                    if glow_intensity > 0.01 { 
                        let hue = (angle.to_degrees() + animation_offset * 2.0).rem_euclid(360.0); 
                        final_col = crate::overlay::paint_utils::hsv_to_rgb(hue, 0.9, 1.0); 
                        final_alpha = glow_intensity; 
                    }
                }
                let a = (final_alpha * 255.0) as u32; 
                let r = ((final_col >> 16) & 0xFF) * a / 255; 
                let g = ((final_col >> 8) & 0xFF) * a / 255; 
                let b = (final_col & 0xFF) * a / 255;
                pixels[idx] = (a << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }
    
    SetBkMode(mem_dc, TRANSPARENT); 
    SetTextColor(mem_dc, COLORREF(0x00FFFFFF));
    let old_font = SelectObject(mem_dc, state.cached_font.into());
    
    let text = if is_selecting { 
        match state.cached_lang.as_ref().unwrap().as_str() { "vi" => "Thả chuột để xử lý", "ko" => "처리를 위해 마우스를 놓으세요", _ => "Release to process" } 
    } else { 
        match state.cached_lang.as_ref().unwrap().as_str() { "vi" => "Bôi đen văn bản...", "ko" => "텍스트 선택...", _ => "Select text..." } 
    };
    let mut text_w = crate::overlay::utils::to_wstring(text); 
    let mut tr = RECT { left: 0, top: 0, right: width, bottom: height };
    DrawTextW(mem_dc, &mut text_w, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    
    if !state.cached_bits.is_null() { 
        let _ = GdiFlush(); 
        let pxs = std::slice::from_raw_parts_mut(state.cached_bits, (width * height) as usize); 
        for p in pxs.iter_mut() { 
            let val = *p; 
            let a = (val >> 24) & 0xFF; 
            let r = (val >> 16) & 0xFF; 
            let g = (val >> 8) & 0xFF; 
            let b = val & 0xFF; 
            let max_c = r.max(g).max(b); 
            if max_c > a { *p = (max_c << 24) | (r << 16) | (g << 8) | b; } 
        } 
    }
    
    let pt_src = POINT { x: 0, y: 0 }; 
    let size = SIZE { cx: width, cy: height };
    let mut bl = BLENDFUNCTION::default(); 
    bl.BlendOp = AC_SRC_OVER as u8; 
    bl.SourceConstantAlpha = alpha; 
    bl.AlphaFormat = AC_SRC_ALPHA as u8;
    let _ = UpdateLayeredWindow(hwnd, None, None, Some(&size), Some(mem_dc), Some(&pt_src), COLORREF(0), Some(&bl), ULW_ALPHA);
    
    let _ = SelectObject(mem_dc, old_font.into()); 
    let _ = SelectObject(mem_dc, old_bitmap.into()); 
    let _ = DeleteDC(mem_dc); 
    ReleaseDC(None, screen_dc);
}
