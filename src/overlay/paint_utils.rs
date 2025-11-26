use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use std::mem::size_of;

const CORNER_RADIUS: f32 = 12.0;

#[inline(always)]
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
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

#[inline(always)]
pub fn sd_rounded_box(px: f32, py: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = px.abs() - bx + r;
    let qy = py.abs() - by + r;
    let len_max_q = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let min_max_q = qx.max(qy).min(0.0);
    len_max_q + min_max_q - r
}

pub unsafe fn render_box_sdf(hdc_dest: HDC, bounds: RECT, w: i32, h: i32, is_glowing: bool, time_offset: f32) {
    let pad = 60; 
    let buf_w = w + (pad * 2);
    let buf_h = h + (pad * 2);
    
    // Threshold: ~670x670. Large areas use Fast Loop.
    let is_large_area = (w as i64 * h as i64) > 450_000; 

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
        let eff_radius = CORNER_RADIUS.min(bx).min(by);

        if is_large_area && is_glowing {
            // === FAST LOOP (Outer Glow for Performance) ===
            // Simplified math for massive regions to prevent lag.
            // OPTIMIZATION: Pre-calculate trig values outside the loop
            let time_rad = time_offset.to_radians();
            let time_sin = (time_rad * 4.0).sin();
            let glow_width = 12.0 + time_sin * 4.0;
            let hue = (time_offset * 3.0) % 360.0;
            let global_color = hsv_to_rgb(hue, 0.75, 1.0);
            
            let gc_r = ((global_color >> 16) & 0xFF) as u32;
            let gc_g = ((global_color >> 8) & 0xFF) as u32;
            let gc_b = (global_color & 0xFF) as u32;

            for y in 0..buf_h {
                let py = (y as f32) - center_y;
                for x in 0..buf_w {
                    let px = (x as f32) - center_x;
                    
                    let qx = px.abs() - bx + eff_radius;
                    let qy = py.abs() - by + eff_radius;
                    // OPTIMIZATION: Inline sd_rounded_box logic and avoid function call
                    let d = if qx > 0.0 && qy > 0.0 { ((qx * qx + qy * qy).sqrt()) - eff_radius } else { qx.max(qy) - eff_radius };
                    
                    if d <= 0.0 {
                        pixels[(y * buf_w + x) as usize] = 0xD80E0E0E;
                    } else if d < glow_width {
                        let t = d / glow_width;
                        let alpha_f = (1.0 - t).powi(2);
                        let a = (alpha_f * 255.0) as u32;
                        if a > 0 {
                            let r = (gc_r * a) >> 8;
                            let g = (gc_g * a) >> 8;
                            let b = (gc_b * a) >> 8;
                            pixels[(y * buf_w + x) as usize] = (a << 24) | (r << 16) | (g << 8) | b;
                        }
                    }
                }
            }
        } else {
            // === FANCY LOOP (Small/Medium or Dragging) ===
            // OPTIMIZATION: Pre-calculate trig values outside the loop
            let time_rad = time_offset.to_radians();
            let time_sin_2 = (time_rad * 2.0).sin();
            let time_sin_neg_3 = (-time_rad * 3.0).sin();
            let min_dim = (w.min(h) as f32).max(1.0);
            let perimeter = 2.0 * (w + h) as f32;
            
            // Restored noise scaling for Inner Glow
            let dynamic_base_scale = (min_dim * 0.2).clamp(30.0, 180.0);
            let complexity_scale = 1.0 + (perimeter / 1800.0);
            let freq1 = (2.0 * complexity_scale).round();
            let freq2 = (5.0 * complexity_scale).round();
            
            for y in 0..buf_h {
                for x in 0..buf_w {
                    let idx = (y * buf_w + x) as usize;
                    let px = (x as f32) - center_x;
                    let py = (y as f32) - center_y;
                    
                    // OPTIMIZATION: Inline sd_rounded_box to avoid function call overhead in tight loop
                    let qx = px.abs() - bx + eff_radius;
                    let qy = py.abs() - by + eff_radius;
                    let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + (qx.max(qy).min(0.0)) - eff_radius;
                    
                    let mut final_col = 0u32;
                    let mut final_alpha = 0.0f32;

                    // --- LOGIC SPLIT ---
                    if d > 0.0 {
                        // == OUTSIDE THE BOX ==
                        if is_glowing {
                            // PROCESSING: White Border
                            let aa = (1.5 - d).clamp(0.0, 1.0);
                            if aa > 0.0 {
                                final_alpha = aa;
                                final_col = 0x00FFFFFF;
                            }
                        } else {
                            // DRAGGING: Grey Border (Anti-Aliased)
                            // Soft fade out from d=1.5 to d=2.5
                            let t_out = (d - 1.5).max(0.0);
                            let fade = (1.0 - t_out).clamp(0.0, 1.0);
                            
                            if fade > 0.0 {
                                // Transition from Dark Fill (0.85) to Border (1.0)
                                // At d=0 (boundary), we mix.
                                let t_in = (d * 2.0).clamp(0.0, 1.0); // Rapid blend over 0.5px
                                final_alpha = (0.85 * (1.0 - t_in)) + (1.0 * t_in);
                                final_alpha *= fade;
                                
                                // Color blend: Dark(0x111111) -> Grey(0xAAAAAA)
                                // Simplification: Just set to Grey for d>0 to keep it crisp
                                final_col = 0x00AAAAAA; 
                            }
                        }
                    } else {
                        // == INSIDE THE BOX (d <= 0) ==
                        if is_glowing {
                            // PROCESSING: Inner Rainbow Glow
                            let angle = py.atan2(px);
                            // OPTIMIZATION: Reuse pre-calculated trig values
                            let noise = (angle * freq1 + time_sin_2).sin() * 0.5
                                      + (angle * freq2 + time_sin_neg_3).sin() * 0.4;

                            let local_glow_width = dynamic_base_scale + (noise * (dynamic_base_scale * 0.65));
                            let dist_in = d.abs();

                            let t = (dist_in / local_glow_width).clamp(0.0, 1.0);
                            let intensity = (1.0 - t).powi(3);

                            final_alpha = intensity;
                            if dist_in < 4.0 { final_alpha = 1.0; } // Solid white core edge

                            if final_alpha > 0.005 {
                                let deg = angle.to_degrees() + 180.0;
                                let hue = (deg + time_offset) % 360.0;
                                let rgb = hsv_to_rgb(hue, 0.8, 1.0);
                                if dist_in < 2.0 { final_col = 0x00FFFFFF; } else { final_col = rgb; }
                            }
                        } else {
                            // DRAGGING: Dark Interior
                            final_alpha = 0.85;
                            final_col = 0x00111111;
                        }
                    }

                    if final_alpha > 0.0 {
                        let a = (final_alpha * 255.0) as u32;
                        let r = ((final_col >> 16) & 0xFF) as f32;
                        let g = ((final_col >> 8) & 0xFF) as f32;
                        let b = (final_col & 0xFF) as f32;
                        
                        let r_pre = ((r * final_alpha) as u32).min(255);
                        let g_pre = ((g * final_alpha) as u32).min(255);
                        let b_pre = ((b * final_alpha) as u32).min(255);
                        
                        pixels[idx] = (a << 24) | (r_pre << 16) | (g_pre << 8) | b_pre;
                    } else {
                        pixels[idx] = 0;
                    }
                }
            }
        }
        
        let mem_dc = CreateCompatibleDC(hdc_dest);
        let old_bmp = SelectObject(mem_dc, hbm);
        let _ = BitBlt(hdc_dest, bounds.left - pad, bounds.top - pad, buf_w, buf_h, mem_dc, 0, 0, SRCCOPY);
        SelectObject(mem_dc, old_bmp);
        DeleteDC(mem_dc);
    }
    DeleteObject(hbm);
}
