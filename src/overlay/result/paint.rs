use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;
use std::mem::size_of;
use crate::overlay::broom_assets::{render_procedural_broom, BroomRenderParams, BROOM_W, BROOM_H};
use super::state::{WINDOW_STATES, AnimationMode};

pub fn create_bitmap_from_pixels(pixels: &[u32], w: i32, h: i32) -> HBITMAP {
    unsafe {
        let hdc = GetDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, 
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(pixels.as_ptr() as *const u8, bits as *mut u8, pixels.len() * 4);
        }
        
        ReleaseDC(None, hdc);
        hbm
    }
}

pub fn paint_window(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bitmap = CreateCompatibleBitmap(hdc, width, height);
        let old_bitmap = SelectObject(mem_dc, mem_bitmap);

        // --- STEP 1: SNAPSHOT STATE ---
        let (
            bg_color, is_hovered, copy_success, broom_data, particles, 
            mut cached_bm, mut font_size, mut cache_dirty
        ) = {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                
                if state.last_w != width || state.last_h != height {
                    state.font_cache_dirty = true;
                    state.last_w = width;
                    state.last_h = height;
                }

                let particles_vec: Vec<(f32, f32, f32, f32, u32)> = state.physics.particles.iter()
                    .map(|p| (p.x, p.y, p.life, p.size, p.color)).collect();
                
                let show_broom = (state.is_hovered && !state.on_copy_btn) || state.physics.mode == AnimationMode::Smashing;
                let broom_info = if show_broom {
                     Some((
                         state.physics.x, 
                         state.physics.y, 
                         BroomRenderParams {
                            tilt_angle: state.physics.current_tilt,
                            squish: state.physics.squish_factor,
                            bend: state.physics.bristle_bend,
                            opacity: 1.0,
                        }
                     ))
                } else { None };

                (
                    state.bg_color, state.is_hovered, state.copy_success, broom_info, particles_vec,
                    state.content_bitmap, state.cached_font_size, state.font_cache_dirty
                )
            } else {
                (0x00222222, false, false, None, Vec::new(), HBITMAP(0), 72, true)
            }
        };

        // --- STEP 2: SMART FONT UPDATE ---
        if cache_dirty || cached_bm.0 == 0 {
            if cached_bm.0 != 0 { DeleteObject(cached_bm); }

            cached_bm = CreateCompatibleBitmap(hdc, width, height);
            let cache_dc = CreateCompatibleDC(hdc);
            let old_cache_bm = SelectObject(cache_dc, cached_bm);

            let dark_brush = CreateSolidBrush(COLORREF(bg_color));
            let fill_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(cache_dc, &fill_rect, dark_brush);
            DeleteObject(dark_brush);

            SetBkMode(cache_dc, TRANSPARENT);
            SetTextColor(cache_dc, COLORREF(0x00FFFFFF));

            let text_len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; text_len as usize];
            GetWindowTextW(hwnd, &mut buf);

            let padding = 8; // Slightly more padding
            let available_w = (width - (padding * 2)).max(1);
            let available_h = (height - (padding * 2)).max(1);

            // === OPTIMIZATION: BIDIRECTIONAL LINEAR SEARCH ===
            // 1. Try current size.
            // 2. If it overflows -> Shrink until fit.
            // 3. If it has extra space -> Grow until limit.
            
            let mut test_size = font_size;
            if test_size < 10 { test_size = 72; } // Reset to max if invalid
            
            // Helper function to measure text height at a given font size
            let mut measure_text = |fs: i32| -> i32 {
                 let hfont = CreateFontW(fs, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
                 let old_font = SelectObject(cache_dc, hfont);
                 let mut calc_rect = RECT { left: 0, top: 0, right: available_w, bottom: 0 };
                 let h = DrawTextW(cache_dc, &mut buf, &mut calc_rect, DT_CALCRECT | DT_WORDBREAK);
                 SelectObject(cache_dc, old_font);
                 DeleteObject(hfont);
                 h
            };

            let current_h = measure_text(test_size);

            if current_h > available_h {
                // Too big? Shrink down.
                while test_size > 12 {
                    test_size -= 2; // Step by 2 for speed
                    if measure_text(test_size) <= available_h { break; }
                }
            } else {
                // Too small? Grow up.
                // We only grow if it's SIGNIFICANTLY smaller to avoid jitter
                while test_size < 72 {
                    let next_size = test_size + 2;
                    if measure_text(next_size) > available_h { break; }
                    test_size = next_size;
                }
            }
            
            font_size = test_size;

            // Draw Final Text
            let hfont = CreateFontW(font_size, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            let old_font = SelectObject(cache_dc, hfont);

            // Recalc final rect for centering
            let mut measure_rect = RECT { left: 0, top: 0, right: available_w, bottom: 0 };
            let text_h = DrawTextW(cache_dc, &mut buf, &mut measure_rect, DT_CALCRECT | DT_WORDBREAK);
            let offset_y = (available_h - text_h) / 2;
            let mut draw_rect = RECT { left: padding, top: padding + offset_y, right: width - padding, bottom: height - padding };

            DrawTextW(cache_dc, &mut buf, &mut draw_rect as *mut _, DT_LEFT | DT_WORDBREAK);

            SelectObject(cache_dc, old_font);
            DeleteObject(hfont);
            
            SelectObject(cache_dc, old_cache_bm);
            DeleteDC(cache_dc);

            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                state.content_bitmap = cached_bm;
                state.cached_font_size = font_size;
                state.font_cache_dirty = false;
            }
        }

        // --- STEP 3: BLIT STATIC CONTENT ---
        if cached_bm.0 != 0 {
            let cache_dc = CreateCompatibleDC(hdc);
            let old_cbm = SelectObject(cache_dc, cached_bm);
            BitBlt(mem_dc, 0, 0, width, height, cache_dc, 0, 0, SRCCOPY).ok();
            SelectObject(cache_dc, old_cbm);
            DeleteDC(cache_dc);
        }

        // --- STEP 4: DYNAMIC OVERLAY (Broom/Particles) ---
        // Broom
        let broom_bitmap_data = if let Some((bx, by, params)) = broom_data {
            let pixels = render_procedural_broom(params);
            let hbm = create_bitmap_from_pixels(&pixels, BROOM_W, BROOM_H);
            Some((bx, by, hbm))
        } else { None };

        // Particles
        for (d_x, d_y, life, size, col) in particles {
            let cur_size = (size * life).ceil() as i32;
            if cur_size > 0 {
                let p_rect = RECT { left: d_x as i32, top: d_y as i32, right: d_x as i32 + cur_size, bottom: d_y as i32 + cur_size };
                let r = (col >> 16) & 0xFF;
                let g = (col >> 8) & 0xFF;
                let b = col & 0xFF;
                let cr = (b << 16) | (g << 8) | r;
                let brush = CreateSolidBrush(COLORREF(cr));
                FillRect(mem_dc, &p_rect, brush);
                DeleteObject(brush);
            }
        }

        // Copy Button (Hover)
        if is_hovered {
             let btn_size = 24;
             let btn_rect = RECT { left: width - btn_size, top: height - btn_size, right: width, bottom: height };
             let btn_brush = CreateSolidBrush(COLORREF(0x00444444));
             FillRect(mem_dc, &btn_rect, btn_brush);
             DeleteObject(btn_brush);
             let icon_pen = if copy_success { CreatePen(PS_SOLID, 2, COLORREF(0x0000FF00)) } else { CreatePen(PS_SOLID, 2, COLORREF(0x00AAAAAA)) };
             let old_pen = SelectObject(mem_dc, icon_pen);
             if copy_success {
                 MoveToEx(mem_dc, btn_rect.left + 6, btn_rect.top + 12, None);
                 LineTo(mem_dc, btn_rect.left + 10, btn_rect.top + 16);
                 LineTo(mem_dc, btn_rect.left + 18, btn_rect.top + 8);
             } else {
                 Rectangle(mem_dc, btn_rect.left + 6, btn_rect.top + 6, btn_rect.right - 6, btn_rect.bottom - 4);
                 Rectangle(mem_dc, btn_rect.left + 9, btn_rect.top + 4, btn_rect.right - 9, btn_rect.top + 8);
             }
             SelectObject(mem_dc, old_pen);
             DeleteObject(icon_pen);
        }

        if let Some((px, py, hbm)) = broom_bitmap_data {
             if hbm.0 != 0 {
                let broom_dc = CreateCompatibleDC(hdc);
                let old_hbm_broom = SelectObject(broom_dc, hbm);
                let mut bf = BLENDFUNCTION::default();
                bf.BlendOp = AC_SRC_OVER as u8;
                bf.SourceConstantAlpha = 255;
                bf.AlphaFormat = AC_SRC_ALPHA as u8;
                let draw_x = px as i32 - (BROOM_W / 2); 
                let draw_y = py as i32 - (BROOM_H as f32 * 0.65) as i32; 
                GdiAlphaBlend(mem_dc, draw_x, draw_y, BROOM_W, BROOM_H, broom_dc, 0, 0, BROOM_W, BROOM_H, bf);
                SelectObject(broom_dc, old_hbm_broom);
                DeleteDC(broom_dc);
                DeleteObject(hbm);
            }
        }

        let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY).ok();
        
        SelectObject(mem_dc, old_bitmap);
        DeleteObject(mem_bitmap);
        DeleteDC(mem_dc);
        EndPaint(hwnd, &mut ps);
    }
}
