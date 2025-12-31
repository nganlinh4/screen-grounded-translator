use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::process::start_processing_pipeline;
use crate::win_types::{SendHbitmap, SendHwnd};
use crate::{GdiCapture, APP};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

lazy_static::lazy_static! {
    static ref SELECTION_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

// FFI types for Windows Magnification API (loaded dynamically from Magnification.dll)
type MagInitializeFn = unsafe extern "system" fn() -> BOOL;
type MagUninitializeFn = unsafe extern "system" fn() -> BOOL;
type MagSetFullscreenTransformFn = unsafe extern "system" fn(f32, i32, i32) -> BOOL;

static mut MAG_DLL: HMODULE = HMODULE(std::ptr::null_mut());
static mut MAG_INITIALIZE: Option<MagInitializeFn> = None;
static mut MAG_UNINITIALIZE: Option<MagUninitializeFn> = None;
static mut MAG_SET_FULLSCREEN_TRANSFORM: Option<MagSetFullscreenTransformFn> = None;

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
static mut SELECTION_HOOK: HHOOK = HHOOK(std::ptr::null_mut());

// Cached back buffer to avoid per-frame allocations
// Use a 32-bit DIB section for per-pixel alpha support (opaque box on semi-transparent dim)
static mut CACHED_BITMAP: SendHbitmap = SendHbitmap(HBITMAP(std::ptr::null_mut()));
static mut CACHED_BITS: *mut u8 = std::ptr::null_mut();
static mut CACHED_W: i32 = 0;
static mut CACHED_H: i32 = 0;

// --- ZOOM/MAGNIFICATION STATE ---
const ZOOM_STEP: f32 = 0.25;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 4.0;
static mut ZOOM_LEVEL: f32 = 1.0;
static mut ZOOM_CENTER_X: i32 = 0;
static mut ZOOM_CENTER_Y: i32 = 0;
// Alpha override when zoomed (0 = fully transparent dim)
static mut ZOOM_ALPHA_OVERRIDE: Option<u8> = None;
// Track if Windows Magnification API is initialized
static mut MAG_INITIALIZED: bool = false;

#[allow(static_mut_refs)]
unsafe fn load_magnification_api() -> bool {
    // Correctly access static mut using addr_of for Rust 2024 compliance
    let mag_dll = std::ptr::addr_of!(MAG_DLL).read();
    if !mag_dll.is_invalid() {
        return true; // Already loaded
    }

    let dll_name = w!("Magnification.dll");
    let dll = LoadLibraryW(dll_name);

    if let Ok(h) = dll {
        MAG_DLL = h;

        // Get function pointers
        if let Some(init) = GetProcAddress(h, s!("MagInitialize")) {
            MAG_INITIALIZE = Some(std::mem::transmute(init));
        }
        if let Some(uninit) = GetProcAddress(h, s!("MagUninitialize")) {
            MAG_UNINITIALIZE = Some(std::mem::transmute(uninit));
        }
        if let Some(transform) = GetProcAddress(h, s!("MagSetFullscreenTransform")) {
            MAG_SET_FULLSCREEN_TRANSFORM = Some(std::mem::transmute(transform));
        }

        let init_ptr = std::ptr::addr_of!(MAG_INITIALIZE).read();
        let trans_ptr = std::ptr::addr_of!(MAG_SET_FULLSCREEN_TRANSFORM).read();
        return init_ptr.is_some() && trans_ptr.is_some();
    }

    false
}

// Helper to extract bytes from the HBITMAP only for the selected area
unsafe fn extract_crop_from_hbitmap(
    capture: &GdiCapture,
    crop_rect: RECT,
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
    GetDIBits(
        hdc_temp,
        hbm_temp,
        0,
        h as u32,
        Some(buffer.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

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
        if SELECTION_OVERLAY_ACTIVE
            && !std::ptr::addr_of!(SELECTION_OVERLAY_HWND)
                .read()
                .is_invalid()
        {
            let _ = PostMessageW(
                Some(SELECTION_OVERLAY_HWND.0),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
            true
        } else {
            false
        }
    }
}

#[allow(static_mut_refs)]
pub fn show_selection_overlay(preset_idx: usize) {
    unsafe {
        CURRENT_PRESET_IDX = preset_idx;
        SELECTION_OVERLAY_ACTIVE = true;
        CURRENT_ALPHA = 0;
        IS_FADING_OUT = false;
        IS_DRAGGING = false;

        // Reset zoom state
        ZOOM_LEVEL = 1.0;
        ZOOM_CENTER_X = 0;
        ZOOM_CENTER_Y = 0;
        ZOOM_ALPHA_OVERRIDE = None;

        SELECTION_ABORT_SIGNAL.store(false, Ordering::SeqCst);
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
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        SELECTION_OVERLAY_HWND = SendHwnd(hwnd);

        // Install Hook
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(selection_hook_proc),
            Some(GetModuleHandleW(None).unwrap().into()),
            0,
        );
        if let Ok(h) = hook {
            SELECTION_HOOK = h;
        }

        // Initial sync to set alpha 0
        sync_layered_window_contents(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let _ = SetTimer(Some(hwnd), FADE_TIMER_ID, 16, None);

        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
                if msg.message == WM_QUIT {
                    break;
                }
            }
            if msg.message == WM_QUIT {
                break;
            }

            if SELECTION_ABORT_SIGNAL.load(Ordering::SeqCst) {
                // Trigger graceful close (fade out)
                let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
                SELECTION_ABORT_SIGNAL.store(false, Ordering::SeqCst);
            }

            let _ = WaitMessage();
        }

        // Uninstall Hook
        let hook = std::ptr::addr_of!(SELECTION_HOOK).read();
        if !hook.is_invalid() {
            let _ = UnhookWindowsHookEx(hook);
            SELECTION_HOOK = HHOOK(std::ptr::null_mut());
        }

        SELECTION_OVERLAY_ACTIVE = false;
        SELECTION_OVERLAY_HWND = SendHwnd::default();
    }
}

unsafe extern "system" fn selection_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize {
            if kbd.vkCode == VK_ESCAPE.0 as u32 {
                SELECTION_ABORT_SIGNAL.store(true, Ordering::SeqCst);
                let hwnd = std::ptr::addr_of!(SELECTION_OVERLAY_HWND).read().0;
                if !hwnd.is_invalid() {
                    // Wake the message loop
                    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                }
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

#[allow(static_mut_refs)]
unsafe extern "system" fn selection_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            if !IS_FADING_OUT {
                IS_DRAGGING = true;
                let _ = GetCursorPos(std::ptr::addr_of_mut!(START_POS));
                CURR_POS = START_POS;
                SetCapture(hwnd);
                sync_layered_window_contents(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                let _ = GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
                // Force immediate repaint for smoothness
                sync_layered_window_contents(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if !IS_FADING_OUT && !IS_DRAGGING {
                // Extract wheel delta from wparam high word (signed)
                let delta = ((wparam.0 >> 16) as i16) as i32;

                // Get cursor position for zoom center
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);

                let old_zoom = ZOOM_LEVEL;

                if delta > 0 {
                    // Scroll up = zoom in
                    ZOOM_LEVEL = (ZOOM_LEVEL + ZOOM_STEP).min(MAX_ZOOM);
                    ZOOM_CENTER_X = cursor.x;
                    ZOOM_CENTER_Y = cursor.y;
                } else if delta < 0 {
                    // Scroll down = zoom out
                    ZOOM_LEVEL = (ZOOM_LEVEL - ZOOM_STEP).max(MIN_ZOOM);
                }

                // Apply Windows Magnification API for real screen zoom
                if (ZOOM_LEVEL - old_zoom).abs() > 0.001 {
                    // Initialize magnification if not already done
                    if !MAG_INITIALIZED {
                        if load_magnification_api() {
                            if let Some(init_fn) = MAG_INITIALIZE {
                                if init_fn().as_bool() {
                                    MAG_INITIALIZED = true;
                                }
                            }
                        }
                    }

                    if MAG_INITIALIZED {
                        // Apply full-screen magnification centered on cursor
                        if let Some(transform_fn) = MAG_SET_FULLSCREEN_TRANSFORM {
                            if ZOOM_LEVEL > 1.0 {
                                let screen_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                                let screen_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                                let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                                let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);

                                // Calculate the top-left offset to center the magnified view on the cursor
                                let view_w = (screen_w as f32 / ZOOM_LEVEL) as i32;
                                let view_h = (screen_h as f32 / ZOOM_LEVEL) as i32;

                                let mut off_x = ZOOM_CENTER_X - view_w / 2;
                                let mut off_y = ZOOM_CENTER_Y - view_h / 2;

                                // Clamp to virtual screen bounds
                                off_x = off_x.max(screen_x).min(screen_x + screen_w - view_w);
                                off_y = off_y.max(screen_y).min(screen_y + screen_h - view_h);

                                let _ = transform_fn(ZOOM_LEVEL, off_x, off_y);
                            } else {
                                // Reset screen transform completely at 1.0 zoom
                                let _ = transform_fn(1.0, 0, 0);
                            }
                        }
                    }

                    sync_layered_window_contents(hwnd);
                }
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
                        guard
                            .config
                            .presets
                            .get(CURRENT_PRESET_IDX)
                            .map(|p| p.is_master)
                            .unwrap_or(false)
                    };

                    // For MASTER presets, show the preset wheel first
                    let final_preset_idx = if is_master {
                        // Get cursor position for wheel center
                        let mut cursor_pos = POINT::default();
                        let _ = GetCursorPos(&mut cursor_pos);

                        // Hide selection overlay temporarily while showing wheel
                        // Hide selection overlay temporarily while showing wheel
                        ZOOM_ALPHA_OVERRIDE = Some(60);
                        sync_layered_window_contents(hwnd);

                        // Show preset wheel - this blocks until user makes selection
                        let selected =
                            super::preset_wheel::show_preset_wheel("image", None, cursor_pos);

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
                            let capture = guard
                                .screenshot_handle
                                .as_ref()
                                .expect("Screenshot handle missing");
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
                    // Reset magnification instantly
                    unsafe {
                        if MAG_INITIALIZED {
                            if let Some(transform_fn) = MAG_SET_FULLSCREEN_TRANSFORM {
                                let _ = transform_fn(1.0, 0, 0);
                            }
                        }
                    }
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
                        CURRENT_ALPHA = (CURRENT_ALPHA as u16 + FADE_STEP as u16)
                            .min(TARGET_OPACITY as u16)
                            as u8;
                        changed = true;
                    } else {
                        let _ = KillTimer(Some(hwnd), FADE_TIMER_ID);
                    }
                }

                if changed {
                    sync_layered_window_contents(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            sync_layered_window_contents(hwnd);
            let _ = EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // Handle erasing to prevent flicker
        WM_CLOSE => {
            if !IS_FADING_OUT {
                IS_FADING_OUT = true;
                // Reset magnification instantly
                unsafe {
                    if MAG_INITIALIZED {
                        if let Some(transform_fn) = MAG_SET_FULLSCREEN_TRANSFORM {
                            let _ = transform_fn(1.0, 0, 0);
                        }
                    }
                }
                let _ = KillTimer(Some(hwnd), FADE_TIMER_ID);
                SetTimer(Some(hwnd), FADE_TIMER_ID, 16, None);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Reset magnification before closing
            unsafe {
                if MAG_INITIALIZED {
                    if let Some(transform_fn) = MAG_SET_FULLSCREEN_TRANSFORM {
                        let _ = transform_fn(1.0, 0, 0);
                    }
                    if let Some(uninit_fn) = MAG_UNINITIALIZE {
                        let _ = uninit_fn();
                    }
                    MAG_INITIALIZED = false;
                }
            }

            // Cleanup cached back buffer resources
            unsafe {
                if !std::ptr::addr_of!(CACHED_BITMAP).read().is_invalid() {
                    let _ = DeleteObject(CACHED_BITMAP.0.into());
                    CACHED_BITMAP = SendHbitmap::default();
                    CACHED_BITS = std::ptr::null_mut();
                }
                CACHED_W = 0;
                CACHED_H = 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// New high-performance renderer using UpdateLayeredWindow
/// This allows us to have an OPAQUE white box even when the dim background is TRANSPARENT
#[allow(static_mut_refs)]
unsafe fn sync_layered_window_contents(hwnd: HWND) {
    let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

    if width <= 0 || height <= 0 {
        return;
    }

    // 1. Prepare/Cache 32-bit DIB Context
    if std::ptr::addr_of!(CACHED_BITMAP).read().is_invalid()
        || CACHED_W != width
        || CACHED_H != height
    {
        if !std::ptr::addr_of!(CACHED_BITMAP).read().is_invalid() {
            let _ = DeleteObject(CACHED_BITMAP.0.into());
            CACHED_BITS = std::ptr::null_mut();
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let hdc_screen = GetDC(None);
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(Some(hdc_screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None, hdc_screen);

        if let Ok(h) = hbm {
            CACHED_BITMAP = SendHbitmap(h);
            CACHED_BITS = bits as *mut u8;
            CACHED_W = width;
            CACHED_H = height;
        } else {
            return;
        }
    }

    // 2. Draw using GDI to the DIB
    let hdc_screen = GetDC(None);
    let mem_dc = CreateCompatibleDC(Some(hdc_screen));
    let old_bmp = SelectObject(mem_dc, CACHED_BITMAP.0.into());

    // OPTIMIZATION: Clear background directly via memory fill (much faster than GDI)
    let effective_alpha = if let Some(zoom_alpha) = ZOOM_ALPHA_OVERRIDE {
        zoom_alpha.min(CURRENT_ALPHA)
    } else {
        CURRENT_ALPHA
    };

    let total_pixels = (width * height) as usize;
    let pixels_u32 = std::slice::from_raw_parts_mut(CACHED_BITS as *mut u32, total_pixels);

    // Fill with pre-multiplied alpha black: (0, 0, 0, alpha)
    let bg_val = (effective_alpha as u32) << 24;
    pixels_u32.fill(bg_val);

    // Draw the selection rectangle
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
            // Draw pure white box (GDI will set color but likely alpha 0)
            let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00FFFFFF));
            let old_pen = SelectObject(mem_dc, pen.into());
            let null_brush = GetStockObject(NULL_BRUSH);
            let old_brush = SelectObject(mem_dc, null_brush);

            let _ = RoundRect(mem_dc, r.left, r.top, r.right, r.bottom, 12, 12);

            SelectObject(mem_dc, old_brush);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(pen.into());

            // 3. SECURING ALPHA: Only iterate over the bounding area of the selection
            // This is much faster than processing the whole screen on every move
            let b_left = (r.left - 5).max(0);
            let b_top = (r.top - 5).max(0);
            let b_right = (r.right + 5).min(width);
            let b_bottom = (r.bottom + 5).min(height);

            for y in b_top..b_bottom {
                let row_start = (y * width + b_left) as usize;
                let row_end = (y * width + b_right) as usize;
                if row_start < pixels_u32.len() && row_end <= pixels_u32.len() {
                    for p in &mut pixels_u32[row_start..row_end] {
                        if (*p & 0x00FFFFFF) > 0x0A0A0A {
                            *p = 0xFFFFFFFF; // Make the white box opaque
                        }
                    }
                }
            }
        }
    }

    // 4. Update the Layered Window
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255, // Use per-pixel alpha from the bitmap
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    let screen_pos = POINT {
        x: GetSystemMetrics(SM_XVIRTUALSCREEN),
        y: GetSystemMetrics(SM_YVIRTUALSCREEN),
    };
    let wnd_size = SIZE {
        cx: width,
        cy: height,
    };
    let src_pos = POINT { x: 0, y: 0 };

    let _ = UpdateLayeredWindow(
        hwnd,
        Some(hdc_screen),
        Some(&screen_pos),
        Some(&wnd_size),
        Some(mem_dc),
        Some(&src_pos),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    // Cleanup DC
    SelectObject(mem_dc, old_bmp);
    let _ = DeleteDC(mem_dc);
    ReleaseDC(None, hdc_screen);
}
