use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetCapture, ReleaseCapture, VK_ESCAPE};
use windows::core::*;

use super::process::start_processing_pipeline;
use crate::{APP, GdiCapture};
use crate::win_types::{SendHwnd, SendHbitmap};

// --- CONFIGURATION ---
const FADE_TIMER_ID: usize = 2;
const TARGET_OPACITY: u8 = 120; 
const FADE_STEP: u8 = 40; 

// --- STATE ---
static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static mut IS_FADING_OUT: bool = false;
static mut CURRENT_ALPHA: u8 = 0;
static mut SELECTION_OVERLAY_ACTIVE: bool = false;
static mut SELECTION_OVERLAY_HWND: SendHwnd = SendHwnd(HWND(std::ptr::null_mut()));
static mut CURRENT_PRESET_IDX: usize = 0;

// Cached back buffer to avoid per-frame allocations
// Only cache the bitmap (the heavy allocation ~33MB for 4K), DC creation is cheap
static mut CACHED_BITMAP: SendHbitmap = SendHbitmap(HBITMAP(std::ptr::null_mut()));
static mut CACHED_W: i32 = 0;
static mut CACHED_H: i32 = 0;

// Helper to extract bytes from the HBITMAP only for the selected area
unsafe fn extract_crop_from_hbitmap(
    capture: &GdiCapture, 
    crop_rect: RECT
) -> image::ImageBuffer<image::Rgba<u8>, Vec<u8>> {
    let hdc_screen = GetDC(None);
    let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
    
    // Select the big screenshot into DC
    let old_obj = SelectObject(hdc_mem, capture.hbitmap.into());

    let w = (crop_rect.right - crop_rect.left).abs();
    let h = (crop_rect.bottom - crop_rect.top).abs();

    // Create a BMI for just the cropped area
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // Top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut buffer: Vec<u8> = vec![0; (w * h * 4) as usize];

    // Create small temp bitmap, blit crop to it, read bits
    let hdc_temp = CreateCompatibleDC(Some(hdc_screen));
    let hbm_temp = CreateCompatibleBitmap(hdc_screen, w, h);
    SelectObject(hdc_temp, hbm_temp.into());
    
    // Copy only the crop region from the huge screenshot
    // IMPORTANT: virtual screen coordinates calculation
    let v_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let v_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    
    // source x/y in the bitmap
    let src_x = crop_rect.left - v_x;
    let src_y = crop_rect.top - v_y;

    let _ = BitBlt(hdc_temp, 0, 0, w, h, Some(hdc_mem), src_x, src_y, SRCCOPY).ok();
    
    // Now read pixels from small bitmap
    GetDIBits(hdc_temp, hbm_temp, 0, h as u32, Some(buffer.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);

    // BGR -> RGB correction
    for chunk in buffer.chunks_exact_mut(4) {
        chunk.swap(0, 2);
        chunk[3] = 255;
    }

    let _ = DeleteObject(hbm_temp.into());
    let _ = DeleteDC(hdc_temp);
    
    // Cleanup main DC
    SelectObject(hdc_mem, old_obj);
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(None, hdc_screen);

    image::ImageBuffer::from_raw(w as u32, h as u32, buffer).unwrap()
}

pub fn is_selection_overlay_active_and_dismiss() -> bool {
    unsafe {
        if SELECTION_OVERLAY_ACTIVE && !std::ptr::addr_of!(SELECTION_OVERLAY_HWND).read().is_invalid() {
            let _ = PostMessageW(Some(SELECTION_OVERLAY_HWND.0), WM_CLOSE, WPARAM(0), LPARAM(0));
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
        CURRENT_ALPHA = 0;
        IS_FADING_OUT = false;
        IS_DRAGGING = false;
        
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SnippingOverlay");
        
        let mut wc = WNDCLASSW::default();
        if !GetClassInfoW(Some(instance.into()), class_name, &mut wc).is_ok() {
            wc.lpfnWndProc = Some(selection_wnd_proc);
            wc.hInstance = instance.into();
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
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("Snipping"),
            WS_POPUP,
            x, y, w, h,
            None, None, Some(instance.into()), None
        ).unwrap_or_default();

        SELECTION_OVERLAY_HWND = SendHwnd(hwnd);

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        
        let _ = SetTimer(Some(hwnd), FADE_TIMER_ID, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
            if msg.message == WM_QUIT { break; }
        }
        
        SELECTION_OVERLAY_ACTIVE = false;
        SELECTION_OVERLAY_HWND = SendHwnd::default();
    }
}

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if !IS_FADING_OUT {
                IS_DRAGGING = true;
                let _ = GetCursorPos(std::ptr::addr_of_mut!(START_POS));
                CURR_POS = START_POS;
                SetCapture(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                let _ = GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
                // Force immediate repaint for smoothness
                let _ = InvalidateRect(Some(hwnd), None, false);
                let _ = UpdateWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false;
                let _ = ReleaseCapture();

                let rect = RECT {
                    left: START_POS.x.min(CURR_POS.x),
                    top: START_POS.y.min(CURR_POS.y),
                    right: START_POS.x.max(CURR_POS.x),
                    bottom: START_POS.y.max(CURR_POS.y),
                };

                let width = (rect.right - rect.left).abs();
                let height = (rect.bottom - rect.top).abs();

                if width > 10 && height > 10 {
                    // Check if this is a MASTER preset
                    let is_master = {
                        let guard = APP.lock().unwrap();
                        guard.config.presets.get(CURRENT_PRESET_IDX)
                            .map(|p| p.is_master)
                            .unwrap_or(false)
                    };
                    
                    // For MASTER presets, show the preset wheel first
                    let final_preset_idx = if is_master {
                        // Get cursor position for wheel center
                        let mut cursor_pos = POINT::default();
                        let _ = GetCursorPos(&mut cursor_pos);
                        
                        // Hide selection overlay temporarily while showing wheel
                        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 60, LWA_ALPHA);
                        
                        // Show preset wheel - this blocks until user makes selection
                        let selected = super::preset_wheel::show_preset_wheel("image", None, cursor_pos);
                        
                        if let Some(idx) = selected {
                            Some(idx)
                        } else {
                            // User dismissed wheel - cancel operation
                            IS_FADING_OUT = true;
                            SetTimer(Some(hwnd), FADE_TIMER_ID, 16, None);
                            return LRESULT(0);
                        }
                    } else {
                        Some(CURRENT_PRESET_IDX)
                    };
                    
                    if let Some(preset_idx) = final_preset_idx {
                        // 1. EXTRACT CROP (New Logic)
                        let (cropped_img, config, preset) = {
                            let mut guard = APP.lock().unwrap();
                            
                            // CRITICAL: Update active_preset_idx so auto_paste logic works!
                            guard.config.active_preset_idx = preset_idx;
                            
                            // Access the handle
                            let capture = guard.screenshot_handle.as_ref().expect("Screenshot handle missing");
                            let config_clone = guard.config.clone();
                            let preset_clone = guard.config.presets[preset_idx].clone();

                            // Extract pixels NOW (The slow part happens here, AFTER user finishes drawing)
                            let img = extract_crop_from_hbitmap(capture, rect);
                            
                            (img, config_clone, preset_clone)
                        };

                        // 2. TRIGGER PROCESSING
                        std::thread::spawn(move || {
                            // Pass the rect for result window positioning
                            start_processing_pipeline(cropped_img, rect, config, preset);
                        });
                    }

                    // 3. START FADE OUT
                    IS_FADING_OUT = true;
                    let _ = SetTimer(Some(hwnd), FADE_TIMER_ID, 16, None); 
                    
                    return LRESULT(0);
                } else {
                    let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == FADE_TIMER_ID {
                let mut changed = false;
                if IS_FADING_OUT {
                    if CURRENT_ALPHA > FADE_STEP {
                        CURRENT_ALPHA -= FADE_STEP;
                        changed = true;
                    } else {
                        CURRENT_ALPHA = 0;
                        let _ = KillTimer(Some(hwnd), FADE_TIMER_ID);
                        let _ = DestroyWindow(hwnd);
                        PostQuitMessage(0);
                        return LRESULT(0);
                    }
                } else {
                    if CURRENT_ALPHA < TARGET_OPACITY {
                        CURRENT_ALPHA = (CURRENT_ALPHA as u16 + FADE_STEP as u16).min(TARGET_OPACITY as u16) as u8;
                        changed = true;
                    } else {
                        let _ = KillTimer(Some(hwnd), FADE_TIMER_ID);
                    }
                }
                
                if changed {
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), CURRENT_ALPHA, LWA_ALPHA);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            
            // OPTIMIZATION: Cache the full-screen bitmap (heavy allocation ~33MB for 4K)
            // The DC is lightweight and created per-frame, bitmap is reused
            if std::ptr::addr_of!(CACHED_BITMAP).read().is_invalid() || CACHED_W != width || CACHED_H != height {
                if !std::ptr::addr_of!(CACHED_BITMAP).read().is_invalid() {
                    let _ = DeleteObject(CACHED_BITMAP.0.into());
                }
                let hbm = CreateCompatibleBitmap(hdc, width, height);
                CACHED_BITMAP = SendHbitmap(hbm);
                CACHED_W = width;
                CACHED_H = height;
            }
            
            // Create lightweight DC per-frame (no expensive allocation)
            let mem_dc = CreateCompatibleDC(Some(hdc));
            let old_bmp = SelectObject(mem_dc, CACHED_BITMAP.0.into());

            // 1. Clear background using stock black brush (no allocation)
            let black_brush = GetStockObject(BLACK_BRUSH);
            let full_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(mem_dc, &full_rect, HBRUSH(black_brush.0));

            if IS_DRAGGING {
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
                
                let w = (r.right - r.left).abs();
                let h = (r.bottom - r.top).abs();
                
                if w > 0 && h > 0 {
                    // FIX: Use Native GDI RoundRect instead of CPU-heavy SDF
                    // This is hardware accelerated and instant for 4K+ resolutions
                    
                    // Create White Pen (2px thick)
                    let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00FFFFFF));
                    let old_pen = SelectObject(mem_dc, pen.into());
                    
                    // Use Null Brush (Transparent Fill)
                    let null_brush = GetStockObject(NULL_BRUSH);
                    let old_brush = SelectObject(mem_dc, null_brush);
                    
                    // Draw Rounded Rectangle
                    let _ = RoundRect(mem_dc, r.left, r.top, r.right, r.bottom, 12, 12);
                    
                    // Cleanup
                    SelectObject(mem_dc, old_brush);
                    SelectObject(mem_dc, old_pen);
                    let _ = DeleteObject(pen.into());
                }
            }

            // Blit to screen
            let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY).ok();
            
            // Cleanup DC (but keep cached bitmap)
            SelectObject(mem_dc, old_bmp);
            let _ = DeleteDC(mem_dc);
            
            let _ = EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_CLOSE => {
            if !IS_FADING_OUT {
                IS_FADING_OUT = true;
                let _ = KillTimer(Some(hwnd), FADE_TIMER_ID);
                SetTimer(Some(hwnd), FADE_TIMER_ID, 16, None);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Cleanup cached back buffer resources
            if !std::ptr::addr_of!(CACHED_BITMAP).read().is_invalid() {
                let _ = DeleteObject(CACHED_BITMAP.0.into());
                CACHED_BITMAP = SendHbitmap::default();
            }
            CACHED_W = 0;
            CACHED_H = 0;
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
