use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::win_types::SendHwnd;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering, AtomicU32}, Once, Mutex};
use crate::APP;


static mut RECORDING_HWND: SendHwnd = SendHwnd(HWND(std::ptr::null_mut()));
static mut IS_RECORDING: bool = false;
static mut IS_PAUSED: bool = false;
static mut ANIMATION_OFFSET: f32 = 0.0;
static mut CURRENT_PRESET_IDX: usize = 0;
static mut CURRENT_ALPHA: i32 = 0; // For fade-in

// Audio Viz
lazy_static::lazy_static! {
    static ref VISUALIZATION_BUFFER: Mutex<[f32; 40]> = Mutex::new([0.0; 40]);
}
static mut VIS_HEAD: usize = 0;
pub static CURRENT_RMS: AtomicU32 = AtomicU32::new(0);

// --- UI CONSTANTS ---
const UI_WIDTH: i32 = 350;   // More compact width
const UI_HEIGHT: i32 = 80;   // Reduced height
const BTN_OFFSET: i32 = 40;  // Distance from edge to icon center
const HIT_RADIUS: i32 = 25;  // Clickable radius around buttons

pub fn update_audio_viz(rms: f32) {
    let bits = rms.to_bits();
    CURRENT_RMS.store(bits, Ordering::Relaxed);
}

// Shared flag for the audio thread
lazy_static::lazy_static! {
    pub static ref AUDIO_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_PAUSE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    // FIX: New signal to explicitly abort/discard recording
    pub static ref AUDIO_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    // NEW: Signal set by audio thread when first data arrives (mic is ready)
    pub static ref AUDIO_WARMUP_COMPLETE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

// Warmup frames counter - how many frames to wait before showing waveform (~25 frames @ 16ms = 400ms)
const WARMUP_FRAMES: i32 = 25;
static mut WARMUP_COUNTER: i32 = 0;

// OPTIMIZATION: Thread-safe one-time window class registration
static REGISTER_RECORDING_CLASS: Once = Once::new();

pub fn is_recording_overlay_active() -> bool {
    unsafe { IS_RECORDING && !RECORDING_HWND.is_invalid() }
}

pub fn stop_recording_and_submit() {
    unsafe {
        if IS_RECORDING && !RECORDING_HWND.is_invalid() {
            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
            // Force immediate update to show "Processing"
            let _ = PostMessageW(Some(RECORDING_HWND.0), WM_TIMER, WPARAM(0), LPARAM(0));
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
        WARMUP_COUNTER = 0; // Reset warmup counter
        AUDIO_STOP_SIGNAL.store(false, Ordering::SeqCst);
         AUDIO_PAUSE_SIGNAL.store(false, Ordering::SeqCst);
         AUDIO_ABORT_SIGNAL.store(false, Ordering::SeqCst); // Reset abort signal
         AUDIO_WARMUP_COMPLETE.store(false, Ordering::SeqCst); // Reset warmup signal
          
          // Reset viz
          VIS_HEAD = 0;
          if let Ok(mut buffer) = VISUALIZATION_BUFFER.lock() {
              buffer.fill(0.0);
          }

         let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("RecordingOverlay");

        // OPTIMIZATION: Register class only once, thread-safely
        REGISTER_RECORDING_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(recording_wnd_proc);
            wc.hInstance = instance.into();
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
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT Recording"),
            WS_POPUP,
            x, y, UI_WIDTH, UI_HEIGHT,
            None, None, Some(instance.into()), None
        ).unwrap_or_default();

        RECORDING_HWND = SendHwnd(hwnd);
        
        SetTimer(Some(hwnd), 1, 16, None); 

        if !preset.hide_recording_ui {
            // Initially 0 alpha, will fade in via timer
            paint_layered_window(hwnd, UI_WIDTH, UI_HEIGHT, 0);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }

        let hwnd_val = hwnd.0 as usize;
        std::thread::spawn(move || {
            let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
            // FIX: Pass AUDIO_ABORT_SIGNAL to the worker thread
            crate::api::record_audio_and_transcribe(
                preset, 
                AUDIO_STOP_SIGNAL.clone(), 
                AUDIO_PAUSE_SIGNAL.clone(), 
                AUDIO_ABORT_SIGNAL.clone(),
                hwnd
            );
        });

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
            if msg.message == WM_QUIT { break; }
        }

        IS_RECORDING = false;
        RECORDING_HWND = SendHwnd::default();
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
    let bitmap = CreateDIBSection(Some(screen_dc), &bmi, windows::Win32::Graphics::Gdi::DIB_RGB_COLORS, &mut p_bits, None, 0).unwrap();
    
    let mem_dc = CreateCompatibleDC(Some(screen_dc));
    let old_bitmap = SelectObject(mem_dc, bitmap.into());

    let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst); // API processing after stop
    let warmup_complete = AUDIO_WARMUP_COMPLETE.load(Ordering::SeqCst);
    
    // Warming up = counter hasn't reached threshold yet OR audio hasn't signaled ready
    // Once WARMUP_COUNTER hits WARMUP_FRAMES, we're done warming up regardless of signal
    let is_warming_up = WARMUP_COUNTER < WARMUP_FRAMES && !warmup_complete;
    
    // "is_waiting" used for the spinning/pulsing glow effect (either warming up OR processing)
    let is_waiting = is_processing || is_warming_up;
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
                // FIX: Use consistent corner radius with proper inset compensation
                // Inner box is inset by 4px from edges, corner radius is 14px
                // This ensures the glow (which extends outward) follows the same curvature
                // and doesn't "peek out" at the diagonal corners due to radius mismatch
                let corner_radius = 14.0;
                let inset = 4.0;
                let d = super::paint_utils::sd_rounded_box(px, py, bx - inset, by - inset, corner_radius);
                
                let mut final_col = 0x000000;
                let mut final_alpha = 0.0f32;

                if should_animate {
                    // PROPER SDF ANTI-ALIASING:
                    // - Fully inside (d < -1): solid fill
                    // - Edge zone (-1 <= d <= 1): smooth blend  
                    // - Outside (d > 0): glow
                    
                    let aa_half = 0.75; // Half-width of AA zone (~1.5px total)
                    
                    if d < -aa_half {
                        // Fully inside - solid fill, NO gradient
                        final_alpha = 0.40;
                        final_col = 0x00050505;
                    } else if d < aa_half {
                        // Edge zone - anti-alias by blending fill with glow
                        // Map d from [-aa_half, aa_half] to [0, 1]
                        let t = (d + aa_half) / (aa_half * 2.0);
                        // Smoothstep for sub-pixel curve
                        let blend = t * t * (3.0 - 2.0 * t);
                        
                        // Calculate glow color/intensity at this point
                        let angle = py.atan2(px);
                        let noise = if is_waiting {
                            (angle * 10.0 - time_rad * 8.0).sin() * 0.5
                        } else {
                            (angle * 2.0 + time_rad * 3.0).sin() * 0.2
                        };
                        let glow_width = if is_waiting { 14.0 } else { 8.0 } + (noise * 5.0);
                        let glow_t = (d.max(0.0) / glow_width).clamp(0.0, 1.0);
                        let glow_intensity = (1.0 - glow_t).powi(2);
                        
                        let hue_offset = if is_waiting { ANIMATION_OFFSET * 4.0 } else { ANIMATION_OFFSET * 2.0 };
                        // Use rem_euclid to handle negative values correctly (Rust % preserves sign)
                        let hue = (angle.to_degrees() + hue_offset).rem_euclid(360.0);
                        let sat = if is_waiting { 1.0 } else { 0.85 };
                        let glow_rgb = super::paint_utils::hsv_to_rgb(hue, sat, 1.0);
                        
                        // Blend: (1-blend) * fill + blend * glow
                        let fill_alpha = 0.40 * (1.0 - blend);
                        let glow_alpha = glow_intensity * blend;
                        final_alpha = fill_alpha + glow_alpha;
                        
                        // Color blend (premultiplied)
                        let fill_r = 0x05 as f32 * (1.0 - blend);
                        let fill_g = 0x05 as f32 * (1.0 - blend);
                        let fill_b = 0x05 as f32 * (1.0 - blend);
                        let glow_r = ((glow_rgb >> 16) & 0xFF) as f32 * blend * glow_intensity;
                        let glow_g = ((glow_rgb >> 8) & 0xFF) as f32 * blend * glow_intensity;
                        let glow_b = (glow_rgb & 0xFF) as f32 * blend * glow_intensity;
                        
                        if final_alpha > 0.001 {
                            let r = ((fill_r + glow_r) / final_alpha * 0.40).min(255.0) as u32;
                            let g = ((fill_g + glow_g) / final_alpha * 0.40).min(255.0) as u32;
                            let b = ((fill_b + glow_b) / final_alpha * 0.40).min(255.0) as u32;
                            final_col = (r << 16) | (g << 8) | b;
                        }
                    } else {
                        // Outside - pure glow with OUTER CORNER CLIPPING
                        let angle = py.atan2(px);
                        
                        let noise = if is_waiting {
                            (angle * 10.0 - time_rad * 8.0).sin() * 0.5
                        } else {
                            (angle * 2.0 + time_rad * 3.0).sin() * 0.2
                        };
                        
                        let glow_width = if is_waiting { 14.0 } else { 8.0 } + (noise * 5.0);
                        
                        // Calculate outer boundary SDF for corner clipping
                        // Use full window bounds with appropriately scaled corner radius
                        // Outer corner radius grows with glow but capped to avoid over-clipping
                        let outer_corner_radius = (corner_radius + glow_width * 0.5).min(by - 2.0);
                        let d_outer = super::paint_utils::sd_rounded_box(px, py, bx, by, outer_corner_radius);
                        
                        // Fade glow based on distance from inner shape
                        let t = (d / glow_width).clamp(0.0, 1.0);
                        let mut glow_intensity = (1.0 - t).powi(2);
                        
                        // Clip glow at outer rounded boundary (anti-aliased)
                        if d_outer > -1.5 {
                            let outer_fade = ((-d_outer) / 1.5).clamp(0.0, 1.0);
                            glow_intensity *= outer_fade;
                        }
                        
                        if glow_intensity > 0.01 {
                            let hue_offset = if is_waiting { ANIMATION_OFFSET * 4.0 } else { ANIMATION_OFFSET * 2.0 };
                            // Use rem_euclid to handle negative values correctly
                            let hue = (angle.to_degrees() + hue_offset).rem_euclid(360.0);
                            let sat = if is_waiting { 1.0 } else { 0.85 };
                            let rgb = super::paint_utils::hsv_to_rgb(hue, sat, 1.0);
                            final_col = rgb;
                            final_alpha = glow_intensity;
                        }
                    }
                } else {
                    // PAUSED STATE - with proper SDF anti-aliasing
                    let aa_half = 0.75;
                    
                    if d < -aa_half {
                        // Fully inside - solid fill
                        final_alpha = 0.40;
                        final_col = 0x00050505;
                    } else if d < aa_half {
                        // Edge zone - anti-alias
                        let t = (d + aa_half) / (aa_half * 2.0);
                        let blend = t * t * (3.0 - 2.0 * t);
                        // Blend fill to border
                        final_alpha = 0.40 * (1.0 - blend) + 0.8 * blend;
                        // Color blend from dark fill to gray border
                        let r = (0x05 as f32 * (1.0 - blend) + 0xAA as f32 * blend) as u32;
                        let g = r;
                        let b = r;
                        final_col = (r << 16) | (g << 8) | b;
                    } else if d < 2.0 {
                        // Outer border with AA fade-out
                        let t = ((d - aa_half) / (2.0 - aa_half)).clamp(0.0, 1.0);
                        let aa = 1.0 - t * t * (3.0 - 2.0 * t);
                        final_alpha = 0.8 * aa;
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

        // 2. Draw Viz Bars (if valid)
         if !is_waiting && !IS_PAUSED {
             let viz_w = 220.0;
             let bar_w = 3.0;
             let spacing = 2.0;
             let start_x = (width as f32 - viz_w) / 2.0;
             let center_y = (height as f32) / 2.0;
             
             for i in 0..40 {
                 let idx = (VIS_HEAD + 40 - i) % 40;
                 let amp = {
                     let buffer = VISUALIZATION_BUFFER.lock().unwrap();
                     buffer[idx]
                 };
                 let h = (amp * 400.0).clamp(4.0, 40.0); // Height scaling
                 
                 let x = start_x + (i as f32 * (bar_w + spacing));
                 let y_top = center_y - h/2.0;
                 let y_bot = center_y + h/2.0;
                 
                 // Draw bar
                 for py in y_top as i32..y_bot as i32 {
                     for px in x as i32..(x + bar_w) as i32 {
                         let p_idx = (py * width + px) as usize;
                         if p_idx < (width * height) as usize {
                             // Use a solid neon color (Cyan)
                             // Premultiplied alpha: If alpha is 255, values are straight.
                             pixels[p_idx] = (255 << 24) | (0 << 16) | (200 << 8) | 255;
                         }
                     }
                 }
             }
         }

         // 3. Draw Icons directly to pixels
         let white_pixel = 0xFFFFFFFF;

         // -- PAUSE / PLAY BUTTON (Left) --
         // Only show pause/play if NOT processing
         if !is_waiting {
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
         }

         // -- CLOSE BUTTON (X) (Right) --
         // ALWAYS show close button
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

    SetBkMode(mem_dc, TRANSPARENT);
    SetTextColor(mem_dc, COLORREF(0x00FFFFFF));

    // --- MAIN STATUS TEXT ---
    // Moved up significantly to be optically centered in top half
    let hfont_main = CreateFontW(19, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
    let old_font = SelectObject(mem_dc, hfont_main.into());

    let src_text = if is_processing {
        "Đang xử lý..."
    } else if is_warming_up {
        "Chuẩn bị mic..."
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

    let _ = SelectObject(mem_dc, old_font);
    let _ = DeleteObject(hfont_main.into());

    // Only show sub-text if not processing
    if !is_waiting {
        let hfont_sub = CreateFontW(14, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        SelectObject(mem_dc, hfont_sub.into());
        SetTextColor(mem_dc, COLORREF(0x00DDDDDD)); 

        let sub_text = "Bấm hotkey lần nữa để xử lý âm thanh";
        let mut sub_text_w = crate::overlay::utils::to_wstring(sub_text);
        let mut tr_sub = RECT { left: 0, top: 47, right: width, bottom: height };
        DrawTextW(mem_dc, &mut sub_text_w, &mut tr_sub, DT_CENTER | DT_TOP | DT_SINGLELINE);

        let _ = SelectObject(mem_dc, old_font);
        let _ = DeleteObject(hfont_sub.into());
    }
    
    // --- TEXT ALPHA FIX ---
    // GDI DrawText doesn't properly handle alpha for layered windows (it writes RGB but ignores Dest Alpha).
    // This causes white text on semi-transparent backgrounds to have RGB > Alpha, which looks "bloomed" or "washed out".
    // We iterate the buffer and force Alpha to be at least the max of R,G,B (Premultiplied constraint).
    if !p_bits.is_null() {
        GdiFlush(); // Ensure GDI drawing is committed to bits
        let pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);
        for px in pixels.iter_mut() {
            let val = *px;
            let a = (val >> 24) & 0xFF;
            let r = (val >> 16) & 0xFF;
            let g = (val >> 8) & 0xFF;
            let b = val & 0xFF;
            
            let max_c = r.max(g).max(b);
            if max_c > a {
                // Fix alpha to support valid premultiplied rendering for the text
                *px = (max_c << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }

    let pt_src = POINT { x: 0, y: 0 };
    let size = SIZE { cx: width, cy: height };
    let mut blend = BLENDFUNCTION::default();
    blend.BlendOp = AC_SRC_OVER as u8;
    blend.SourceConstantAlpha = alpha; // Use the fading alpha
    blend.AlphaFormat = AC_SRC_ALPHA as u8;

    UpdateLayeredWindow(hwnd, Some(HDC::default()), None, Some(&size), Some(mem_dc), Some(&pt_src), COLORREF(0), Some(&blend), ULW_ALPHA);

    let _ = SelectObject(mem_dc, old_bitmap);
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(mem_dc);
    let _ = ReleaseDC(None, screen_dc);
}

unsafe extern "system" fn recording_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            let hit_test = (lparam.0 & 0xFFFF) as i16 as i32;
            let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
            
            if hit_test == HTCLIENT as i32 {
                if is_processing {
                    SetCursor(Some(LoadCursorW(None, IDC_APPSTARTING).unwrap()));
                } else {
                    SetCursor(Some(LoadCursorW(None, IDC_HAND).unwrap()));
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
            
            let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
            
            // ALWAYS allow Close button to be hit, even during processing
            if (local_x - center_right).abs() < HIT_RADIUS {
                return LRESULT(HTCLIENT as isize);
            }
            
            // Only allow Pause button if not processing
            if !is_processing {
                if (local_x - center_left).abs() < HIT_RADIUS { 
                    return LRESULT(HTCLIENT as isize); 
                }
            }

            LRESULT(HTCAPTION as isize)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            // Note: lparam coords are relative to client area (top-left 0,0)
            
            let center_left = BTN_OFFSET;
            let center_right = UI_WIDTH - BTN_OFFSET;
            let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
            
            // Always check Close Button first
            if (x - center_right).abs() < HIT_RADIUS {
                // FIX: Clicked "X" button -> ABORT, NOT SUBMIT
                AUDIO_ABORT_SIGNAL.store(true, Ordering::SeqCst); 
                AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst); // Stop loop
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            } 
            // Only allow Pause button if not processing
            else if !is_processing {
                if (x - center_left).abs() < HIT_RADIUS {
                    IS_PAUSED = !IS_PAUSED;
                    AUDIO_PAUSE_SIGNAL.store(IS_PAUSED, Ordering::SeqCst);
                    paint_layered_window(hwnd, UI_WIDTH, UI_HEIGHT, CURRENT_ALPHA as u8);
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
             let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
             let warmup_complete = AUDIO_WARMUP_COMPLETE.load(Ordering::SeqCst);
             let is_warming_up = WARMUP_COUNTER < WARMUP_FRAMES && !warmup_complete && !is_processing;
             
             // Increment warmup counter each frame
             if is_warming_up {
                 WARMUP_COUNTER += 1;
             }
             
             if is_processing || is_warming_up {
                 // Rapid Clockwise Animation for Processing/Warmup
                 // Warmup uses slightly faster spin to show activity
                 let speed = if is_warming_up { 12.0 } else { 8.0 };
                 ANIMATION_OFFSET -= speed;
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

             // UPDATE VIZ BUFFER
             let rms_bits = CURRENT_RMS.load(Ordering::Relaxed);
             let rms = f32::from_bits(rms_bits);
             unsafe {
                 VIS_HEAD = (VIS_HEAD + 1) % 40;
             }
             if let Ok(mut buffer) = VISUALIZATION_BUFFER.lock() {
                 buffer[VIS_HEAD] = rms;
             }

             paint_layered_window(hwnd, UI_WIDTH, UI_HEIGHT, CURRENT_ALPHA as u8);
             LRESULT(0)
         }
        WM_CLOSE => {
             // FIX: Ensure clean stop even if Alt+F4
            AUDIO_ABORT_SIGNAL.store(true, Ordering::SeqCst);
            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
            let _ = DestroyWindow(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
