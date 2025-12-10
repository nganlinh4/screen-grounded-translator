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

// OPTIMIZED DRAWING: Writes directly to cached buffer
// Defers expensive atan2 until after visibility check
pub unsafe fn draw_direct_sdf_glow(
    pixels_ptr: *mut u32, 
    w: i32, 
    h: i32, 
    time_offset: f32,
    alpha_mult: f32,
    is_glowing: bool
) {
    if pixels_ptr.is_null() { return; }
    
    let pixels = std::slice::from_raw_parts_mut(pixels_ptr, (w * h) as usize);
    let bx = (w as f32) / 2.0;
    let by = (h as f32) / 2.0;
    let center_x = bx;
    let center_y = by;
    let eff_radius = CORNER_RADIUS.min(bx).min(by);

    // ADAPTIVE GLOW SCALING: Scale based on window size to keep center hollow
    // For small windows (e.g., 100px): scales to ~20px glow
    // For large windows (e.g., 600px+): scales to 60px glow
    let min_dim = (w as f32).min(h as f32);
    let dynamic_base_scale = (min_dim * 0.2).clamp(20.0, 60.0);

    // Optimization: Skip the middle of the box to save CPU
    // Increase margin slightly to ensure we don't process internal pixels
    let safe_margin = 85.0;
    let skip_min_x = center_x - bx + safe_margin;
    let skip_max_x = center_x + bx - safe_margin;
    let skip_min_y = center_y - by + safe_margin;
    let skip_max_y = center_y + by - safe_margin;
    
    // Pre-calculate constants outside loop
    let time_rad = time_offset.to_radians();
    let glow_threshold = -dynamic_base_scale * 1.5;

    for y in 0..h {
        let py = (y as f32) - center_y;
        let fy = y as f32;
        let is_y_safe = fy > skip_min_y && fy < skip_max_y;
        
        for x in 0..w {
            let idx = (y * w + x) as usize;
            
            // Fast Path: Clear middle pixels and skip math entirely
            if is_y_safe && (x as f32) > skip_min_x && (x as f32) < skip_max_x {
                pixels[idx] = 0;
                continue;
            }

            let px = (x as f32) - center_x;
            
            // Standard SDF Logic
            let qx = px.abs() - bx + eff_radius;
            let qy = py.abs() - by + eff_radius;
            let d = if qx > 0.0 && qy > 0.0 { 
                ((qx * qx + qy * qy).sqrt()) - eff_radius 
            } else { 
                qx.max(qy) - eff_radius 
            };

            if d > 0.0 {
                // Border Logic (Outer edge)
                // Use explicit bounds to avoid expensive clamp if outside
                if d > 2.0 {
                     pixels[idx] = 0;
                     continue;
                }
                
                let t = (d / 2.0).clamp(0.0, 1.0);
                let aa = 1.0 - t * t * (3.0 - 2.0 * t);
                
                if aa > 0.0 {
                     let a = (aa * 255.0 * alpha_mult) as u32;
                     pixels[idx] = (a << 24) | (a << 16) | (a << 8) | a;
                } else {
                     pixels[idx] = 0;
                }
            } else {
                // Inner Glow Logic
                if !is_glowing || d < glow_threshold {
                    pixels[idx] = 0;
                } else {
                    let dist_in = d.abs();
                    
                    // OPTIMIZATION: Lazy Math
                    // Only calculate atan2/sin/cos (Expensive!) if pixel is actually visible.
                    // Use rough intensity check first to skip pixels that are too faint.
                    
                    let t_rough = (dist_in / (dynamic_base_scale * 1.4)).clamp(0.0, 1.0);
                    let base_intensity_rough = (1.0 - t_rough).powi(3);
                    
                    if base_intensity_rough < 0.005 {
                        pixels[idx] = 0;
                        continue;
                    }

                    // Now perform expensive math only for visible pixels
                    let angle = py.atan2(px);
                    let noise = (angle * 4.0 + time_rad * 2.0).sin() * 0.5; 
                    let local_glow_width = dynamic_base_scale + (noise * (dynamic_base_scale * 0.4));
                    
                    let t = (dist_in / local_glow_width).clamp(0.0, 1.0);
                    let intensity = (1.0 - t).powi(3);
                    let final_alpha = if dist_in < 3.0 { 1.0 } else { intensity };
                    
                    if final_alpha > 0.005 {
                         let deg = angle.to_degrees() + 180.0;
                         let hue = (deg + time_offset) % 360.0;
                         
                         let rgb = if dist_in < 2.5 { 0x00FFFFFF } else { hsv_to_rgb(hue, 0.8, 1.0) };
                         
                         let a = (final_alpha * 255.0 * alpha_mult) as u32;
                         let r = ((rgb >> 16) & 0xFF) * a / 255;
                         let g = ((rgb >> 8) & 0xFF) * a / 255;
                         let b = (rgb & 0xFF) * a / 255;
                         
                         pixels[idx] = (a << 24) | (r << 16) | (g << 8) | b;
                    } else {
                        pixels[idx] = 0;
                    }
                }
            }
        }
    }
}

// Deprecated but kept for compatibility if needed elsewhere
pub unsafe fn render_box_sdf(hdc_dest: HDC, bounds: RECT, w: i32, h: i32, is_glowing: bool, time_offset: f32) {
    let pad = 60; 
    let buf_w = w + (pad * 2);
    let buf_h = h + (pad * 2);
    
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
    if CreateDIBSection(hdc_dest, &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0).is_err() { return; }
    
    draw_direct_sdf_glow(p_bits as *mut u32, buf_w, buf_h, time_offset, 1.0, is_glowing);
}
