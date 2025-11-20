use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::core::*;
use std::sync::{Arc, Mutex};
use std::mem::size_of;
use image::GenericImageView; 

use crate::{AppState, APP, api::translate_image};

static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static mut IS_PROCESSING: bool = false;
static mut SCAN_LINE_Y: i32 = 0;
static mut SCAN_DIR: i32 = 5;
static mut SELECTION_OVERLAY_ACTIVE: bool = false;
static mut SELECTION_OVERLAY_HWND: HWND = HWND(0);

fn to_wstring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// Helper to check if selection overlay is currently active and dismiss it
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


// --- CLIPBOARD SUPPORT ---
fn copy_to_clipboard(text: &str, hwnd: HWND) {
    unsafe {
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
        }
    }
}

// --- 1. SELECTION OVERLAY ---

pub fn show_selection_overlay() {
    unsafe {
        // Mark overlay as active
        SELECTION_OVERLAY_ACTIVE = true;
        
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SnippingOverlay");
        
        let wc = WNDCLASSW {
            lpfnWndProc: Some(selection_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap(),
            lpszClassName: class_name,
            hbrBackground: CreateSolidBrush(COLORREF(0x00000000)),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        
        let hwnd = CreateWindowExW(
            // WS_EX_TOOLWINDOW prevents taskbar appearance
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Snipping"),
            WS_POPUP | WS_VISIBLE,
            x, y, w, h,
            None, None, instance, None
        );

        // Store the window handle
        SELECTION_OVERLAY_HWND = hwnd;

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 100, LWA_ALPHA);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_CLOSE { break; }
        }
        
        // Mark overlay as inactive when it closes
        SELECTION_OVERLAY_ACTIVE = false;
        SELECTION_OVERLAY_HWND = HWND(0);
        
        UnregisterClassW(class_name, instance);
    }
}

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if !IS_PROCESSING {
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

                if (rect.right - rect.left) > 10 && (rect.bottom - rect.top) > 10 {
                    IS_PROCESSING = true;
                    SCAN_LINE_Y = rect.top;
                    InvalidateRect(hwnd, None, false);
                    SetTimer(hwnd, 1, 30, None);
                    
                    let app_clone = APP.clone();
                    std::thread::spawn(move || {
                        process_and_close(app_clone, rect, hwnd);
                    });
                } else {
                    PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if IS_PROCESSING {
                let rect = RECT {
                    left: START_POS.x.min(CURR_POS.x),
                    top: START_POS.y.min(CURR_POS.y),
                    right: START_POS.x.max(CURR_POS.x),
                    bottom: START_POS.y.max(CURR_POS.y),
                };
                
                SCAN_LINE_Y += SCAN_DIR;
                if SCAN_LINE_Y > rect.bottom || SCAN_LINE_Y < rect.top {
                    SCAN_DIR = -SCAN_DIR;
                }
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

            let brush = CreateSolidBrush(COLORREF(0x00000000));
            let full_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(mem_dc, &full_rect, brush);
            DeleteObject(brush);

            if IS_DRAGGING || IS_PROCESSING {
                let rect = RECT {
                    left: (START_POS.x.min(CURR_POS.x)) - GetSystemMetrics(SM_XVIRTUALSCREEN),
                    top: (START_POS.y.min(CURR_POS.y)) - GetSystemMetrics(SM_YVIRTUALSCREEN),
                    right: (START_POS.x.max(CURR_POS.x)) - GetSystemMetrics(SM_XVIRTUALSCREEN),
                    bottom: (START_POS.y.max(CURR_POS.y)) - GetSystemMetrics(SM_YVIRTUALSCREEN),
                };
                
                let frame_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
                FrameRect(mem_dc, &rect, frame_brush);
                DeleteObject(frame_brush);
                
                if IS_PROCESSING {
                     let scan_y_rel = SCAN_LINE_Y - GetSystemMetrics(SM_YVIRTUALSCREEN);
                     let scan_rect = RECT {
                         left: rect.left + 2,
                         top: scan_y_rel,
                         right: rect.right - 2,
                         bottom: scan_y_rel + 2
                     };
                     let scan_brush = CreateSolidBrush(COLORREF(0x0000FF00));
                     FillRect(mem_dc, &scan_rect, scan_brush);
                     DeleteObject(scan_brush);
                }
            }

            BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY).ok().unwrap();
            DeleteObject(mem_bitmap);
            DeleteDC(mem_dc);
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn process_and_close(app: Arc<Mutex<AppState>>, rect: RECT, overlay_hwnd: HWND) {
    let (img, config, model_name) = {
        let mut guard = app.lock().unwrap();
        let model = guard.model_selector.get_next_model();
        (guard.original_screenshot.clone().unwrap(), guard.config.clone(), model)
    };

    let x_virt = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y_virt = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    
    let crop_x = (rect.left - x_virt).max(0) as u32;
    let crop_y = (rect.top - y_virt).max(0) as u32;
    let crop_w = (rect.right - rect.left).abs() as u32;
    let crop_h = (rect.bottom - rect.top).abs() as u32;
    
    let img_w = img.width();
    let img_h = img.height();
    let crop_w = crop_w.min(img_w.saturating_sub(crop_x));
    let crop_h = crop_h.min(img_h.saturating_sub(crop_y));

    if crop_w > 0 && crop_h > 0 {
        let cropped = img.view(crop_x, crop_y, crop_w, crop_h).to_image();
        
        // Store settings before config is moved
        let auto_copy = config.auto_copy;
        let target_lang = config.target_language.clone();
        let api_key = config.api_key.clone();
        
        // Blocking call - no async/await needed
        let res = translate_image(api_key, config.target_language, model_name, cropped);
        
        unsafe {
            KillTimer(overlay_hwnd, 1);
            PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }

        match res {
            Ok(text) => {
                if !text.trim().is_empty() {
                    let text_for_result = text.clone();
                    
                    std::thread::spawn(move || {
                        show_result_window(rect, text_for_result);
                    });
                    
                    // Apply auto-copy if enabled
                    if auto_copy {
                        let text_for_copy = text.clone();
                        std::thread::spawn(move || {
                            // Small delay to ensure window is created
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            copy_to_clipboard(&text_for_copy, unsafe { GetActiveWindow() });
                        });
                    }
                }
            }
            Err(e) => {
                let err_msg = format!("Error: {}", e);
                std::thread::spawn(move || show_result_window(rect, err_msg));
            }
        }
    } else {
        unsafe { PostMessageW(overlay_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
    }
    
    unsafe {
        IS_PROCESSING = false;
        IS_DRAGGING = false;
    }
}

// --- 2. RESULT WINDOW (Fixed Padding & Taskbar) ---

static mut IS_DISMISSING: bool = false;
static mut DISMISS_ALPHA: u8 = 220;

pub fn show_result_window(target_rect: RECT, text: String) {
    unsafe {
        IS_DISMISSING = false;
        DISMISS_ALPHA = 220;
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        let mut wc = WNDCLASSW::default();
        if !GetClassInfoW(instance, class_name, &mut wc).as_bool() {
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance;
            // Load custom broom cursor from embedded data
            static BROOM_CURSOR_DATA: &[u8] = include_bytes!("../broom.cur");
            
            // Write cursor to temp file
            let temp_path = std::env::temp_dir().join("broom_cursor.cur");
            if let Ok(()) = std::fs::write(&temp_path, BROOM_CURSOR_DATA) {
                let path_wide: Vec<u16> = temp_path.to_string_lossy()
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let cursor_handle = LoadImageW(
                    None,
                    PCWSTR(path_wide.as_ptr()),
                    IMAGE_CURSOR,
                    0, 0,
                    LR_LOADFROMFILE | LR_DEFAULTSIZE
                );
                wc.hCursor = if let Ok(handle) = cursor_handle {
                    HCURSOR(handle.0)
                } else {
                    LoadCursorW(None, IDC_HAND).unwrap()
                };
            } else {
                wc.hCursor = LoadCursorW(None, IDC_HAND).unwrap();
            }
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW;
            RegisterClassW(&wc);
        }

        let width = (target_rect.right - target_rect.left).abs();
        let height = (target_rect.bottom - target_rect.top).abs();
        
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name,
            PCWSTR(to_wstring(&text).as_ptr()),
            WS_POPUP | WS_VISIBLE,
            target_rect.left, target_rect.top, width, height,
            None, None, instance, None
        );

        let policy = DWM_SYSTEMBACKDROP_TYPE(2);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &policy as *const _ as *const _, size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), DISMISS_ALPHA, LWA_ALPHA);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(hwnd).as_bool() { break; }
        }
    }
}

unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONUP => {
            // Start fade-out animation on left click
            IS_DISMISSING = true;
            SetTimer(hwnd, 2, 8, None); // ~120 FPS (half duration)
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            // Right-click: copy to clipboard
            let text_len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; text_len as usize];
            GetWindowTextW(hwnd, &mut buf);
            
            // Convert back to String (remove null terminator)
            let text = String::from_utf16_lossy(&buf[..text_len as usize - 1]).to_string();
            copy_to_clipboard(&text, hwnd);
            
            // Dismiss window
            IS_DISMISSING = true;
            SetTimer(hwnd, 2, 8, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == 2 && IS_DISMISSING {
                if DISMISS_ALPHA > 15 {
                    DISMISS_ALPHA = DISMISS_ALPHA.saturating_sub(15);
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), DISMISS_ALPHA, LWA_ALPHA);
                } else {
                    KillTimer(hwnd, 2);
                    DestroyWindow(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => { 
            if wparam.0 == VK_ESCAPE.0 as usize { 
                DestroyWindow(hwnd); 
            } else if wparam.0 == 'C' as usize {
                // Ctrl+C to copy
                let text_len = GetWindowTextLengthW(hwnd) + 1;
                let mut buf = vec![0u16; text_len as usize];
                GetWindowTextW(hwnd, &mut buf);
                let text = String::from_utf16_lossy(&buf[..text_len as usize - 1]).to_string();
                copy_to_clipboard(&text, hwnd);
            }
            LRESULT(0) 
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            
            // 2. Background
            let brush = CreateSolidBrush(COLORREF(0x00151515)); // Dark Grey
            FillRect(hdc, &rect, brush);
            DeleteObject(brush);

            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FFFFFF));
            
            let text_len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; text_len as usize];
            GetWindowTextW(hwnd, &mut buf);
            
            // --- SMART PADDING LOGIC ---
            // Reduced from 20px to 4px to allow small text selections to fit
            let padding = 4; 
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            let available_w = (width - (padding * 2)).max(1); 
            let available_h = (height - (padding * 2)).max(1);

            // Binary Search for Font Size
            let mut low = 10;
            let mut high = 72;
            let mut optimal_size = 10; // Default to min
            let mut text_h = 0;

            // If the box is really small, allow even smaller fonts if necessary, 
            // but 10 is usually the readbility floor.
            while low <= high {
                let mid = (low + high) / 2;
                let hfont = CreateFontW(mid, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
                SelectObject(hdc, hfont);
                
                let mut calc_rect = RECT { left: 0, top: 0, right: available_w, bottom: 0 };
                let h = DrawTextW(hdc, &mut buf, &mut calc_rect, DT_CALCRECT | DT_WORDBREAK);
                
                DeleteObject(hfont);

                if h <= available_h {
                    optimal_size = mid;
                    text_h = h;
                    low = mid + 1; 
                } else {
                    high = mid - 1; 
                }
            }

            // Draw with Optimal Size & Vertical Centering
            let hfont = CreateFontW(optimal_size, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            SelectObject(hdc, hfont);

            // Center Vertically: Top = padding + (AvailableH - TextH) / 2
            let offset_y = (available_h - text_h) / 2;
            let mut draw_rect = rect;
            draw_rect.left += padding; 
            draw_rect.right -= padding;
            draw_rect.top += padding + offset_y;
            
            DrawTextW(hdc, &mut buf, &mut draw_rect, DT_LEFT | DT_WORDBREAK);
            
            DeleteObject(hfont);
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
