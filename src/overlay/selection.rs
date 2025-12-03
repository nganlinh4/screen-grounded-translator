use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetCapture, ReleaseCapture, VK_ESCAPE};
use windows::core::*;
use image::GenericImageView; 

use super::process::start_processing_pipeline;
use crate::APP;

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
static mut SELECTION_OVERLAY_HWND: HWND = HWND(0);
static mut CURRENT_PRESET_IDX: usize = 0;

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
        CURRENT_ALPHA = 0;
        IS_FADING_OUT = false;
        IS_DRAGGING = false;
        
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
            if !IS_FADING_OUT {
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
                    // 1. CROP IMMEDIATELY
                    let (cropped_img, config, preset) = {
                        let guard = APP.lock().unwrap();
                        let original = guard.original_screenshot.as_ref().expect("Screenshot missing");
                        let config_clone = guard.config.clone();
                        let preset_clone = guard.config.presets[CURRENT_PRESET_IDX].clone();

                        let x_virt = GetSystemMetrics(SM_XVIRTUALSCREEN);
                        let y_virt = GetSystemMetrics(SM_YVIRTUALSCREEN);
                        
                        let crop_x = (rect.left - x_virt).max(0) as u32;
                        let crop_y = (rect.top - y_virt).max(0) as u32;
                        let crop_w = (rect.right - rect.left).abs() as u32;
                        let crop_h = (rect.bottom - rect.top).abs() as u32;
                        
                        let img_w = original.width();
                        let img_h = original.height();
                        
                        let final_w = crop_w.min(img_w.saturating_sub(crop_x));
                        let final_h = crop_h.min(img_h.saturating_sub(crop_y));
                        
                        let cropped = original.view(crop_x, crop_y, final_w, final_h).to_image();
                        (cropped, config_clone, preset_clone)
                    };

                    // 2. TRIGGER PROCESSING THREAD IMMEDIATELY
                    std::thread::spawn(move || {
                        start_processing_pipeline(cropped_img, rect, config, preset);
                    });

                    // 3. START FADE OUT
                    IS_FADING_OUT = true;
                    SetTimer(hwnd, FADE_TIMER_ID, 16, None); 
                    
                    return LRESULT(0);
                } else {
                    SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
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

            let brush = CreateSolidBrush(COLORREF(0x00000000));
            let full_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(mem_dc, &full_rect, brush);
            DeleteObject(brush);

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
                
                let w = (r.right - r.left) as i32;
                let h = (r.bottom - r.top) as i32;
                if w > 0 && h > 0 {
                    super::paint_utils::render_box_sdf(
                        HDC(mem_dc.0),
                        r,
                        w,
                        h,
                        false,
                        0.0
                    );
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
                KillTimer(hwnd, FADE_TIMER_ID);
                SetTimer(hwnd, FADE_TIMER_ID, 16, None);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
