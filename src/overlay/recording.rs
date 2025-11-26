use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, Once};
use crate::APP;

static mut RECORDING_HWND: HWND = HWND(0);
static mut IS_RECORDING: bool = false;
static mut IS_PAUSED: bool = false;
static mut ANIMATION_OFFSET: f32 = 0.0;
static mut CURRENT_PRESET_IDX: usize = 0;
static mut CURRENT_ALPHA: i32 = 0; // For fade-in

// --- UI CONSTANTS ---
const UI_WIDTH: i32 = 350;   // More compact width
const UI_HEIGHT: i32 = 80;   // Reduced height
const BTN_OFFSET: i32 = 40;  // Distance from edge to icon center
const HIT_RADIUS: i32 = 25;  // Clickable radius around buttons

// Shared flag for the audio thread
lazy_static::lazy_static! {
    pub static ref AUDIO_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_PAUSE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

// OPTIMIZATION: Thread-safe one-time window class registration
static REGISTER_RECORDING_CLASS: Once = Once::new();

pub fn is_recording_overlay_active() -> bool {
    unsafe { IS_RECORDING && RECORDING_HWND.0 != 0 }
}

pub fn stop_recording_and_submit() {
    unsafe {
        if IS_RECORDING && RECORDING_HWND.0 != 0 {
            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
            // Force immediate update to show "Processing"
            PostMessageW(RECORDING_HWND, WM_TIMER, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn show_recording_overlay(preset_idx: usize) {
    unsafe {
        if IS_RECORDING { return; }
        
        let preset = APP.lock().unwrap().config.presets[preset_idx].clone();
        
        IS_RECORDING = true;
        IS_PAUSED = false;
        CURRENT_PRESET_IDX = preset_idx;
        ANIMATION_OFFSET = 0.0;
        CURRENT_ALPHA = 0; // Start invisible
        AUDIO_STOP_SIGNAL.store(false, Ordering::SeqCst);
        AUDIO_PAUSE_SIGNAL.store(false, Ordering::SeqCst);

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("RecordingOverlay");

        // OPTIMIZATION: Register class only once, thread-safely
        REGISTER_RECORDING_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(recording_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap(); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassW(&wc);
        });

        let screen_x = GetSystemMetrics(SM_CXSCREEN);
        let screen_y = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_x - UI_WIDTH) / 2;
        let y = (screen_y - UI_HEIGHT) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("SGT Recording"),
            WS_POPUP,
            x, y, UI_WIDTH, UI_HEIGHT,
            None, None, instance, None
        );

        RECORDING_HWND = hwnd;
        
        SetTimer(hwnd, 1, 16, None); 

        if !preset.hide_recording_ui {
            // Initially 0 alpha, will fade in via timer
            paint_layered_window(hwnd, UI_WIDTH, UI_HEIGHT, 0);
            ShowWindow(hwnd, SW_SHOW);
        }

        std::thread::spawn(move || {
            crate::api::record_audio_and_transcribe(preset, AUDIO_STOP_SIGNAL.clone(), AUDIO_PAUSE_SIGNAL.clone(), hwnd);
        });

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_QUIT { break; }
        }

        IS_RECORDING = false;
        RECORDING_HWND = HWND(0);
    }
}

unsafe fn paint_layered_window(hwnd: HWND, width: i32, height: i32, alpha: u8) {
    let screen_dc = GetDC(None);
    
    let bmi = windows::Win32::Graphics::Gdi::BITMAPINFO {
        bmiHeader: windows::Win32::Graphics::Gdi::BITMAPINFOHEADER {
            biSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: windows::Win32::Graphics::Gdi::BI_RGB.0 as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    
    let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(screen_dc, &bmi, windows::Win32::Graphics::Gdi::DIB_RGB_COLORS, &mut p_bits, None, 0).unwrap();
    
    let mem_dc = CreateCompatibleDC(screen_dc);
    let old_bitmap = SelectObject(mem_dc, bitmap);

    let is_waiting = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
    let should_animate = !IS_PAUSED || is_waiting;
    
    if !p_bits.is_null() {
        let pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);
        
        let bx = (width as f32) / 2.0;
        let by = (height as f32) / 2.0;
        let center_x = bx;
        let center_y = by;
        
        let time_rad = ANIMATION_OFFSET.to_radians();
        
        // 1. Draw Background (SDF)
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let px = (x as f32) - center_x;
                let py = (y as f32) - center_y;
                
                // Rounded Box SDF
                let d = super::paint_utils::sd_rounded_box(px, py, bx - 2.0, by - 2.0, 16.0);
                
                let mut final_col = 0x000000;
                let mut final_alpha = 0.0f32;

                if should_animate {
                    if d <= 0.0 {
                         final_alpha = 0.40; 
                         final_col = 0x00050505;
                    } else {
                        let angle = py.atan2(px);
                        
                        // Modified Glow Logic based on Processing State
                        let noise = if is_waiting {
                            // "Inner random reach" - high speed, high frequency, spiky
                            (angle * 10.0 - time_rad * 8.0).sin() * 0.5
                        } else {
                            // Smooth breathing
                            (angle * 2.0 + time_rad * 3.0).sin() * 0.2
                        };
                        
                        let glow_width = if is_waiting { 14.0 } else { 8.0 } + (noise * 5.0);
                        
                        let t = (d / glow_width).clamp(0.0, 1.0);
                        let glow_intensity = (1.0 - t).powi(2);
                        
                        if glow_intensity > 0.01 {
                            let hue_offset = if is_waiting { ANIMATION_OFFSET * 4.0 } else { ANIMATION_OFFSET * 2.0 };
                            let hue = (angle.to_degrees() + hue_offset) % 360.0;
                            // More vibrant during processing
                            let sat = if is_waiting { 1.0 } else { 0.85 };
                            let rgb = super::paint_utils::hsv_to_rgb(hue, sat, 1.0);
                            final_col = rgb;
                            final_alpha = glow_intensity;
                        }
                    }
                } else {
                     if d <= 0.0 {
                        final_alpha = 0.40;
                        final_col = 0x00050505;
                     } else if d < 2.0 {
                        final_alpha = 0.8;
                        final_col = 0x00AAAAAA;
                     }
                }

                let a = (final_alpha * 255.0) as u32;
                let r = ((final_col >> 16) & 0xFF) * a / 255;
                let g = ((final_col >> 8) & 0xFF) * a / 255;
                let b = (final_col & 0xFF) * a / 255;
                
                pixels[idx] = (a << 24) | (r << 16) | (g << 8) | b;
            }
        }

        // 2. Draw Icons directly to pixels (Skip if processing for cleaner look?)
        // Let's keep them but maybe dim them? No, keep standard behavior.
        if !is_waiting {
            let white_pixel = 0xFFFFFFFF;

            // -- PAUSE / PLAY BUTTON (Left) --
            let p_cx = BTN_OFFSET; 
            let p_cy = height / 2;

            if IS_PAUSED {
            // Draw Play Triangle
            for y in (p_cy - 12)..(p_cy + 12) {
                for x in (p_cx - 8)..(p_cx + 12) {
                    if x >= 0 && x < width && y >= 0 && y < height {
                        let dx = x - p_cx;
                        let dy = y - p_cy;
                        if dx >= -6 && dx <= 10 {
                            let max_y = (10.0 - dx as f32) * 0.625;
                            if (dy as f32).abs() <= max_y + 0.8 {
                                pixels[(y * width + x) as usize] = white_pixel;
                            }
                        }
                    }
                }
            }
        } else {
            // Draw Pause Bars (||)
            for y in (p_cy - 10)..=(p_cy + 10) {
                for x in (p_cx - 8)..=(p_cx + 8) {
                    if x > p_cx - 2 && x < p_cx + 2 { continue; } // Gap
                    if x >= 0 && x < width && y >= 0 && y < height {
                        pixels[(y * width + x) as usize] = white_pixel;
                    }
                }
            }
        }

        // -- CLOSE BUTTON (X) (Right) --
         let c_cx = width - BTN_OFFSET;
         let c_cy = height / 2;
         let thickness = 2.0;
         
         for y in (c_cy - 10)..(c_cy + 10) {
             for x in (c_cx - 10)..(c_cx + 10) {
                 if x >= 0 && x < width && y >= 0 && y < height {
                     let dx = (x - c_cx) as f32;
                     let dy = (y - c_cy) as f32;
                     let dist1 = (dx - dy).abs() * 0.7071;
                     let dist2 = (dx + dy).abs() * 0.7071;
                     if dist1 < thickness || dist2 < thickness {
                          pixels[(y * width + x) as usize] = white_pixel;
                     }
                 }
             }
         }
        }
        }

    SetBkMode(mem_dc, TRANSPARENT);
    SetTextColor(mem_dc, COLORREF(0x00FFFFFF));

    // --- MAIN STATUS TEXT ---
    // Moved up significantly to be optically centered in top half
    let hfont_main = CreateFontW(19, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
    let old_font = SelectObject(mem_dc, hfont_main);

    let src_text = if is_waiting {
        "Đang xử lý..."
    } else {
        if CURRENT_PRESET_IDX < APP.lock().unwrap().config.presets.len() {
             let p = &APP.lock().unwrap().config.presets[CURRENT_PRESET_IDX];
             if IS_PAUSED { "Tạm dừng" } 
             else if p.audio_source == "device" { "Ghi âm máy..." } 
             else { "Ghi âm mic..." }
        } else { "Recording..." }
    };

    let mut text_w = crate::overlay::utils::to_wstring(src_text);
    let mut tr = RECT { left: 0, top: 0, right: width, bottom: 45 };
    DrawTextW(mem_dc, &mut text_w, &mut tr, DT_CENTER | DT_BOTTOM | DT_SINGLELINE);

    SelectObject(mem_dc, old_font);
    DeleteObject(hfont_main);

    // Only show sub-text if not processing
    if !is_waiting {
        let hfont_sub = CreateFontW(14, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        SelectObject(mem_dc, hfont_sub);
        SetTextColor(mem_dc, COLORREF(0x00DDDDDD)); 

        let sub_text = "Bấm hotkey lần nữa để xử lý âm thanh";
        let mut sub_text_w = crate::overlay::utils::to_wstring(sub_text);
        let mut tr_sub = RECT { left: 0, top: 47, right: width, bottom: height };
        DrawTextW(mem_dc, &mut sub_text_w, &mut tr_sub, DT_CENTER | DT_TOP | DT_SINGLELINE);

        SelectObject(mem_dc, old_font);
        DeleteObject(hfont_sub);
    }

    let pt_src = POINT { x: 0, y: 0 };
    let size = SIZE { cx: width, cy: height };
    let mut blend = BLENDFUNCTION::default();
    blend.BlendOp = AC_SRC_OVER as u8;
    blend.SourceConstantAlpha = alpha; // Use the fading alpha
    blend.AlphaFormat = AC_SRC_ALPHA as u8;

    UpdateLayeredWindow(hwnd, HDC(0), None, Some(&size), mem_dc, Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA);

    SelectObject(mem_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);
}

unsafe extern "system" fn recording_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            let hit_test = (lparam.0 & 0xFFFF) as i16 as i32;
            let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
            
            if hit_test == HTCLIENT as i32 {
                if is_processing {
                    SetCursor(LoadCursorW(None, IDC_APPSTARTING).unwrap());
                } else {
                    SetCursor(LoadCursorW(None, IDC_HAND).unwrap());
                }
                LRESULT(1)
            } else {
                 DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_NCHITTEST => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect);
            let local_x = x - rect.left;
            
            let center_left = BTN_OFFSET;
            let center_right = UI_WIDTH - BTN_OFFSET;
            
            // Only allow button clicks if not processing
            if !AUDIO_STOP_SIGNAL.load(Ordering::SeqCst) {
                if (local_x - center_left).abs() < HIT_RADIUS { return LRESULT(HTCLIENT as isize); }
                if (local_x - center_right).abs() < HIT_RADIUS { return LRESULT(HTCLIENT as isize); }
            } else {
                // During processing, return HTCLIENT for the whole window
                // This consumes clicks in LBUTTONDOWN (where we ignore them), preventing dragging (HTCAPTION) and button actions.
                return LRESULT(HTCLIENT as isize);
            }

            LRESULT(HTCAPTION as isize)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            // Note: lparam coords are relative to client area (top-left 0,0)
            
            let center_left = BTN_OFFSET;
            let center_right = UI_WIDTH - BTN_OFFSET;
            
            if !AUDIO_STOP_SIGNAL.load(Ordering::SeqCst) {
                if (x - center_left).abs() < HIT_RADIUS {
                    IS_PAUSED = !IS_PAUSED;
                    AUDIO_PAUSE_SIGNAL.store(IS_PAUSED, Ordering::SeqCst);
                    paint_layered_window(hwnd, UI_WIDTH, UI_HEIGHT, CURRENT_ALPHA as u8);
                } else if (x - center_right).abs() < HIT_RADIUS {
                    AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
                    PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
            
            if is_processing {
                // Rapid Clockwise Animation for Processing
                // Reduced speed from 20.0 to 8.0
                ANIMATION_OFFSET -= 8.0;
            } else if !IS_PAUSED {
                // Standard Counter-Clockwise Animation
                ANIMATION_OFFSET += 5.0;
            }
            
            // Keep offset bounded to prevent float precision issues over long runs
            if ANIMATION_OFFSET > 3600.0 { ANIMATION_OFFSET -= 3600.0; }
            if ANIMATION_OFFSET < -3600.0 { ANIMATION_OFFSET += 3600.0; }
            
            if CURRENT_ALPHA < 255 {
                CURRENT_ALPHA += 15;
                if CURRENT_ALPHA > 255 { CURRENT_ALPHA = 255; }
            }

            paint_layered_window(hwnd, UI_WIDTH, UI_HEIGHT, CURRENT_ALPHA as u8);
            LRESULT(0)
        }
        WM_CLOSE => {
            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
            DestroyWindow(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
