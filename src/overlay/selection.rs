use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetCapture, ReleaseCapture, VK_ESCAPE};
use windows::core::*;
use std::mem::size_of;

use super::process::process_and_close;
use crate::{APP};

// --- CONFIGURATION ---
const FADE_TIMER_ID: usize = 2;
const ANIM_TIMER_ID: usize = 1;
const TARGET_OPACITY: u8 = 120; 
const FADE_STEP: u8 = 40; // Increased for much faster fade (approx 3 frames / 50ms)
const CORNER_RADIUS: f32 = 12.0;

// --- STATE ---
static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static mut IS_PROCESSING: bool = false;
static mut IS_FADING_OUT: bool = false;
static mut CURRENT_ALPHA: u8 = 0;
static mut SELECTION_OVERLAY_ACTIVE: bool = false;
static mut SELECTION_OVERLAY_HWND: HWND = HWND(0);
static mut CURRENT_PRESET_IDX: usize = 0;
static mut ANIMATION_OFFSET: f32 = 0.0;

// Helper: HSV to RGB
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let h_prime = (h % 360.0) / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h_prime < 1.0 { (c, x, 0.0) }
    else if h_prime < 2.0 { (x, c, 0.0) }
    else if h_prime < 3.0 { (0.0, c, x) }
    else if h_prime < 4.0 { (0.0, x, c) }
    else if h_prime < 5.0 { (x, 0.0, c) }
    else { (c, 0.0, x) };

    let r_u = ((r + m) * 255.0) as u32;
    let g_u = ((g + m) * 255.0) as u32;
    let b_u = ((b + m) * 255.0) as u32;

    (r_u << 16) | (g_u << 8) | b_u 
}

// Signed Distance Function for Rounded Box
fn sd_rounded_box(px: f32, py: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = px.abs() - bx + r;
    let qy = py.abs() - by + r;
    let len_max_q = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let min_max_q = qx.max(qy).min(0.0);
    len_max_q + min_max_q - r
}

pub fn is_selection_overlay_active_and_dismiss() -> bool {
    unsafe {
        if SELECTION_OVERLAY_ACTIVE && SELECTION_OVERLAY_HWND.0 != 0 {
            PostMessageW(SELECTION_OVERLAY_HWND, WM_CLOSE, WPARAM(0), LPARAM(0));
            true
        } else {
            false
        }
    }
}

pub fn show_selection_overlay(preset_idx: usize) {
    unsafe {
        CURRENT_PRESET_IDX = preset_idx;
        SELECTION_OVERLAY_ACTIVE = true;
        ANIMATION_OFFSET = 0.0;
        CURRENT_ALPHA = 0;
        IS_FADING_OUT = false;
        IS_DRAGGING = false;
        IS_PROCESSING = false;
        
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SnippingOverlay");
        
        let mut wc = WNDCLASSW::default();
        if !GetClassInfoW(instance, class_name, &mut wc).as_bool() {
            wc.lpfnWndProc = Some(selection_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_CROSS).unwrap();
            wc.lpszClassName = class_name;
            wc.hbrBackground = CreateSolidBrush(COLORREF(0x00000000));
            RegisterClassW(&wc);
        }

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Snipping"),
            WS_POPUP,
            x, y, w, h,
            None, None, instance, None
        );

        SELECTION_OVERLAY_HWND = hwnd;

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        ShowWindow(hwnd, SW_SHOW);
        
        SetTimer(hwnd, FADE_TIMER_ID, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_QUIT { break; }
        }
        
        SELECTION_OVERLAY_ACTIVE = false;
        SELECTION_OVERLAY_HWND = HWND(0);
    }
}

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if !IS_PROCESSING && !IS_FADING_OUT {
                IS_DRAGGING = true;
                GetCursorPos(std::ptr::addr_of_mut!(START_POS));
                CURR_POS = START_POS;
                SetCapture(hwnd);
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false;
                ReleaseCapture();

                let rect = RECT {
                    left: START_POS.x.min(CURR_POS.x),
                    top: START_POS.y.min(CURR_POS.y),
                    right: START_POS.x.max(CURR_POS.x),
                    bottom: START_POS.y.max(CURR_POS.y),
                };

                let width = (rect.right - rect.left).abs();
                let height = (rect.bottom - rect.top).abs();

                if width > 10 && height > 10 {
                    IS_PROCESSING = true;
                    SetTimer(hwnd, ANIM_TIMER_ID, 16, None);
                    
                    let app_clone = APP.clone();
                    let p_idx = CURRENT_PRESET_IDX;
                    std::thread::spawn(move || {
                        process_and_close(app_clone, rect, hwnd, p_idx);
                    });
                } else {
                    SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let timer_id = wparam.0;
            
            if timer_id == FADE_TIMER_ID {
                let mut changed = false;
                if IS_FADING_OUT {
                    if CURRENT_ALPHA > FADE_STEP {
                        CURRENT_ALPHA -= FADE_STEP;
                        changed = true;
                    } else {
                        CURRENT_ALPHA = 0;
                        KillTimer(hwnd, FADE_TIMER_ID);
                        DestroyWindow(hwnd);
                        PostQuitMessage(0);
                        return LRESULT(0);
                    }
                } else {
                    if CURRENT_ALPHA < TARGET_OPACITY {
                        CURRENT_ALPHA = (CURRENT_ALPHA as u16 + FADE_STEP as u16).min(TARGET_OPACITY as u16) as u8;
                        changed = true;
                    } else {
                        KillTimer(hwnd, FADE_TIMER_ID);
                    }
                }
                
                if changed {
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), CURRENT_ALPHA, LWA_ALPHA);
                }
            }
            
            if timer_id == ANIM_TIMER_ID && IS_PROCESSING {
                ANIMATION_OFFSET += 3.0; 
                if ANIMATION_OFFSET > 360.0 { ANIMATION_OFFSET -= 360.0; }
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            let mem_dc = CreateCompatibleDC(hdc);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            
            let mem_bitmap = CreateCompatibleBitmap(hdc, width, height);
            SelectObject(mem_dc, mem_bitmap);

            // Clear Background
            let brush = CreateSolidBrush(COLORREF(0x00000000));
            let full_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(mem_dc, &full_rect, brush);
            DeleteObject(brush);

            if IS_DRAGGING || IS_PROCESSING {
                let rect_abs = RECT {
                    left: START_POS.x.min(CURR_POS.x),
                    top: START_POS.y.min(CURR_POS.y),
                    right: START_POS.x.max(CURR_POS.x),
                    bottom: START_POS.y.max(CURR_POS.y),
                };

                let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);

                let r = RECT {
                    left: rect_abs.left - screen_x,
                    top: rect_abs.top - screen_y,
                    right: rect_abs.right - screen_x,
                    bottom: rect_abs.bottom - screen_y,
                };
                
                // Use Software Renderer for AA in both cases
                if IS_PROCESSING {
                    let w = (r.right - r.left) as i32;
                    let h = (r.bottom - r.top) as i32;
                    if w > 0 && h > 0 {
                        render_box_sdf(HDC(mem_dc.0), r, w, h, true, ANIMATION_OFFSET);
                    }
                } else {
                    let w = (r.right - r.left) as i32;
                    let h = (r.bottom - r.top) as i32;
                    if w > 0 && h > 0 {
                        render_box_sdf(HDC(mem_dc.0), r, w, h, false, 0.0);
                    }
                }
            }

            BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY).ok().unwrap();
            DeleteObject(mem_bitmap);
            DeleteDC(mem_dc);
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_CLOSE => {
            if !IS_FADING_OUT {
                IS_FADING_OUT = true;
                KillTimer(hwnd, ANIM_TIMER_ID);
                SetTimer(hwnd, FADE_TIMER_ID, 16, None);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// Unified Software Renderer using SDF for AA and Glow
unsafe fn render_box_sdf(hdc_dest: HDC, bounds: RECT, w: i32, h: i32, is_glowing: bool, time_offset: f32) {
    // Pad buffer to allow for AA feathering and Glow reach
    let glow_reach = if is_glowing { 60.0 } else { 2.0 };
    let pad = glow_reach as i32 + 2; 
    let buf_w = w + (pad * 2);
    let buf_h = h + (pad * 2);
    
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: buf_w,
            biHeight: -buf_h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hbm = CreateDIBSection(hdc_dest, &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0).unwrap();
    
    if !p_bits.is_null() {
        let pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (buf_w * buf_h) as usize);
        
        let bx = (w as f32) / 2.0;
        let by = (h as f32) / 2.0;
        let center_x = (pad as f32) + bx;
        let center_y = (pad as f32) + by;

        let time_rad = time_offset.to_radians();

        for y in 0..buf_h {
            for x in 0..buf_w {
                let px = (x as f32) - center_x;
                let py = (y as f32) - center_y;
                
                // Base distance to rounded box
                let d = sd_rounded_box(px, py, bx, by, CORNER_RADIUS);
                
                let mut final_col = 0u32;
                let mut final_alpha = 0.0f32;

                if is_glowing {
                    // --- ANIMATED GLOW MODE ---
                    // 1. Hard Border (AA)
                    if d > 0.0 {
                        // Anti-aliased outer edge (fades out in 1.5px)
                        let aa = (1.5 - d).clamp(0.0, 1.0);
                        if aa > 0.0 {
                            final_alpha = aa;
                            // Outer edge color is bright
                            final_col = 0x00FFFFFF; 
                        }
                    } else {
                        // Inside: d <= 0
                        // 2. Dynamic Noise for "Random Reach"
                        let angle = py.atan2(px);
                        
                        // Noise function: combining sine waves for organic look
                        // Frequencies: 3, 7. Time influences phase.
                        let noise = (angle * 3.0 + time_rad * 2.0).sin() * 0.5 
                                  + (angle * 7.0 - time_rad * 3.0).sin() * 0.3;
                        
                        // Base variance + Noise variance
                        let local_glow_width = 30.0 + (noise * 15.0); 

                        // Distance factor (0.0 at edge, 1.0 at deep center)
                        let dist_in = d.abs();
                        
                        // Cubic Falloff: Strength drops rapidly away from border
                        let t = (dist_in / local_glow_width).clamp(0.0, 1.0);
                        let intensity = (1.0 - t).powi(3); 
                        
                        // Base Alpha is mostly transparent, highlighted by intensity
                        final_alpha = intensity;
                        
                        // Boost the border region significantly
                        if dist_in < 4.0 {
                            final_alpha = 1.0; // Solid border
                        }

                        if final_alpha > 0.005 {
                            // HSV Rainbow Calculation
                            let deg = angle.to_degrees() + 180.0;
                            let hue = (deg + time_offset) % 360.0;
                            let rgb = hsv_to_rgb(hue, 0.8, 1.0);
                            
                            // Blend towards white at the very edge (dist_in < 2.0)
                            if dist_in < 2.0 {
                                final_col = 0x00FFFFFF;
                            } else {
                                final_col = rgb;
                            }
                        }
                    }

                } else {
                    // --- STATIC DRAG MODE (Gray Box with AA) ---
                    let border_width = 2.0;
                    
                    // Inner AA (Hollow center)
                    // We want opacity 1.0 between d=0 and d=-border_width
                    // And fading out at d > 0 and d < -border_width
                    
                    let dist_from_stroke_center = (d + (border_width * 0.5)).abs();
                    // Stroke width covers roughly +/- border_width/2
                    let stroke_mask = (1.0 - (dist_from_stroke_center - (border_width * 0.5))).clamp(0.0, 1.0);

                    final_alpha = stroke_mask;
                    final_col = 0x00CCCCCC; // Light Gray
                }

                // Write Pixel if visible
                if final_alpha > 0.0 {
                    let r = ((final_col >> 16) & 0xFF) as f32;
                    let g = ((final_col >> 8) & 0xFF) as f32;
                    let b = (final_col & 0xFF) as f32;
                    
                    let r_out = (r * final_alpha) as u32;
                    let g_out = (g * final_alpha) as u32;
                    let b_out = (b * final_alpha) as u32;
                    
                    pixels[(y * buf_w + x) as usize] = (r_out << 16) | (g_out << 8) | b_out;
                } else {
                    pixels[(y * buf_w + x) as usize] = 0;
                }
            }
        }
        
        let mem_dc = CreateCompatibleDC(hdc_dest);
        let old_bmp = SelectObject(mem_dc, hbm);
        
        // SRCPAINT (Additive) works best for the glow against the darkened background
        let _ = BitBlt(hdc_dest, bounds.left - pad, bounds.top - pad, buf_w, buf_h, mem_dc, 0, 0, SRCPAINT);
        
        SelectObject(mem_dc, old_bmp);
        DeleteDC(mem_dc);
    }
    DeleteObject(hbm);
}
