use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use crate::APP;

static mut RECORDING_HWND: HWND = HWND(0);
static mut IS_RECORDING: bool = false;
static mut IS_PAUSED: bool = false;
static mut ANIMATION_OFFSET: f32 = 0.0;
static mut CURRENT_PRESET_IDX: usize = 0;

// Shared flag for the audio thread
lazy_static::lazy_static! {
    pub static ref AUDIO_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_PAUSE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub fn is_recording_overlay_active() -> bool {
    unsafe { IS_RECORDING && RECORDING_HWND.0 != 0 }
}

pub fn stop_recording_and_submit() {
    unsafe {
        if IS_RECORDING && RECORDING_HWND.0 != 0 {
            // Signal stop
            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
            // Force repaint to show "Waiting..." immediately
            // But since we use UpdateLayeredWindow, we trigger a timer update or post message
            // Sending a timer message forces a frame update
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
        AUDIO_STOP_SIGNAL.store(false, Ordering::SeqCst);
        AUDIO_PAUSE_SIGNAL.store(false, Ordering::SeqCst);

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("RecordingOverlay");

        let mut wc = WNDCLASSW::default();
        if !GetClassInfoW(instance, class_name, &mut wc).as_bool() {
            wc.lpfnWndProc = Some(recording_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap(); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            RegisterClassW(&wc);
        }

        let w = 340;
        let h = 80;
        let screen_x = GetSystemMetrics(SM_CXSCREEN);
        let screen_y = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_x - w) / 2;
        let y = (screen_y - h) / 2;

        // Use WS_EX_LAYERED for per-pixel alpha (UpdateLayeredWindow)
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("SGT Recording"),
            WS_POPUP,
            x, y, w, h,
            None, None, instance, None
        );

        RECORDING_HWND = hwnd;
        
        SetTimer(hwnd, 1, 16, None); // Animation timer

        // HIDE RECORDING UI LOGIC
        if !preset.hide_recording_ui {
            ShowWindow(hwnd, SW_SHOW);
            // Initial paint to avoid flicker
            paint_layered_window(hwnd, w, h);
        }

        // Start Audio Recording Thread
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

// Helper to draw the window with full alpha channel
unsafe fn paint_layered_window(hwnd: HWND, width: i32, height: i32) {
    let screen_dc = GetDC(None);
    
    // Create a 32-bit DIB for proper alpha support
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

    // 1. Draw Content
    // Fill background with gradient glow
    let is_waiting = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
    let should_animate = !IS_PAUSED || is_waiting;
    
    // Render directly to pixels with pre-multiplied alpha
    if !p_bits.is_null() {
        let pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);
        
        let bx = (width as f32) / 2.0;
        let by = (height as f32) / 2.0;
        let center_x = bx;
        let center_y = by;
        
        let time_rad = ANIMATION_OFFSET.to_radians();
        
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let px = (x as f32) - center_x;
                let py = (y as f32) - center_y;
                
                let d = super::paint_utils::sd_rounded_box(px, py, bx, by, 12.0);
                
                let mut final_col = 0x00FFFFFF;
                let mut final_alpha = 0.0f32;

                if should_animate {
                    if d <= 0.0 {
                         final_alpha = 0.85; 
                         final_col = 0x00111111;
                    } else {
                        let angle = py.atan2(px);
                        let noise = (angle * 2.0 + time_rad * 3.0).sin() * 0.2;
                        let glow_width = 8.0 + (noise * 5.0);
                        
                        let t = (d / glow_width).clamp(0.0, 1.0);
                        final_alpha = (1.0 - t).powi(2);
                        
                        if final_alpha > 0.01 {
                            let hue = (angle.to_degrees() + ANIMATION_OFFSET * 2.0) % 360.0;
                            final_col = super::paint_utils::hsv_to_rgb(hue, 0.8, 1.0);
                        }
                    }
                } else {
                     if d <= 0.0 {
                        final_alpha = 0.85;
                        final_col = 0x00111111;
                     } else if d < 2.0 {
                        final_alpha = 1.0;
                        final_col = 0x00AAAAAA;
                     }
                }

                // PRE-MULTIPLY ALPHA
                let a = (final_alpha * 255.0) as u32;
                let r = ((final_col >> 16) & 0xFF) * a / 255;
                let g = ((final_col >> 8) & 0xFF) * a / 255;
                let b = (final_col & 0xFF) * a / 255;
                
                pixels[idx] = (a << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }

    // Text & UI
    SetBkMode(mem_dc, TRANSPARENT);
    SetTextColor(mem_dc, COLORREF(0x00FFFFFF)); // White text

    // Choose Font
    let hfont = CreateFontW(18, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
    let old_font = SelectObject(mem_dc, hfont);

    let src_text = if is_waiting {
        "Đang xử lý kết quả..."
    } else {
        if CURRENT_PRESET_IDX < APP.lock().unwrap().config.presets.len() {
             let p = &APP.lock().unwrap().config.presets[CURRENT_PRESET_IDX];
             if IS_PAUSED {
                 "Đã tạm dừng"
             } else if p.audio_source == "device" {
                 "Đang ghi âm thiết bị..." 
             } else {
                 "Đang ghi âm micro..."
             }
        } else { "Recording..." }
    };

    let mut text_w = crate::overlay::utils::to_wstring(src_text);
    let mut tr = RECT { left: 40, top: 0, right: width - 40, bottom: height }; // Padding for buttons
    DrawTextW(mem_dc, &mut text_w, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    SelectObject(mem_dc, old_font);
    DeleteObject(hfont);

    // Draw Buttons (Manual drawing for visual feedback)
    let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00DDDDDD));
    let brush = CreateSolidBrush(COLORREF(0x00DDDDDD));
    let old_pen = SelectObject(mem_dc, pen);
    let old_brush = SelectObject(mem_dc, brush);

    // Cancel / Close (X) - Top Right
    // Area: Rightmost 30px
    let btn_x = width - 25;
    let btn_y = 15;
    MoveToEx(mem_dc, btn_x - 5, btn_y - 5, None); LineTo(mem_dc, btn_x + 5, btn_y + 5);
    MoveToEx(mem_dc, btn_x + 5, btn_y - 5, None); LineTo(mem_dc, btn_x - 5, btn_y + 5);

    // Pause / Play - Left side
    // Area: Leftmost 30px
    let play_x = 25;
    let play_y = height / 2; 

    if IS_PAUSED {
        // Play Triangle
        let pts = [POINT{x: play_x - 4, y: play_y - 6}, POINT{x: play_x - 4, y: play_y + 6}, POINT{x: play_x + 6, y: play_y}];
        Polygon(mem_dc, &pts);
    } else {
        // Pause Bars
        Rectangle(mem_dc, play_x - 5, play_y - 6, play_x - 2, play_y + 6);
        Rectangle(mem_dc, play_x + 2, play_y - 6, play_x + 5, play_y + 6);
    }

    SelectObject(mem_dc, old_pen);
    SelectObject(mem_dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);

    // 2. Update Layered Window
    let pt_src = POINT { x: 0, y: 0 };
    let size = SIZE { cx: width, cy: height };
    let mut blend = BLENDFUNCTION::default();
    blend.BlendOp = AC_SRC_OVER as u8;
    blend.SourceConstantAlpha = 255;
    blend.AlphaFormat = AC_SRC_ALPHA as u8;

    UpdateLayeredWindow(hwnd, HDC(0), None, Some(&size), mem_dc, Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA);

    SelectObject(mem_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);
}

unsafe extern "system" fn recording_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            // Check buttons first
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect);
            let local_x = x - rect.left;
            let local_y = y - rect.top;
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;

            // Pause Area (Left 40px)
            if local_x < 40 && local_y > 10 && local_y < h - 10 {
                return LRESULT(HTCLIENT as isize);
            }
            // Close Area (Right 40px, Top 40px)
            if local_x > w - 40 && local_y < 40 {
                return LRESULT(HTCLIENT as isize);
            }

            LRESULT(HTCAPTION as isize)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let w = 340; // Window width known
            
            // Left click logic based on coordinate
            if x < 40 {
                // Pause/Play
                IS_PAUSED = !IS_PAUSED;
                AUDIO_PAUSE_SIGNAL.store(IS_PAUSED, Ordering::SeqCst);
                // Force immediate repaint
                paint_layered_window(hwnd, w, 80);
            } else if x > w - 40 {
                // Cancel
                AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if AUDIO_STOP_SIGNAL.load(Ordering::SeqCst) {
                 ANIMATION_OFFSET += 3.0; // Slow rotate waiting
            } else if !IS_PAUSED {
                ANIMATION_OFFSET += 5.0; // Normal rotate
            }
            if ANIMATION_OFFSET > 360.0 { ANIMATION_OFFSET -= 360.0; }
            
            // Repaint using UpdateLayeredWindow
            paint_layered_window(hwnd, 340, 80);
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
