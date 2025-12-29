use crate::APP;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, Once,
};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Content type for the badge
#[derive(Clone)]
pub enum CopiedContent {
    Text(String),
    Image,
}

struct AutoCopyBadgeState {
    hwnd: HWND,
    current_alpha: i32,
    start_time: std::time::Instant,
    cached_bitmap: HBITMAP,
    cached_bits: *mut u32,
    cached_font: HFONT,
    cached_small_font: HFONT,
    cached_lang: Option<String>,
    content: Option<CopiedContent>,
}
unsafe impl Send for AutoCopyBadgeState {}

static BADGE_STATE: Mutex<AutoCopyBadgeState> = Mutex::new(AutoCopyBadgeState {
    hwnd: HWND(std::ptr::null_mut()),
    current_alpha: 0,
    start_time: unsafe { std::mem::zeroed() },
    cached_bitmap: HBITMAP(std::ptr::null_mut()),
    cached_bits: std::ptr::null_mut(),
    cached_font: HFONT(std::ptr::null_mut()),
    cached_small_font: HFONT(std::ptr::null_mut()),
    cached_lang: None,
    content: None,
});

static REGISTER_BADGE_CLASS: Once = Once::new();

lazy_static::lazy_static! {
    static ref BADGE_ABORT_SIGNAL: AtomicBool = AtomicBool::new(false);
}

const BADGE_DURATION_MS: u128 = 1500; // 1.5 seconds
const BADGE_WIDTH: i32 = 350;
const BADGE_HEIGHT: i32 = 54;

// Green color theme
const GREEN_BG: u32 = 0x1A3D2A; // Dark green background (BGR format for GDI)
const GREEN_GLOW: u32 = 0x4ADE80; // Bright green glow (RGB)
const GREEN_BORDER: u32 = 0x22C55E; // Green border

#[allow(dead_code)]
pub fn is_active() -> bool {
    !BADGE_STATE.lock().unwrap().hwnd.is_invalid()
}

/// Show the auto-copy badge with text content snippet
pub fn show_auto_copy_badge_text(text: &str) {
    let content = CopiedContent::Text(text.to_string());
    std::thread::spawn(move || {
        show_badge_internal(content);
    });
}

/// Show the auto-copy badge for image copy
pub fn show_auto_copy_badge_image() {
    std::thread::spawn(move || {
        show_badge_internal(CopiedContent::Image);
    });
}

fn show_badge_internal(content: CopiedContent) {
    unsafe {
        // Check and Init
        {
            let mut state = BADGE_STATE.lock().unwrap();
            if !state.hwnd.is_invalid() {
                // Already showing - just reset the timer and update content
                state.start_time = std::time::Instant::now();
                state.current_alpha = 0;
                state.content = Some(content);
                // Invalidate bitmap cache to redraw with new content
                if !state.cached_bitmap.is_invalid() {
                    let _ = DeleteObject(state.cached_bitmap.into());
                    state.cached_bitmap = HBITMAP::default();
                }
                state.cached_bits = std::ptr::null_mut();
                return;
            }

            state.current_alpha = 0;
            state.start_time = std::time::Instant::now();
            state.content = Some(content);
            BADGE_ABORT_SIGNAL.store(false, Ordering::SeqCst);

            // Cleanup old cache
            if !state.cached_bitmap.is_invalid() {
                let _ = DeleteObject(state.cached_bitmap.into());
                state.cached_bitmap = HBITMAP::default();
            }
            if !state.cached_font.is_invalid() {
                let _ = DeleteObject(state.cached_font.into());
                state.cached_font = HFONT(std::ptr::null_mut());
            }
            if !state.cached_small_font.is_invalid() {
                let _ = DeleteObject(state.cached_small_font.into());
                state.cached_small_font = HFONT(std::ptr::null_mut());
            }
            state.cached_bits = std::ptr::null_mut();
            state.cached_lang = None;
        }

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_AutoCopyBadge");

        REGISTER_BADGE_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(badge_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassW(&wc);
        });

        // Position at bottom-center of screen
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_w - BADGE_WIDTH) / 2;
        let y = screen_h - BADGE_HEIGHT - 100; // 100px from bottom

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT AutoCopy Badge"),
            WS_POPUP,
            x,
            y,
            BADGE_WIDTH,
            BADGE_HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        {
            BADGE_STATE.lock().unwrap().hwnd = hwnd;
        }

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut msg = MSG::default();

        // Game Loop for smooth animation
        loop {
            // 1. Process all pending messages
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                if msg.message == WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if msg.message == WM_QUIT {
                break;
            }

            // 2. Check Abort Signal or timeout
            let should_close = {
                let state = BADGE_STATE.lock().unwrap();
                let elapsed = state.start_time.elapsed().as_millis();
                BADGE_ABORT_SIGNAL.load(Ordering::SeqCst) || elapsed > BADGE_DURATION_MS + 300
            };

            if should_close {
                let _ = DestroyWindow(hwnd);
                break;
            }

            // 3. Update Animation
            let current_alpha = {
                let mut state = BADGE_STATE.lock().unwrap();
                let elapsed = state.start_time.elapsed().as_millis();

                // Fade in/out logic
                if elapsed < 150 {
                    // Fade in (first 150ms)
                    state.current_alpha = ((elapsed as f32 / 150.0) * 255.0) as i32;
                    state.current_alpha = state.current_alpha.clamp(0, 255);
                } else if elapsed > BADGE_DURATION_MS - 200 {
                    // Fade out (last 200ms)
                    let fade_elapsed = elapsed - (BADGE_DURATION_MS - 200);
                    state.current_alpha = ((1.0 - fade_elapsed as f32 / 300.0) * 255.0) as i32;
                    state.current_alpha = state.current_alpha.clamp(0, 255);
                } else {
                    // Fully visible
                    state.current_alpha = 255;
                }

                state.current_alpha as u8
            };

            // 4. Render
            paint_badge_window(hwnd, BADGE_WIDTH, BADGE_HEIGHT, current_alpha, x, y);

            // 5. Sync with DWM for smoothness
            let _ = DwmFlush();
        }

        // Cleanup cache on exit
        {
            let mut state = BADGE_STATE.lock().unwrap();

            if !state.cached_bitmap.is_invalid() {
                let _ = DeleteObject(state.cached_bitmap.into());
                state.cached_bitmap = HBITMAP::default();
            }
            if !state.cached_font.is_invalid() {
                let _ = DeleteObject(state.cached_font.into());
                state.cached_font = HFONT(std::ptr::null_mut());
            }
            if !state.cached_small_font.is_invalid() {
                let _ = DeleteObject(state.cached_small_font.into());
                state.cached_small_font = HFONT(std::ptr::null_mut());
            }
            state.cached_bits = std::ptr::null_mut();
            state.content = None;
            state.hwnd = HWND::default();
        }
    }
}

unsafe extern "system" fn badge_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLOSE => {
            BADGE_ABORT_SIGNAL.store(true, Ordering::SeqCst);
            let _ = DestroyWindow(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_badge_window(hwnd: HWND, width: i32, height: i32, alpha: u8, x: i32, y: i32) {
    if alpha == 0 {
        return;
    }

    let screen_dc = GetDC(None);
    let mem_dc = CreateCompatibleDC(Some(screen_dc));

    let mut state = BADGE_STATE.lock().unwrap();

    // Cached lang check
    if state.cached_lang.is_none() {
        let app = APP.lock().unwrap();
        state.cached_lang = Some(app.config.ui_language.clone());
    }

    if state.cached_bitmap.is_invalid() {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(bmp) =
            CreateDIBSection(Some(screen_dc), &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0)
        {
            state.cached_bitmap = bmp;
            state.cached_bits = p_bits as *mut u32;
        }
    }

    // Main font for title
    if state.cached_font.is_invalid() {
        state.cached_font = CreateFontW(
            20,
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(DEFAULT_CHARSET.0 as u8),
            FONT_OUTPUT_PRECISION(OUT_DEFAULT_PRECIS.0 as u8),
            FONT_CLIP_PRECISION(CLIP_DEFAULT_PRECIS.0 as u8),
            FONT_QUALITY(CLEARTYPE_QUALITY.0 as u8),
            std::mem::transmute((VARIABLE_PITCH.0 | FF_SWISS.0) as u32),
            w!("Google Sans Flex"),
        );
    }

    // Smaller font for snippet
    if state.cached_small_font.is_invalid() {
        state.cached_small_font = CreateFontW(
            16,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            FONT_CHARSET(DEFAULT_CHARSET.0 as u8),
            FONT_OUTPUT_PRECISION(OUT_DEFAULT_PRECIS.0 as u8),
            FONT_CLIP_PRECISION(CLIP_DEFAULT_PRECIS.0 as u8),
            FONT_QUALITY(CLEARTYPE_QUALITY.0 as u8),
            std::mem::transmute((VARIABLE_PITCH.0 | FF_SWISS.0) as u32),
            w!("Google Sans Flex"),
        );
    }

    let old_bitmap = SelectObject(mem_dc, state.cached_bitmap.into());

    if !state.cached_bits.is_null() {
        let pixels = std::slice::from_raw_parts_mut(state.cached_bits, (width * height) as usize);
        let bx = width as f32 / 2.0;
        let by = height as f32 / 2.0;

        let corner_radius = 14.0;
        let glow_size = 4.0;

        // Extract RGB from green colors
        let bg_r = ((GREEN_BG >> 16) & 0xFF) as f32;
        let bg_g = ((GREEN_BG >> 8) & 0xFF) as f32;
        let bg_b = (GREEN_BG & 0xFF) as f32;

        let glow_r = ((GREEN_GLOW >> 16) & 0xFF) as f32;
        let glow_g = ((GREEN_GLOW >> 8) & 0xFF) as f32;
        let glow_b = (GREEN_GLOW & 0xFF) as f32;

        for py_idx in 0..height {
            let py = py_idx as f32 - by;

            for px_idx in 0..width {
                let idx = (py_idx * width + px_idx) as usize;
                let px = px_idx as f32 - bx;

                let d = crate::overlay::paint_utils::sd_rounded_box(
                    px,
                    py,
                    bx - 6.0,
                    by - 6.0,
                    corner_radius,
                );

                let mut final_r: f32;
                let mut final_g: f32;
                let mut final_b: f32;
                let mut final_alpha: f32;

                if d < -1.0 {
                    // Inside the badge - green background
                    final_r = bg_r;
                    final_g = bg_g;
                    final_b = bg_b;
                    final_alpha = 0.92;
                } else if d < 1.0 {
                    // Edge with anti-aliasing
                    let t = (d + 1.0) / 2.0;
                    let blend = t * t * (3.0 - 2.0 * t);

                    // Blend between fill and border/glow
                    let border_r = ((GREEN_BORDER >> 16) & 0xFF) as f32;
                    let border_g = ((GREEN_BORDER >> 8) & 0xFF) as f32;
                    let border_b = (GREEN_BORDER & 0xFF) as f32;

                    final_r = bg_r * (1.0 - blend) + border_r * blend;
                    final_g = bg_g * (1.0 - blend) + border_g * blend;
                    final_b = bg_b * (1.0 - blend) + border_b * blend;
                    final_alpha = 0.92 * (1.0 - blend) + 0.8 * blend;
                } else if d < glow_size {
                    // Outer glow
                    let glow_t = (d - 1.0) / (glow_size - 1.0);
                    let glow_intensity = (1.0 - glow_t).powi(2);

                    final_r = glow_r;
                    final_g = glow_g;
                    final_b = glow_b;
                    final_alpha = glow_intensity * 0.5;
                } else {
                    // Transparent outside
                    pixels[idx] = 0;
                    continue;
                }

                // Premultiply alpha
                let a = (final_alpha * 255.0) as u32;
                let r = (final_r * final_alpha) as u32;
                let g = (final_g * final_alpha) as u32;
                let b = (final_b * final_alpha) as u32;
                pixels[idx] = (a << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }

    SetBkMode(mem_dc, TRANSPARENT);

    // Get localized strings
    let locale = crate::gui::locale::LocaleText::get(state.cached_lang.as_ref().unwrap());
    let title = locale.auto_copied_badge;

    // Get snippet text based on content
    let snippet = match &state.content {
        Some(CopiedContent::Text(text)) => {
            // Truncate text to ~40 chars for display
            let clean_text = text.replace('\n', " ").replace('\r', "");
            if clean_text.chars().count() > 40 {
                let truncated: String = clean_text.chars().take(37).collect();
                format!("\"{}...\"", truncated)
            } else {
                format!("\"{}\"", clean_text)
            }
        }
        Some(CopiedContent::Image) => {
            // Localized "Image copied" message from locale system
            locale.auto_copied_image_badge.to_string()
        }
        None => String::new(),
    };

    // Draw checkmark + title (top line)
    let title_with_check = format!("✓ {}", title);
    let old_font = SelectObject(mem_dc, state.cached_font.into());
    SetTextColor(mem_dc, COLORREF(0x00FFFFFF)); // White text

    let mut title_w = crate::overlay::utils::to_wstring(&title_with_check);
    let mut title_rect = RECT {
        left: 10,
        top: 6,
        right: width - 10,
        bottom: height / 2 + 4,
    };
    DrawTextW(
        mem_dc,
        &mut title_w,
        &mut title_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    // Draw snippet (bottom line) in smaller, slightly dimmer text
    let _ = SelectObject(mem_dc, state.cached_small_font.into());
    SetTextColor(mem_dc, COLORREF(0x00BBBBBB)); // Slightly dimmer white

    let mut snippet_w = crate::overlay::utils::to_wstring(&snippet);
    let mut snippet_rect = RECT {
        left: 10,
        top: height / 2 - 2,
        right: width - 10,
        bottom: height - 4,
    };
    DrawTextW(
        mem_dc,
        &mut snippet_w,
        &mut snippet_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    // Fix alpha channel for text pixels
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
            if max_c > a {
                *p = (max_c << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }

    let pt_src = POINT { x: 0, y: 0 };
    let pt_dst = POINT { x, y };

    let size = SIZE {
        cx: width,
        cy: height,
    };
    let mut bl = BLENDFUNCTION::default();
    bl.BlendOp = AC_SRC_OVER as u8;
    bl.SourceConstantAlpha = alpha;
    bl.AlphaFormat = AC_SRC_ALPHA as u8;
    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&pt_dst),
        Some(&size),
        Some(mem_dc),
        Some(&pt_src),
        COLORREF(0),
        Some(&bl),
        ULW_ALPHA,
    );

    let _ = SelectObject(mem_dc, old_font.into());
    let _ = SelectObject(mem_dc, old_bitmap.into());
    let _ = DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);
}
