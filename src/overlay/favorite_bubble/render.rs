use super::state::*;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, UpdateLayeredWindow, ULW_ALPHA};

pub fn update_bubble_visual(hwnd: HWND) {
    // Sync theme state
    let is_dark = crate::overlay::is_dark_mode();
    LAST_THEME_IS_DARK.store(is_dark, Ordering::SeqCst);

    unsafe {
        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));

        // Create 32-bit ARGB bitmap
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: BUBBLE_SIZE,
                biHeight: -BUBBLE_SIZE, // Top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm =
            CreateDIBSection(Some(hdc_mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        let old_bm = SelectObject(hdc_mem, hbm.into());

        if !bits.is_null() {
            // Draw directly to pixel buffer with anti-aliasing
            let pixels = std::slice::from_raw_parts_mut(
                bits as *mut u32,
                (BUBBLE_SIZE * BUBBLE_SIZE) as usize,
            );
            let is_hovered = IS_HOVERED.load(Ordering::SeqCst);
            let is_expanded = IS_EXPANDED.load(Ordering::SeqCst);

            draw_bubble_pixels(pixels, BUBBLE_SIZE, is_hovered || is_expanded);
        }

        // Update layered window
        let size = SIZE {
            cx: BUBBLE_SIZE,
            cy: BUBBLE_SIZE,
        };
        let pt_src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let pt_dst = POINT {
            x: rect.left,
            y: rect.top,
        };

        let _ = UpdateLayeredWindow(
            hwnd,
            Some(hdc_screen),
            Some(&pt_dst),
            Some(&size),
            Some(hdc_mem),
            Some(&pt_src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        let _ = SelectObject(hdc_mem, old_bm);
        let _ = DeleteObject(hbm.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
    }
}

fn draw_bubble_pixels(pixels: &mut [u32], size: i32, _is_active: bool) {
    // Use animated opacity for smooth transitions
    let opacity = CURRENT_OPACITY.load(Ordering::SeqCst);

    // Select icon based on theme
    let icon_data = if LAST_THEME_IS_DARK.load(Ordering::SeqCst) {
        &*ICON_RGBA
    } else {
        &*ICON_LIGHT_RGBA
    };

    // Use embedded icon if available
    if !icon_data.is_empty() {
        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) as usize;
                let src_idx = idx * 4; // RGBA

                if src_idx + 3 < icon_data.len() {
                    let r = icon_data[src_idx] as u32;
                    let g = icon_data[src_idx + 1] as u32;
                    let b = icon_data[src_idx + 2] as u32;
                    let a = icon_data[src_idx + 3] as u32;

                    // Apply opacity multiplier
                    let final_a = (a * opacity as u32) / 255;

                    // Premultiplied alpha for UpdateLayeredWindow
                    let r_pm = (r * final_a) / 255;
                    let g_pm = (g * final_a) / 255;
                    let b_pm = (b * final_a) / 255;

                    // BGRA format for Windows (but stored as ARGB in u32)
                    pixels[idx] = (final_a << 24) | (r_pm << 16) | (g_pm << 8) | b_pm;
                } else {
                    pixels[idx] = 0;
                }
            }
        }
    } else {
        // Fallback: draw a simple purple circle if icon not available
        let center = size as f32 / 2.0;
        let radius = center - 2.0;

        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) as usize;
                let fx = x as f32 + 0.5;
                let fy = y as f32 + 0.5;

                let dx = fx - center;
                let dy = fy - center;
                let dist = (dx * dx + dy * dy).sqrt();

                if dist <= radius {
                    let a = opacity as u32;
                    let r = (130u32 * a) / 255;
                    let g = (80u32 * a) / 255;
                    let b = (200u32 * a) / 255;
                    pixels[idx] = (a << 24) | (r << 16) | (g << 8) | b;
                } else {
                    pixels[idx] = 0;
                }
            }
        }
    }
}
