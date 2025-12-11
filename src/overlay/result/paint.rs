use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;
use std::mem::size_of;
use crate::overlay::broom_assets::{render_procedural_broom, BroomRenderParams, BROOM_W, BROOM_H};
use crate::overlay::paint_utils::{sd_rounded_box, hsv_to_rgb};
use super::state::{WINDOW_STATES, AnimationMode, ResizeEdge};

// Helper: Measure text dimensions (Height AND Width)
unsafe fn measure_text_bounds(hdc: windows::Win32::Graphics::Gdi::CreatedHDC, text: &mut [u16], font_size: i32, max_width: i32) -> (i32, i32) {
    let hfont = CreateFontW(font_size, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
    let old_font = SelectObject(hdc, hfont);
    
    // We start with the max width constraint.
    // DT_CALCRECT will expand the 'right' value if a single word is wider than max_width (unless we handle it),
    // or wrap lines which increases 'bottom'.
    let mut calc_rect = RECT { left: 0, top: 0, right: max_width, bottom: 0 };
    
    // DT_EDITCONTROL helps simulate multiline text box behavior
    DrawTextW(hdc, text, &mut calc_rect, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL);
    
    SelectObject(hdc, old_font);
    DeleteObject(hfont);
    
    // Return (Height, Width)
    (calc_rect.bottom, calc_rect.right)
}

pub fn create_bitmap_from_pixels(pixels: &[u32], w: i32, h: i32) -> HBITMAP {
    unsafe {
        let hdc = GetDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default()
            }, ..Default::default()
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

// --- MATH HELPERS FOR SDF ICONS ---
fn dist_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let h = (pax * bax + pay * bay) / (bax * bax + bay * bay).max(0.001);
    let h = h.clamp(0.0, 1.0);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx*dx + dy*dy).sqrt()
}

fn sd_box(px: f32, py: f32, cx: f32, cy: f32, w: f32, h: f32) -> f32 {
    let dx = (px - cx).abs() - w;
    let dy = (py - cy).abs() - h;
    (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0)
}

pub fn paint_window(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        // --- PHASE 1: STATE SNAPSHOT & CACHE MANAGEMENT ---
        let (
             bg_color_u32, is_hovered, on_copy_btn, copy_success, on_edit_btn, on_undo_btn, broom_data, particles,
             mut cached_text_bm, _cached_font_size, cache_dirty,
             cached_bg_bm,
             is_refining,
             anim_offset,
             history_count
         ) = {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                
                // 1.1 Update Background Cache if needed
                if state.bg_bitmap.0 == 0 || state.bg_w != width || state.bg_h != height {
                    if state.bg_bitmap.0 != 0 { DeleteObject(state.bg_bitmap); }

                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default()
                        }, ..Default::default()
                    };
                    
                    let mut p_bg_bits: *mut core::ffi::c_void = std::ptr::null_mut();
                    let hbm_bg = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut p_bg_bits, None, 0).unwrap();
                    
                    if !p_bg_bits.is_null() {
                        let pixels = std::slice::from_raw_parts_mut(p_bg_bits as *mut u32, (width * height) as usize);
                        let top_r = (state.bg_color >> 16) & 0xFF;
                        let top_g = (state.bg_color >> 8) & 0xFF;
                        let top_b = state.bg_color & 0xFF;
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
                            pixels[start..end].fill(col);
                        }
                    }
                    state.bg_bitmap = hbm_bg;
                    state.bg_w = width;
                    state.bg_h = height;
                }

                if state.last_w != width || state.last_h != height {
                    state.font_cache_dirty = true;
                    state.last_w = width;
                    state.last_h = height;
                }

                let particles_vec: Vec<(f32, f32, f32, f32, u32)> = state.physics.particles.iter()
                    .map(|p| (p.x, p.y, p.life, p.size, p.color)).collect();

                // AGGRESSIVE FIX: Don't render broom during ANY closing animation (Smashing OR DragOut)
                // This completely eliminates the "frozen frame" issue during fade
                // The broom is only shown on hover, not during click-to-close
                let is_closing = matches!(state.physics.mode, AnimationMode::Smashing | AnimationMode::DragOut);
                
                let show_broom = !is_closing && (
                    state.is_hovered 
                        && !state.on_copy_btn 
                        && !state.on_edit_btn
                        && !state.on_undo_btn
                        && state.current_resize_edge == ResizeEdge::None
                );
                
                let broom_info = if show_broom {
                     Some((state.physics.x, state.physics.y, BroomRenderParams {
                            tilt_angle: state.physics.current_tilt,
                            squish: state.physics.squish_factor,
                            bend: state.physics.bristle_bend,
                            opacity: 1.0,
                        }))
                } else { None };

                (
                    state.bg_color, state.is_hovered, state.on_copy_btn, state.copy_success, state.on_edit_btn, state.on_undo_btn, broom_info, particles_vec,
                    state.content_bitmap, state.cached_font_size as i32, state.font_cache_dirty,
                    state.bg_bitmap,
                    state.is_refining,
                    state.animation_offset,
                    state.text_history.len()
                )
            } else {
                (0, false, false, false, false, false, None, Vec::new(), HBITMAP(0), 72, true, HBITMAP(0), false, 0.0, 0)
            }
        };

        // --- PHASE 2: COMPOSITOR SETUP ---
        let mem_dc = CreateCompatibleDC(hdc);
        
        let bmi_scratch = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0 as u32, ..Default::default()
            }, ..Default::default()
        };
        let mut scratch_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let scratch_bitmap = CreateDIBSection(hdc, &bmi_scratch, DIB_RGB_COLORS, &mut scratch_bits, None, 0).unwrap();
        let old_scratch = SelectObject(mem_dc, scratch_bitmap);

        // 2.1 Copy Background
        if cached_bg_bm.0 != 0 {
            let cache_dc = CreateCompatibleDC(hdc);
            let old_cbm = SelectObject(cache_dc, cached_bg_bm);
            let _ = BitBlt(mem_dc, 0, 0, width, height, cache_dc, 0, 0, SRCCOPY).ok();
            SelectObject(cache_dc, old_cbm);
            DeleteDC(cache_dc);
        }

        // --- PHASE 3: TEXT CACHE UPDATE ---
        if !is_refining {
            if cache_dirty || cached_text_bm.0 == 0 {
                if cached_text_bm.0 != 0 { DeleteObject(cached_text_bm); }

                cached_text_bm = CreateCompatibleBitmap(hdc, width, height);
                let cache_dc = CreateCompatibleDC(hdc);
                let old_cache_bm = SelectObject(cache_dc, cached_text_bm);

                let dark_brush = CreateSolidBrush(COLORREF(bg_color_u32));
                let fill_rect = RECT { left: 0, top: 0, right: width, bottom: height };
                FillRect(cache_dc, &fill_rect, dark_brush);
                DeleteObject(dark_brush);

                SetBkMode(cache_dc, TRANSPARENT);
                SetTextColor(cache_dc, COLORREF(0x00FFFFFF));

                let text_len = GetWindowTextLengthW(hwnd) + 1;
                let mut buf = vec![0u16; text_len as usize];
                GetWindowTextW(hwnd, &mut buf);

                let h_padding = 6; 
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
                        let (h, w) = measure_text_bounds(cache_dc, &mut buf, mid, available_w);
                        if h <= available_h && w <= available_w {
                            best_fit = mid;
                            low = mid + 1;
                        } else {
                            high = mid - 1;
                        }
                    }
                }
                let font_size_val = best_fit;

                let hfont = CreateFontW(font_size_val, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
                let old_font = SelectObject(cache_dc, hfont);

                let mut measure_rect = RECT { left: 0, top: 0, right: available_w, bottom: 0 };
                DrawTextW(cache_dc, &mut buf, &mut measure_rect, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL);
                let text_h = measure_rect.bottom;
                
                let offset_y = ((height - text_h) / 2).max(0);
                let mut draw_rect = RECT {
                    left: h_padding,
                    top: offset_y,
                    right: width - h_padding,
                    bottom: height
                };
                
                DrawTextW(cache_dc, &mut buf, &mut draw_rect as *mut _, DT_LEFT | DT_WORDBREAK | DT_EDITCONTROL);

                SelectObject(cache_dc, old_font);
                DeleteObject(hfont);
                SelectObject(cache_dc, old_cache_bm);
                DeleteDC(cache_dc);

                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.content_bitmap = cached_text_bm;
                    state.cached_font_size = font_size_val;
                    state.font_cache_dirty = false;
                }
            }

            if cached_text_bm.0 != 0 {
                let cache_dc = CreateCompatibleDC(hdc);
                let old_cbm = SelectObject(cache_dc, cached_text_bm);
                let _ = BitBlt(mem_dc, 0, 0, width, height, cache_dc, 0, 0, SRCCOPY).ok();
                SelectObject(cache_dc, old_cbm);
                DeleteDC(cache_dc);
            }
        }

        // --- PHASE 4: PIXEL MANIPULATION ---
        if !scratch_bits.is_null() {
            let raw_pixels = std::slice::from_raw_parts_mut(scratch_bits as *mut u32, (width * height) as usize);

            // 4.0 REFINEMENT GLOW
            if is_refining {
                let bx = width as f32 / 2.0;
                let by = height as f32 / 2.0;
                let center_x = bx;
                let center_y = by;
                let time_rad = anim_offset.to_radians();

                for y in 0..height {
                    for x in 0..width {
                        let idx = (y * width + x) as usize;
                        let px = x as f32 - center_x;
                        let py = y as f32 - center_y;
                        let d = sd_rounded_box(px, py, bx, by, 12.0);

                        if d <= 0.0 {
                             let dist = d.abs();
                             if dist < 20.0 {
                                 let angle = py.atan2(px);
                                 let noise = (angle * 12.0 - time_rad * 2.0).sin() * 0.5;
                                 let glow_width = 14.0;
                                 let t = (dist / glow_width).clamp(0.0, 1.0);
                                 let base_intensity = (1.0 - t).powi(3);

                                 if base_intensity > 0.01 {
                                     let noise_mod = (1.0 + noise * 0.3).clamp(0.0, 2.0);
                                     let final_intensity = (base_intensity * noise_mod).clamp(0.0, 1.0);
                                     if final_intensity > 0.01 {
                                         let deg = angle.to_degrees() + (anim_offset * 2.0);
                                         let hue = (deg % 360.0 + 360.0) % 360.0;
                                         let rgb = hsv_to_rgb(hue, 0.85, 1.0); 
                                         let bg_px = raw_pixels[idx];
                                         let bg_b = (bg_px & 0xFF) as f32;
                                         let bg_g = ((bg_px >> 8) & 0xFF) as f32;
                                         let bg_r = ((bg_px >> 16) & 0xFF) as f32;
                                         let fg_r = ((rgb >> 16) & 0xFF) as f32;
                                         let fg_g = ((rgb >> 8) & 0xFF) as f32;
                                         let fg_b = (rgb & 0xFF) as f32;
                                         
                                         let out_r = (fg_r * final_intensity + bg_r * (1.0 - final_intensity)) as u32;
                                         let out_g = (fg_g * final_intensity + bg_g * (1.0 - final_intensity)) as u32;
                                         let out_b = (fg_b * final_intensity + bg_b * (1.0 - final_intensity)) as u32;
                                         raw_pixels[idx] = (255 << 24) | (out_r << 16) | (out_g << 8) | out_b;
                                     }
                                 }
                             }
                        }
                    }
                }
            }

            // 4.1 Particles
            for (d_x, d_y, life, size, col) in particles {
                if life <= 0.0 { continue; }
                let radius = size * life;
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

            // 4.2 Buttons
            if is_hovered && !is_refining {
                let btn_size = 28;
                let margin = 12;
                let threshold_h = btn_size + (margin * 2);
                let cy = if height < threshold_h {
                    (height as f32) / 2.0
                } else {
                    (height - margin - btn_size / 2) as f32
                };
                let cx_copy = (width - margin - btn_size / 2) as f32;
                let cx_edit = cx_copy - (btn_size as f32) - 8.0;
                let cx_undo = cx_edit - (btn_size as f32) - 8.0;
                
                let radius = 13.0;

                let (tr_c, tg_c, tb_c) = if copy_success {
                    (30.0, 180.0, 30.0) 
                } else if on_copy_btn {
                    (128.0, 128.0, 128.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                
                let (tr_e, tg_e, tb_e) = if on_edit_btn {
                    (128.0, 128.0, 128.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                let (tr_u, tg_u, tb_u) = if on_undo_btn {
                    (128.0, 128.0, 128.0)
                } else {
                    (80.0, 80.0, 80.0)
                };

                let b_start_x = (cx_undo - radius - 4.0) as i32;
                let b_end_x = (cx_copy + radius + 4.0) as i32;
                let b_start_y = (cy - radius - 4.0) as i32;
                let b_end_y = (cy + radius + 4.0) as i32;

                let show_undo = history_count > 0;

                for y in b_start_y.max(0)..b_end_y.min(height) {
                    for x in b_start_x.max(0)..b_end_x.min(width) {
                        let fx = x as f32;
                        let fy = y as f32;
                        let dy = (fy - cy).abs();
                        
                        let dx_c = (fx - cx_copy).abs();
                        let dist_c = (dx_c*dx_c + dy*dy).sqrt();
                        
                        let dx_e = (fx - cx_edit).abs();
                        let dist_e = (dx_e*dx_e + dy*dy).sqrt();
                        
                        let dx_u = (fx - cx_undo).abs();
                        let dist_u = (dx_u*dx_u + dy*dy).sqrt();

                        // --- COPY BUTTON ---
                        let aa_body_c = (radius + 0.5 - dist_c).clamp(0.0, 1.0);
                        let border_inner_radius = radius - 1.5;
                        let border_alpha_c = ((radius + 0.5 - dist_c).clamp(0.0, 1.0) * ((dist_c - (border_inner_radius - 0.5)).clamp(0.0, 1.0))) * 0.6;

                        let icon_alpha_c = if copy_success {
                            let d1 = dist_segment(fx, fy, cx_copy - 4.0, cy, cx_copy - 1.0, cy + 3.0);
                            let d2 = dist_segment(fx, fy, cx_copy - 1.0, cy + 3.0, cx_copy + 4.0, cy - 4.0);
                            (1.8 - d1.min(d2)).clamp(0.0, 1.0)
                        } else {
                            let back_d = sd_box(fx, fy, cx_copy - 2.0, cy - 2.0, 3.0, 4.0);
                            let back_outline = (1.25 - back_d.abs()).clamp(0.0, 1.0);
                            let front_d = sd_box(fx, fy, cx_copy + 2.0, cy + 2.0, 3.0, 4.0);
                            let front_fill = (0.8 - front_d).clamp(0.0, 1.0);
                            let mask_d = sd_box(fx, fy, cx_copy + 2.0, cy + 2.0, 4.5, 5.5);
                            (front_fill + back_outline * mask_d.clamp(0.0, 1.0)).clamp(0.0, 1.0)
                        };

                        // --- EDIT BUTTON (AI SPARKLE) ---
                        let aa_body_e = (radius + 0.5 - dist_e).clamp(0.0, 1.0);
                        let border_alpha_e = ((radius + 0.5 - dist_e).clamp(0.0, 1.0) * ((dist_e - (border_inner_radius - 0.5)).clamp(0.0, 1.0))) * 0.6;
                        
                        // Main Star
                        let sx = (fx - cx_edit).abs();
                        let sy = (fy - cy).abs();
                        // Concave Star Shape: (x^0.5 + y^0.5)^2 approximation
                        let star_dist = (sx.powf(0.6) + sy.powf(0.6)).powf(1.0/0.6) - 4.5;
                        let mut icon_alpha_e = (1.2 - star_dist).clamp(0.0, 1.0);
                        
                        // Small Star (Offset)
                        let sx2 = (fx - (cx_edit + 4.5)).abs();
                        let sy2 = (fy - (cy - 3.5)).abs();
                        let star2_dist = (sx2.powf(0.6) + sy2.powf(0.6)).powf(1.0/0.6) - 2.2;
                        icon_alpha_e = icon_alpha_e.max((1.2 - star2_dist).clamp(0.0, 1.0));


                        // --- UNDO BUTTON (Simple Back Arrow) ---
                        let mut aa_body_u = 0.0;
                        let mut border_alpha_u = 0.0;
                        let mut icon_alpha_u = 0.0;
                        
                        if show_undo {
                            aa_body_u = (radius + 0.5 - dist_u).clamp(0.0, 1.0);
                            border_alpha_u = ((radius + 0.5 - dist_u).clamp(0.0, 1.0) * ((dist_u - (border_inner_radius - 0.5)).clamp(0.0, 1.0))) * 0.6;
                            
                            // Simple Straight Arrow <-
                            // Shaft: Right to Left
                            let tip_x = cx_undo - 3.5;
                            let tail_x = cx_undo + 3.5;
                            let d_shaft = dist_segment(fx, fy, tip_x, cy, tail_x, cy);
                            
                            // Wings: Tip to Right-Up/Right-Down
                            // Wing length approx 3.5, angle 45 deg
                            let wing_dx = 3.0; 
                            let wing_dy = 3.0;
                            let d_wing1 = dist_segment(fx, fy, tip_x, cy, tip_x + wing_dx, cy - wing_dy);
                            let d_wing2 = dist_segment(fx, fy, tip_x, cy, tip_x + wing_dx, cy + wing_dy);
                            
                            let d_arrow = d_shaft.min(d_wing1).min(d_wing2);
                            icon_alpha_u = (1.3 - d_arrow).clamp(0.0, 1.0);
                        }

                        if aa_body_c > 0.0 || border_alpha_c > 0.0 || icon_alpha_c > 0.0 ||
                           aa_body_e > 0.0 || border_alpha_e > 0.0 || icon_alpha_e > 0.0 ||
                           aa_body_u > 0.0 || border_alpha_u > 0.0 || icon_alpha_u > 0.0 {
                            let idx = (y * width + x) as usize;
                            let bg = raw_pixels[idx];
                            let bg_b = (bg & 0xFF) as f32;
                            let bg_g = ((bg >> 8) & 0xFF) as f32;
                            let bg_r = ((bg >> 16) & 0xFF) as f32;
                            
                            let mut final_r = bg_r;
                            let mut final_g = bg_g;
                            let mut final_b = bg_b;

                            // BLEND COPY
                            if aa_body_c > 0.0 {
                                let alpha = 0.9 * aa_body_c;
                                final_r = tr_c * alpha + final_r * (1.0 - alpha);
                                final_g = tg_c * alpha + final_g * (1.0 - alpha);
                                final_b = tb_c * alpha + final_b * (1.0 - alpha);
                            }
                            if border_alpha_c > 0.0 {
                                final_r += 255.0 * border_alpha_c;
                                final_g += 255.0 * border_alpha_c;
                                final_b += 255.0 * border_alpha_c;
                            }
                            if icon_alpha_c > 0.0 {
                                final_r = 255.0 * icon_alpha_c + final_r * (1.0 - icon_alpha_c);
                                final_g = 255.0 * icon_alpha_c + final_g * (1.0 - icon_alpha_c);
                                final_b = 255.0 * icon_alpha_c + final_b * (1.0 - icon_alpha_c);
                            }

                            // BLEND EDIT (Sparkle)
                            if aa_body_e > 0.0 {
                                let alpha = 0.9 * aa_body_e;
                                final_r = tr_e * alpha + final_r * (1.0 - alpha);
                                final_g = tg_e * alpha + final_g * (1.0 - alpha);
                                final_b = tb_e * alpha + final_b * (1.0 - alpha);
                            }
                            if border_alpha_e > 0.0 {
                                final_r += 255.0 * border_alpha_e;
                                final_g += 255.0 * border_alpha_e;
                                final_b += 255.0 * border_alpha_e;
                            }
                            if icon_alpha_e > 0.0 {
                                final_r = 255.0 * icon_alpha_e + final_r * (1.0 - icon_alpha_e);
                                final_g = 255.0 * icon_alpha_e + final_g * (1.0 - icon_alpha_e);
                                final_b = 255.0 * icon_alpha_e + final_b * (1.0 - icon_alpha_e);
                            }

                            // BLEND UNDO
                            if show_undo {
                                if aa_body_u > 0.0 {
                                    let alpha = 0.9 * aa_body_u;
                                    final_r = tr_u * alpha + final_r * (1.0 - alpha);
                                    final_g = tg_u * alpha + final_g * (1.0 - alpha);
                                    final_b = tb_u * alpha + final_b * (1.0 - alpha);
                                }
                                if border_alpha_u > 0.0 {
                                    final_r += 255.0 * border_alpha_u;
                                    final_g += 255.0 * border_alpha_u;
                                    final_b += 255.0 * border_alpha_u;
                                }
                                if icon_alpha_u > 0.0 {
                                    final_r = 255.0 * icon_alpha_u + final_r * (1.0 - icon_alpha_u);
                                    final_g = 255.0 * icon_alpha_u + final_g * (1.0 - icon_alpha_u);
                                    final_b = 255.0 * icon_alpha_u + final_b * (1.0 - icon_alpha_u);
                                }
                            }

                            final_r = final_r.min(255.0);
                            final_g = final_g.min(255.0);
                            final_b = final_b.min(255.0);
                            
                            raw_pixels[idx] = (255 << 24) | ((final_r as u32) << 16) | ((final_g as u32) << 8) | (final_b as u32);
                        }
                    }
                }
            }
        }

        // --- PHASE 5: DYNAMIC BROOM ---
        let broom_bitmap_data = if let Some((bx, by, params)) = broom_data {
            let pixels = render_procedural_broom(params);
            let hbm = create_bitmap_from_pixels(&pixels, BROOM_W, BROOM_H);
            Some((bx, by, hbm))
        } else { None };

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

        // --- PHASE 6: FINAL BLIT ---
        let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY).ok();

        SelectObject(mem_dc, old_scratch);
        DeleteObject(scratch_bitmap);
        DeleteDC(mem_dc);
        
        EndPaint(hwnd, &mut ps);
    }
}
