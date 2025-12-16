use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Once};
use crate::APP;

static mut TAG_HWND: HWND = HWND(0);
static mut CURRENT_PRESET_IDX: usize = 0;
static mut IS_SELECTING: bool = false;
static mut IS_PROCESSING: bool = false; // Prevents re-entry during clipboard operations
static mut ANIMATION_OFFSET: f32 = 0.0;
static mut CURRENT_ALPHA: i32 = 0;

// PERFORMANCE: Cache bitmap and font to avoid per-frame allocation
static mut CACHED_BITMAP: HBITMAP = HBITMAP(0);
static mut CACHED_BITS: *mut u32 = std::ptr::null_mut();
static mut CACHED_FONT: HFONT = HFONT(0);
static mut CACHED_LANG: Option<String> = None;

static REGISTER_TAG_CLASS: Once = Once::new();

lazy_static::lazy_static! {
    pub static ref TAG_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub fn is_active() -> bool {
    unsafe { TAG_HWND.0 != 0 }
}

pub fn cancel_selection() {
    TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
    unsafe {
        if TAG_HWND.0 != 0 {
            PostMessageW(TAG_HWND, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn show_text_selection_tag(preset_idx: usize) {
    unsafe {
        if TAG_HWND.0 != 0 { return; } // Prevent double open

        CURRENT_PRESET_IDX = preset_idx;
        IS_SELECTING = false;
        IS_PROCESSING = false;
        ANIMATION_OFFSET = 0.0;
        CURRENT_ALPHA = 0;
        TAG_ABORT_SIGNAL.store(false, Ordering::SeqCst);
        
        // Cleanup old cache
        if CACHED_BITMAP.0 != 0 { DeleteObject(CACHED_BITMAP); CACHED_BITMAP = HBITMAP(0); }
        if CACHED_FONT.0 != 0 { DeleteObject(CACHED_FONT); CACHED_FONT = HFONT(0); }
        CACHED_BITS = std::ptr::null_mut();

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextTag");

        REGISTER_TAG_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(tag_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassW(&wc);
        });

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, 
            class_name, w!("SGT Tag"), WS_POPUP, -1000, -1000, 200, 50, None, None, instance, None
        );
        TAG_HWND = hwnd;
        SetTimer(hwnd, 1, 16, None); 
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let mut msg = MSG::default(); while GetMessageW(&mut msg, None, 0, 0).into() { TranslateMessage(&msg); DispatchMessageW(&msg); if msg.message == WM_QUIT { break; } }
        
        // Cleanup cache on exit
        if CACHED_BITMAP.0 != 0 { DeleteObject(CACHED_BITMAP); CACHED_BITMAP = HBITMAP(0); }
        if CACHED_FONT.0 != 0 { DeleteObject(CACHED_FONT); CACHED_FONT = HFONT(0); }
        CACHED_BITS = std::ptr::null_mut();
        TAG_HWND = HWND(0);
    }
}

unsafe extern "system" fn tag_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if TAG_ABORT_SIGNAL.load(Ordering::SeqCst) { DestroyWindow(hwnd); PostQuitMessage(0); return LRESULT(0); }
            let lbutton_down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0;
            if !IS_SELECTING && lbutton_down { IS_SELECTING = true; }
            else if IS_SELECTING && !lbutton_down && !IS_PROCESSING {
                // Set processing flag to prevent re-entry
                IS_PROCESSING = true;
                
                let preset_idx = CURRENT_PRESET_IDX;
                let hwnd_copy = hwnd;
                
                std::thread::spawn(move || {
                    // Check abort before heavy lifting
                    if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) { return; }
                    
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    
                    if OpenClipboard(HWND(0)).as_bool() { EmptyClipboard(); CloseClipboard(); }

                    let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
                        let input = INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), dwFlags: flags, time: 0, dwExtraInfo: 0, wScan: 0 } } };
                        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                    };

                    send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0)); 
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    send_input_event(0x43, KEYBD_EVENT_FLAGS(0)); // 'C'
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    send_input_event(0x43, KEYEVENTF_KEYUP);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);
                    
                    let mut clipboard_text = String::new();
                    for _ in 0..10 {
                        if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) { return; }
                        std::thread::sleep(std::time::Duration::from_millis(25));
                        if OpenClipboard(HWND(0)).as_bool() {
                            if let Ok(h_data) = GetClipboardData(13u32) {
                                let h_global: HGLOBAL = std::mem::transmute(h_data);
                                let ptr = GlobalLock(h_global);
                                if !ptr.is_null() {
                                    let size = GlobalSize(h_global);
                                    let wide_slice = std::slice::from_raw_parts(ptr as *const u16, size / 2);
                                    if let Some(end) = wide_slice.iter().position(|&c| c == 0) { clipboard_text = String::from_utf16_lossy(&wide_slice[..end]); }
                                }
                                GlobalUnlock(h_global);
                            }
                            CloseClipboard();
                        }
                        if !clipboard_text.is_empty() { break; }
                    }

                    if !clipboard_text.trim().is_empty() && !TAG_ABORT_SIGNAL.load(Ordering::Relaxed) {
                        // Text found - check if this is a MASTER preset
                        let (is_master, original_preset_type) = {
                            let app = APP.lock().unwrap();
                            let p = &app.config.presets[preset_idx];
                            (p.is_master, p.text_input_mode.clone())
                        };
                        
                        let final_preset_idx = if is_master {
                            // Get cursor position for wheel center
                            let mut cursor_pos = POINT { x: 0, y: 0 };
                            GetCursorPos(&mut cursor_pos);
                            
                            // Show preset wheel on main thread
                            let selected = super::preset_wheel::show_preset_wheel("text", Some("select"), cursor_pos);
                            
                            if let Some(idx) = selected {
                                idx
                            } else {
                                // User dismissed wheel - cancel operation
                                PostMessageW(hwnd_copy, WM_CLOSE, WPARAM(0), LPARAM(0));
                                return;
                            }
                        } else {
                            preset_idx
                        };
                        
                        // Process with the selected preset
                        let (config, mut preset, screen_w, screen_h) = {
                            let app = APP.lock().unwrap(); 
                            (
                                app.config.clone(),
                                app.config.presets[final_preset_idx].clone(),
                                GetSystemMetrics(SM_CXSCREEN),
                                GetSystemMetrics(SM_CYSCREEN)
                            )
                        }; 

                        // CRITICAL FIX: Force text_input_mode to "select" so the text is processed
                        // directly, not re-opened in a text input modal.
                        // This is the key fix for "Bôi MASTER" - we already have the text!
                        preset.text_input_mode = "select".to_string();

                        let center_rect = RECT { left: (screen_w - 700) / 2, top: (screen_h - 300) / 2, right: (screen_w + 700) / 2, bottom: (screen_h + 300) / 2 };
                        super::process::start_text_processing(clipboard_text, center_rect, config, preset, String::new(), String::new());

                        
                        // Now destroy the window
                        PostMessageW(hwnd_copy, WM_CLOSE, WPARAM(0), LPARAM(0));
                    } else {
                        // No text selected - reset state to allow user to try again
                        IS_SELECTING = false;
                        IS_PROCESSING = false;
                    }
                });
                
                return LRESULT(0);
            }
            let mut pt = POINT::default(); GetCursorPos(&mut pt);
            SetWindowPos(hwnd, HWND_TOPMOST, pt.x - 30, pt.y - 60, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
            if IS_SELECTING { ANIMATION_OFFSET -= 15.0; } else { ANIMATION_OFFSET += 5.0; }
            if ANIMATION_OFFSET > 3600.0 { ANIMATION_OFFSET -= 3600.0; } if ANIMATION_OFFSET < -3600.0 { ANIMATION_OFFSET += 3600.0; }
            if CURRENT_ALPHA < 255 { CURRENT_ALPHA += 25; if CURRENT_ALPHA > 255 { CURRENT_ALPHA = 255; } }
            paint_tag_window(hwnd, 200, 40, CURRENT_ALPHA as u8, IS_SELECTING); LRESULT(0)
        }
        WM_CLOSE => { TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst); DestroyWindow(hwnd); PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_tag_window(hwnd: HWND, width: i32, height: i32, alpha: u8, is_selecting: bool) {
    let screen_dc = GetDC(None); 
    let mem_dc = CreateCompatibleDC(screen_dc);
    
    // OPTIMIZATION 1: Reuse bitmap if already created
    if CACHED_BITMAP.0 == 0 {
        let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default() }, ..Default::default() };
        let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(bmp) = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0) {
            CACHED_BITMAP = bmp;
            CACHED_BITS = p_bits as *mut u32;
        }
    }
    
    // OPTIMIZATION 2: Reuse font
    if CACHED_FONT.0 == 0 {
        CACHED_FONT = CreateFontW(15, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
    }
    
    let old_bitmap = SelectObject(mem_dc, CACHED_BITMAP);
    
    if !CACHED_BITS.is_null() {
        let pixels = std::slice::from_raw_parts_mut(CACHED_BITS, (width * height) as usize);
        let bx = width as f32 / 2.0; 
        let by = height as f32 / 2.0; 
        let time_rad = ANIMATION_OFFSET.to_radians();
        
        // OPTIMIZATION 3: Pre-calculate constants
        let inner_margin = 20.0; // Larger margin to preserve rounded corners
        let glow_base = if is_selecting { 8.0 } else { 5.0 };
        
        for y in 0..height {
            let py = y as f32 - by;
            let is_y_interior = y > inner_margin as i32 && y < height - inner_margin as i32;
            
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let px = x as f32 - bx;
                
                // OPTIMIZATION 4: Fast path for interior pixels (just solid fill)
                let is_x_interior = x > inner_margin as i32 && x < width - inner_margin as i32;
                if is_y_interior && is_x_interior {
                    // Interior: solid dark fill (0.85 alpha, 0x101010 color)
                    pixels[idx] = 0xD9101010; // Pre-multiplied: (217 << 24) | (16 << 16) | (16 << 8) | 16
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
                    let hue = (angle.to_degrees() + ANIMATION_OFFSET * 2.0).rem_euclid(360.0); 
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
                        let hue = (angle.to_degrees() + ANIMATION_OFFSET * 2.0).rem_euclid(360.0); 
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
    let old_font = SelectObject(mem_dc, CACHED_FONT);
    
    // OPTIMIZATION 5: Cache language check (avoid mutex lock every frame if unchanged)
    let app_lang = if CACHED_LANG.is_none() {
        let lang = APP.lock().unwrap().config.ui_language.clone();
        CACHED_LANG = Some(lang.clone());
        lang
    } else {
        CACHED_LANG.clone().unwrap()
    };
    
    let text = if is_selecting { 
        match app_lang.as_str() { "vi" => "Thả chuột để xử lý", "ko" => "처리를 위해 마우스를 놓으세요", _ => "Release to process" } 
    } else { 
        match app_lang.as_str() { "vi" => "Bôi đen văn bản...", "ko" => "텍스트 선택...", _ => "Select text..." } 
    };
    let mut text_w = crate::overlay::utils::to_wstring(text); 
    let mut tr = RECT { left: 0, top: 0, right: width, bottom: height };
    DrawTextW(mem_dc, &mut text_w, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    
    // Fix text alpha
    if !CACHED_BITS.is_null() { 
        GdiFlush(); 
        let pxs = std::slice::from_raw_parts_mut(CACHED_BITS, (width * height) as usize); 
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
    UpdateLayeredWindow(hwnd, HDC(0), None, Some(&size), mem_dc, Some(&pt_src), COLORREF(0), Some(&bl), ULW_ALPHA);
    
    SelectObject(mem_dc, old_font); 
    SelectObject(mem_dc, old_bitmap); 
    DeleteDC(mem_dc); 
    ReleaseDC(None, screen_dc);
    // Note: Don't delete CACHED_BITMAP or CACHED_FONT here - they're reused
}
