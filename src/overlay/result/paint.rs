use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;
use std::mem::size_of;
use crate::overlay::broom_assets::{render_procedural_broom, BroomRenderParams, BROOM_W, BROOM_H};
use super::state::{WINDOW_STATES, AnimationMode};

// OPTIMIZATION: GDI object wrapper for automatic cleanup (RAII pattern)
struct GdiObj(HGDIOBJ);
impl GdiObj {
    fn from_hpen(pen: HPEN) -> Self {
        GdiObj(HGDIOBJ(pen.0))
    }
    fn from_hbrush(brush: HBRUSH) -> Self {
        GdiObj(HGDIOBJ(brush.0))
    }
}
impl Drop for GdiObj {
    fn drop(&mut self) {
        if self.0.0 != 0 {
            unsafe { let _ = DeleteObject(self.0); }
        }
    }
}

// Helper: Efficiently measure text height
unsafe fn measure_text_height(hdc: windows::Win32::Graphics::Gdi::CreatedHDC, text: &mut [u16], font_size: i32, width: i32) -> i32 {
    let hfont = CreateFontW(font_size, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
    let old_font = SelectObject(hdc, hfont);
    let mut calc_rect = RECT { left: 0, top: 0, right: width, bottom: 0 };
    DrawTextW(hdc, text, &mut calc_rect, DT_CALCRECT | DT_WORDBREAK);
    SelectObject(hdc, old_font);
    DeleteObject(hfont);
    calc_rect.bottom
}

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

        // DIB Section for direct pixel access
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let mem_bitmap = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0).unwrap();
        let mem_dc = CreateCompatibleDC(hdc);
        let old_bitmap = SelectObject(mem_dc, mem_bitmap);

        // --- STEP 1: SNAPSHOT STATE ---
        let (
            bg_color_u32, is_hovered, on_copy_btn, copy_success, broom_data, particles, 
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
                    state.bg_color, state.is_hovered, state.on_copy_btn, state.copy_success, broom_info, particles_vec,
                    state.content_bitmap, state.cached_font_size, state.font_cache_dirty
                )
            } else {
                (0x00222222, false, false, false, None, Vec::new(), HBITMAP(0), 72, true)
            }
        };

        // --- STEP 1.5: DRAW BACKGROUND GRADIENT ---
        if !p_bits.is_null() {
            let raw_pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);
            
            let top_r = (bg_color_u32 >> 16) & 0xFF;
            let top_g = (bg_color_u32 >> 8) & 0xFF;
            let top_b = bg_color_u32 & 0xFF;

            let bot_r = (top_r as f32 * 0.6) as u32;
            let bot_g = (top_g as f32 * 0.6) as u32;
            let bot_b = (top_b as f32 * 0.6) as u32;

            for y in 0..height {
                let t = y as f32 / height as f32;
                let r = (top_r as f32 * (1.0 - t) + bot_r as f32 * t) as u32;
                let g = (top_g as f32 * (1.0 - t) + bot_g as f32 * t) as u32;
                let b = (top_b as f32 * (1.0 - t) + bot_b as f32 * t) as u32;
                let col = (255 << 24) | (r << 16) | (g << 8) | b;
                
                let start = (y * width) as usize;
                let end = start + width as usize;
                raw_pixels[start..end].fill(col);
            }
        }

        // --- STEP 2: SMART FONT UPDATE ---
        if cache_dirty || cached_bm.0 == 0 {
            if cached_bm.0 != 0 { DeleteObject(cached_bm); }

            cached_bm = CreateCompatibleBitmap(hdc, width, height);
            let cache_dc = CreateCompatibleDC(hdc);
            let old_cache_bm = SelectObject(cache_dc, cached_bm);

            let dark_brush = CreateSolidBrush(COLORREF(bg_color_u32));
            let fill_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(cache_dc, &fill_rect, dark_brush);
            DeleteObject(dark_brush);

            SetBkMode(cache_dc, TRANSPARENT);
            SetTextColor(cache_dc, COLORREF(0x00FFFFFF));

            let text_len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; text_len as usize];
            GetWindowTextW(hwnd, &mut buf);

            let h_padding = 12;
            let available_w = (width - (h_padding * 2)).max(1);
            let v_safety_margin = 4; 
            let available_h = (height - v_safety_margin).max(1);

            let mut low = 8;
            let max_possible = available_h.min(100); 
            let mut high = max_possible;
            let mut best_fit = 8;

            if high < low {
                best_fit = 8;
            } else {
                while low <= high {
                    let mid = (low + high) / 2;
                    let h = measure_text_height(cache_dc, &mut buf, mid, available_w);
                    if h <= available_h {
                        best_fit = mid;
                        low = mid + 1; 
                    } else {
                        high = mid - 1; 
                    }
                }
            }
            font_size = best_fit;

            let hfont = CreateFontW(font_size, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            let old_font = SelectObject(cache_dc, hfont);

            let mut measure_rect = RECT { left: 0, top: 0, right: available_w, bottom: 0 };
            DrawTextW(cache_dc, &mut buf, &mut measure_rect, DT_CALCRECT | DT_WORDBREAK);
            let text_h = measure_rect.bottom;
            
            let offset_y = ((height - text_h) / 2).max(0);
            let mut draw_rect = RECT { 
                left: h_padding, 
                top: offset_y, 
                right: width - h_padding, 
                bottom: height
            };

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

        // --- STEP 4: DYNAMIC PARTICLES ---
        if !particles.is_empty() && !p_bits.is_null() {
            let raw_pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);

            for (d_x, d_y, life, size, col) in particles {
                if life <= 0.0 { continue; }
                let radius = (size * life); 
                if radius < 0.5 { continue; }

                let p_r = ((col >> 16) & 0xFF) as f32;
                let p_g = ((col >> 8) & 0xFF) as f32;
                let p_b = (col & 0xFF) as f32;
                let p_max_alpha = 255.0 * life;

                let min_x = (d_x - radius - 1.0).floor() as i32;
                let max_x = (d_x + radius + 1.0).ceil() as i32;
                let min_y = (d_y - radius - 1.0).floor() as i32;
                let max_y = (d_y + radius + 1.0).ceil() as i32;

                let start_x = min_x.max(0);
                let end_x = max_x.min(width - 1);
                let start_y = min_y.max(0);
                let end_y = max_y.min(height - 1);

                for y in start_y..=end_y {
                    for x in start_x..=end_x {
                        let dx = x as f32 - d_x;
                        let dy = y as f32 - d_y;
                        let dist = (dx*dx + dy*dy).sqrt();
                        let aa_edge = (radius + 0.5 - dist).clamp(0.0, 1.0);

                        if aa_edge > 0.0 {
                            let idx = (y * width + x) as usize;
                            let bg_px = raw_pixels[idx];

                            let bg_b = (bg_px & 0xFF) as f32;
                            let bg_g = ((bg_px >> 8) & 0xFF) as f32;
                            let bg_r = ((bg_px >> 16) & 0xFF) as f32;

                            let final_alpha_norm = (p_max_alpha * aa_edge) / 255.0;
                            let inv_alpha = 1.0 - final_alpha_norm;

                            let out_r = (p_r * final_alpha_norm + bg_r * inv_alpha) as u32;
                            let out_g = (p_g * final_alpha_norm + bg_g * inv_alpha) as u32;
                            let out_b = (p_b * final_alpha_norm + bg_b * inv_alpha) as u32;

                            raw_pixels[idx] = (255 << 24) | (out_r << 16) | (out_g << 8) | out_b;
                        }
                    }
                }
            }
        }

        // --- STEP 5: DYNAMIC BROOM ---
        let broom_bitmap_data = if let Some((bx, by, params)) = broom_data {
            let pixels = render_procedural_broom(params);
            let hbm = create_bitmap_from_pixels(&pixels, BROOM_W, BROOM_H);
            Some((bx, by, hbm))
        } else { None };

        // --- STEP 6: HIGH-VISIBILITY ROUNDED BUTTON ---
        if is_hovered && !p_bits.is_null() {
            let raw_pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);
            
            let btn_size = 28; 
            let margin = 12;
            
            // === ADAPTIVE VERTICAL CENTER ===
            let threshold_h = btn_size + (margin * 2);
            let cy = if height < threshold_h {
                (height as f32) / 2.0
            } else {
                (height - margin - btn_size / 2) as f32
            };
            
            let cx = (width - margin - btn_size / 2) as f32;
            let radius = 13.0;

            // Button color logic
            let (tr, tg, tb) = if copy_success {
                (30.0, 180.0, 30.0) // Success Green
            } else if on_copy_btn {
                (128.0, 128.0, 128.0) // Hover Bright
            } else {
                (80.0, 80.0, 80.0)    // Standard Visible Grey
            };

            let b_start_x = (cx - radius - 4.0) as i32;
            let b_end_x = (cx + radius + 4.0) as i32;
            let b_start_y = (cy - radius - 4.0) as i32;
            let b_end_y = (cy + radius + 4.0) as i32;

            for y in b_start_y.max(0)..b_end_y.min(height) {
                for x in b_start_x.max(0)..b_end_x.min(width) {
                    let dx = (x as f32 - cx).abs();
                    let dy = (y as f32 - cy).abs();
                    let dist = (dx*dx + dy*dy).sqrt();
                    
                    // 1. AA for Button Body
                    let aa_body = (radius + 0.5 - dist).clamp(0.0, 1.0);

                    // 2. Smooth Ring Border (Thickness 1.5px)
                    let border_inner_radius = radius - 1.5;
                    let border_outer = (radius + 0.5 - dist).clamp(0.0, 1.0);
                    let border_inner = (dist - (border_inner_radius - 0.5)).clamp(0.0, 1.0);
                    let border_alpha = border_outer * border_inner * 0.6; // 60% opacity white border

                    if aa_body > 0.0 || border_alpha > 0.0 {
                        let idx = (y * width + x) as usize;
                        let bg = raw_pixels[idx];
                        let bg_b = (bg & 0xFF) as f32;
                        let bg_g = ((bg >> 8) & 0xFF) as f32;
                        let bg_r = ((bg >> 16) & 0xFF) as f32;
                        
                        let mut final_r = bg_r;
                        let mut final_g = bg_g;
                        let mut final_b = bg_b;

                        // Apply Body Color
                        if aa_body > 0.0 {
                            let alpha = 0.9 * aa_body;
                            let inv = 1.0 - alpha;
                            final_r = tr * alpha + final_r * inv;
                            final_g = tg * alpha + final_g * inv;
                            final_b = tb * alpha + final_b * inv;
                        }

                        // Apply Additive White Border
                        if border_alpha > 0.0 {
                            final_r += 255.0 * border_alpha;
                            final_g += 255.0 * border_alpha;
                            final_b += 255.0 * border_alpha;
                        }

                        final_r = final_r.min(255.0);
                        final_g = final_g.min(255.0);
                        final_b = final_b.min(255.0);
                        
                        raw_pixels[idx] = (255 << 24) | ((final_r as u32) << 16) | ((final_g as u32) << 8) | (final_b as u32);
                    }
                }
            }

            // Draw Icon (GDI)
            // Use accurate rounding for GDI coordinates to match button center (cx, cy)
            let icx = cx.round() as i32;
            let icy = cy.round() as i32;

            // OPTIMIZATION: Use GDI wrapper for automatic cleanup
            let icon_pen = GdiObj::from_hpen(CreatePen(PS_SOLID, 2, COLORREF(0x00FFFFFF)));
            let old_pen = SelectObject(mem_dc, icon_pen.0);
            
            if copy_success {
                // Checkmark
                MoveToEx(mem_dc, icx - 4, icy, None);
                LineTo(mem_dc, icx - 1, icy + 4);
                LineTo(mem_dc, icx + 5, icy - 4);
            } else {
                // Copy Icon (Two rects) with Occlusion
                let r2_l = icx - 5; let r2_t = icy - 4; let r2_r = icx + 2; let r2_b = icy + 1;
                let r1_l = icx - 3; let r1_t = icy - 2; let r1_r = icx + 5; let r1_b = icy + 4;
                
                // Draw Back Rect (Outline)
                let null_brush = GetStockObject(NULL_BRUSH);
                let old_brush = SelectObject(mem_dc, null_brush);
                Rectangle(mem_dc, r2_l, r2_t, r2_r, r2_b);
                
                // Fill Front Rect (Masking)
                let brush_col = (tb as u32) << 16 | (tg as u32) << 8 | (tr as u32);
                let solid_brush = GdiObj::from_hbrush(CreateSolidBrush(COLORREF(brush_col)));
                SelectObject(mem_dc, solid_brush.0);
                Rectangle(mem_dc, r1_l, r1_t, r1_r, r1_b);
                
                SelectObject(mem_dc, old_brush);
            }
            SelectObject(mem_dc, old_pen);
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
