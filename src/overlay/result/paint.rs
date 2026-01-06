use super::layout::should_show_buttons;
use super::state::{ResizeEdge, WINDOW_STATES};
use crate::overlay::broom_assets::{render_procedural_broom, BroomRenderParams, BROOM_H, BROOM_W};
use crate::overlay::paint_utils::{hsv_to_rgb, sd_rounded_box};
use std::mem::size_of;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// Helper: Measure text dimensions (Height AND Width)
unsafe fn measure_text_bounds(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    text: &mut [u16],
    font_size: i32,
    max_width: i32,
) -> (i32, i32) {
    let hfont = CreateFontW(
        font_size,
        0,
        0,
        0,
        FW_MEDIUM.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
        w!("Google Sans Flex"),
    );
    let old_font = SelectObject(hdc, hfont.into());

    // We start with the max width constraint.
    // DT_CALCRECT will expand the 'right' value if a single word is wider than max_width (unless we handle it),
    // or wrap lines which increases 'bottom'.
    let mut calc_rect = RECT {
        left: 0,
        top: 0,
        right: max_width,
        bottom: 0,
    };

    // DT_EDITCONTROL helps simulate multiline text box behavior
    DrawTextW(
        hdc,
        text,
        &mut calc_rect,
        DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL,
    );

    SelectObject(hdc, old_font);
    let _ = DeleteObject(hfont.into());

    // Return (Height, Width)
    (calc_rect.bottom, calc_rect.right)
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
        let hbm = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(
                pixels.as_ptr() as *const u8,
                bits as *mut u8,
                pixels.len() * 4,
            );
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
    (dx * dx + dy * dy).sqrt()
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
        let _ = GetClientRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        // --- PHASE 1: STATE SNAPSHOT & CACHE MANAGEMENT ---
        let (
            bg_color_u32,
            is_hovered,
            on_copy_btn,
            copy_success,
            on_edit_btn,
            on_undo_btn,
            on_redo_btn,
            on_markdown_btn,
            is_markdown_mode,
            is_browsing,
            on_back_btn,
            on_forward_btn,
            on_download_btn,
            on_speaker_btn,
            is_speaking,
            tts_loading,
            broom_data,
            particles,
            mut cached_text_bm,
            _cached_font_size,
            cache_dirty,
            cached_bg_bm,
            is_refining,
            is_streaming_active,
            anim_offset,
            history_count,
            redo_count,
            navigation_depth,
            max_navigation_depth,
            graphics_mode,
            preset_prompt,
            input_text,
        ) = {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                // 1.1 Update Background Cache if needed
                if state.bg_bitmap.is_invalid() || state.bg_w != width || state.bg_h != height {
                    if !state.bg_bitmap.is_invalid() {
                        let _ = DeleteObject(state.bg_bitmap.into());
                    }

                    let bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: width,
                            biHeight: -height,
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0 as u32,
                            ..Default::default()
                        },
                        ..Default::default()
                    };

                    let mut p_bg_bits: *mut core::ffi::c_void = std::ptr::null_mut();
                    let hbm_bg =
                        CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut p_bg_bits, None, 0)
                            .unwrap();

                    if !p_bg_bits.is_null() {
                        let pixels = std::slice::from_raw_parts_mut(
                            p_bg_bits as *mut u32,
                            (width * height) as usize,
                        );
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

                let particles_vec: Vec<(f32, f32, f32, f32, u32)> = state
                    .physics
                    .particles
                    .iter()
                    .map(|p| (p.x, p.y, p.life, p.size, p.color))
                    .collect();

                // AGGRESSIVE FIX: Don't render broom during ANY closing animation (Smashing OR DragOut)
                // This completely eliminates the "frozen frame" issue during fade
                // The broom is only shown on hover, not during click-to-close
                let is_closing = false;

                let show_broom = !is_closing
                    && (state.is_hovered
                        && !state.on_copy_btn
                        && !state.on_edit_btn
                        && !state.on_undo_btn
                        && !state.on_redo_btn
                        && !state.on_markdown_btn
                        && !state.on_back_btn
                        && !state.on_forward_btn
                        && !state.on_download_btn
                        && !state.on_speaker_btn
                        && state.current_resize_edge == ResizeEdge::None);

                let broom_info = if show_broom {
                    Some((
                        state.physics.x,
                        state.physics.y,
                        BroomRenderParams {
                            tilt_angle: state.physics.current_tilt,
                            squish: state.physics.squish_factor,
                            bend: state.physics.bristle_bend,
                            opacity: 1.0,
                        },
                    ))
                } else {
                    None
                };

                // Check if TTS is currently speaking for this window
                let is_speaking = state.tts_request_id != 0
                    && crate::api::tts::TTS_MANAGER.is_speaking(state.tts_request_id);

                (
                    state.bg_color,
                    state.is_hovered,
                    state.on_copy_btn,
                    state.copy_success,
                    state.on_edit_btn,
                    state.on_undo_btn,
                    state.on_redo_btn,
                    state.on_markdown_btn,
                    state.is_markdown_mode,
                    state.is_browsing,
                    state.on_back_btn,
                    state.on_forward_btn,
                    state.on_download_btn,
                    state.on_speaker_btn,
                    is_speaking,
                    state.tts_loading,
                    broom_info,
                    particles_vec,
                    state.content_bitmap,
                    state.cached_font_size as i32,
                    state.font_cache_dirty,
                    state.bg_bitmap,
                    state.is_refining,
                    state.is_streaming_active,
                    state.animation_offset,
                    state.text_history.len(),
                    state.redo_history.len(),
                    state.navigation_depth,
                    state.max_navigation_depth,
                    state.graphics_mode.clone(),
                    state.preset_prompt.clone(),
                    state.input_text.clone(),
                )
            } else {
                (
                    0,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    false,
                    None,
                    Vec::new(),
                    HBITMAP::default(),
                    72,
                    true,
                    HBITMAP::default(),
                    false,
                    false,
                    0.0,
                    0,
                    0,
                    0,
                    0,
                    "standard".to_string(),
                    String::new(),
                    String::new(),
                )
            }
        };

        // --- PHASE 2: COMPOSITOR SETUP ---
        let mem_dc = CreateCompatibleDC(Some(hdc));

        let bmi_scratch = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut scratch_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let scratch_bitmap = CreateDIBSection(
            Some(hdc),
            &bmi_scratch,
            DIB_RGB_COLORS,
            &mut scratch_bits,
            None,
            0,
        )
        .unwrap();
        let old_scratch = SelectObject(mem_dc, scratch_bitmap.into());

        // 2.1 Copy Background
        if !cached_bg_bm.is_invalid() {
            let cache_dc = CreateCompatibleDC(Some(hdc));
            let old_cbm = SelectObject(cache_dc, cached_bg_bm.into());
            let _ = BitBlt(mem_dc, 0, 0, width, height, Some(cache_dc), 0, 0, SRCCOPY).ok();
            SelectObject(cache_dc, old_cbm);
            let _ = DeleteDC(cache_dc);
        }

        if !is_markdown_mode {
            if cache_dirty || cached_text_bm.is_invalid() {
                if !cached_text_bm.is_invalid() {
                    let _ = DeleteObject(cached_text_bm.into());
                }

                cached_text_bm = CreateCompatibleBitmap(hdc, width, height);
                let cache_dc = CreateCompatibleDC(Some(hdc));
                let old_cache_bm = SelectObject(cache_dc, cached_text_bm.into());

                let dark_brush = CreateSolidBrush(COLORREF(bg_color_u32));
                let fill_rect = RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                FillRect(cache_dc, &fill_rect, dark_brush);
                let _ = DeleteObject(dark_brush.into());

                SetBkMode(cache_dc, TRANSPARENT);
                SetTextColor(cache_dc, COLORREF(0x00FFFFFF));

                let mut buf = if is_refining {
                    if !crate::overlay::utils::SHOW_REFINING_CONTEXT_QUOTE {
                        vec![0u16; 1] // Return empty buffer
                    } else {
                        let combined = if input_text.is_empty() {
                            preset_prompt.clone()
                        } else {
                            format!("{}\n\n{}", preset_prompt, input_text)
                        };
                        let quote = crate::overlay::utils::get_context_quote(&combined);
                        quote
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect::<Vec<u16>>()
                    }
                } else {
                    let text_len = GetWindowTextLengthW(hwnd) + 1;
                    let mut b = vec![0u16; text_len as usize];
                    GetWindowTextW(hwnd, &mut b);
                    b
                };

                let h_padding = if is_refining { 20 } else { 2 };
                let available_w = (width - (h_padding * 2)).max(1);
                let v_safety_margin = 0;
                let available_h = (height - v_safety_margin).max(1);

                let mut low = if is_refining { 8 } else { 8 };
                let max_possible = if is_refining {
                    18.min(available_h)
                } else {
                    available_h.min(100)
                };
                let mut high = max_possible;
                let mut best_fit = 8;

                if high < low {
                    best_fit = low;
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

                let font_weight = if is_refining { FW_NORMAL } else { FW_MEDIUM };
                let hfont = CreateFontW(
                    font_size_val,
                    0,
                    0,
                    0,
                    font_weight.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
                    w!("Google Sans Flex"),
                );
                let old_font = SelectObject(cache_dc, hfont.into());

                let mut measure_rect = RECT {
                    left: 0,
                    top: 0,
                    right: available_w,
                    bottom: 0,
                };
                DrawTextW(
                    cache_dc,
                    &mut buf,
                    &mut measure_rect,
                    DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL,
                );
                let text_h = measure_rect.bottom;

                let offset_y = ((height - text_h) / 2).max(0);
                let mut draw_rect = RECT {
                    left: h_padding,
                    top: offset_y,
                    right: width - h_padding,
                    bottom: height,
                };

                let draw_flags = if is_refining {
                    DT_CENTER | DT_WORDBREAK | DT_EDITCONTROL
                } else {
                    DT_LEFT | DT_WORDBREAK | DT_EDITCONTROL
                };
                DrawTextW(cache_dc, &mut buf, &mut draw_rect as *mut _, draw_flags);

                SelectObject(cache_dc, old_font);
                let _ = DeleteObject(hfont.into());
                SelectObject(cache_dc, old_cache_bm);
                let _ = DeleteDC(cache_dc);

                let mut states = WINDOW_STATES.lock().unwrap();
                if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                    state.content_bitmap = cached_text_bm;
                    state.cached_font_size = font_size_val;
                    state.font_cache_dirty = false;
                }
            }

            if !cached_text_bm.is_invalid() {
                let cache_dc = CreateCompatibleDC(Some(hdc));
                let old_cbm = SelectObject(cache_dc, cached_text_bm.into());
                let _ = BitBlt(mem_dc, 0, 0, width, height, Some(cache_dc), 0, 0, SRCCOPY).ok();
                SelectObject(cache_dc, old_cbm);
                let _ = DeleteDC(cache_dc);
            }
        }

        // --- PHASE 4: PIXEL MANIPULATION ---
        if !scratch_bits.is_null() {
            let raw_pixels =
                std::slice::from_raw_parts_mut(scratch_bits as *mut u32, (width * height) as usize);

            // 4.0 REFINEMENT GLOW
            if is_refining {
                let is_minimal = graphics_mode == "minimal";

                if is_minimal {
                    // MINIMAL MODE: Bouncing orange scan line (exactly like green laser but orange)
                    // Simple, lightweight, no per-pixel calculation

                    // Calculate scan line position (bounces up and down)
                    // Use abs() because anim_offset can be negative
                    let cycle = (anim_offset.abs() % 360.0) / 180.0; // 0.0 to 2.0
                    let t = if cycle <= 1.0 { cycle } else { 2.0 - cycle }; // 0.0 to 1.0 (bounce)

                    let margin = 3;
                    let scan_range = height - (margin * 2);
                    if scan_range > 0 {
                        let scan_y =
                            margin + ((t * scan_range as f32) as i32).clamp(0, scan_range - 1);

                        // Draw 2px thick orange line
                        for line_offset in 0..2 {
                            let y = scan_y + line_offset;
                            if y > 0 && y < height - 1 {
                                for x in margin..(width - margin) {
                                    let idx = (y * width + x) as usize;
                                    if idx < raw_pixels.len() {
                                        // Blend orange with background
                                        let bg_px = raw_pixels[idx];
                                        let bg_b = (bg_px & 0xFF) as f32;
                                        let bg_g = ((bg_px >> 8) & 0xFF) as f32;
                                        let bg_r = ((bg_px >> 16) & 0xFF) as f32;

                                        let intensity = 0.9; // Strong but not fully opaque
                                        let out_r =
                                            (255.0 * intensity + bg_r * (1.0 - intensity)) as u32;
                                        let out_g =
                                            (140.0 * intensity + bg_g * (1.0 - intensity)) as u32;
                                        let out_b =
                                            (0.0 * intensity + bg_b * (1.0 - intensity)) as u32;
                                        raw_pixels[idx] =
                                            (255 << 24) | (out_r << 16) | (out_g << 8) | out_b;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // STANDARD MODE: Rainbow edge glow (full per-pixel calculation)
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
                                        let final_intensity =
                                            (base_intensity * noise_mod).clamp(0.0, 1.0);
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

                                            let out_r = (fg_r * final_intensity
                                                + bg_r * (1.0 - final_intensity))
                                                as u32;
                                            let out_g = (fg_g * final_intensity
                                                + bg_g * (1.0 - final_intensity))
                                                as u32;
                                            let out_b = (fg_b * final_intensity
                                                + bg_b * (1.0 - final_intensity))
                                                as u32;
                                            raw_pixels[idx] =
                                                (255 << 24) | (out_r << 16) | (out_g << 8) | out_b;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 4.1 Particles
            for (d_x, d_y, life, size, col) in particles {
                if life <= 0.0 {
                    continue;
                }
                let radius = size * life;
                if radius < 0.5 {
                    continue;
                }

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
                        let dist = (dx * dx + dy * dy).sqrt();
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

            // 4.2 Buttons - hide during refining, streaming, or when overlay is too small
            if is_hovered
                && !is_refining
                && !is_streaming_active
                && should_show_buttons(width, height)
            {
                let btn_size = 28;
                let margin = 12;
                let threshold_h = btn_size + (margin * 2);
                let cy = if height < threshold_h {
                    (height as f32) / 2.0
                } else {
                    (height - margin - btn_size / 2) as f32
                };

                // Button positions - used differently based on browsing mode
                let cx_back = (margin + btn_size / 2) as f32;
                let cx_forward = (width - margin - btn_size / 2) as f32; // Forward on right when browsing

                // Result UI button positions (only used when not browsing)
                // Order from right to left: Copy -> Speaker -> Edit -> Markdown -> Download -> Undo -> Redo
                let cx_copy = (width - margin - btn_size / 2) as f32;
                let cx_speaker = cx_copy - (btn_size as f32) - 8.0;
                let cx_edit = cx_speaker - (btn_size as f32) - 8.0;
                let cx_md = cx_edit - (btn_size as f32) - 8.0;
                let cx_dl = cx_md - (btn_size as f32) - 8.0;
                let cx_undo = cx_dl - (btn_size as f32) - 8.0;
                let cx_redo = cx_undo - (btn_size as f32) - 8.0;

                let radius = 13.0;

                // Color configuration
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
                let (tr_rd, tg_rd, tb_rd) = if on_redo_btn {
                    (128.0, 128.0, 128.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                let (tr_m, tg_m, tb_m) = if is_markdown_mode {
                    (60.0, 180.0, 200.0)
                } else if on_markdown_btn {
                    (100.0, 140.0, 180.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                let (tr_b, tg_b, tb_b) = if on_back_btn {
                    (128.0, 128.0, 128.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                let (tr_f, tg_f, tb_f) = if on_forward_btn {
                    (128.0, 128.0, 128.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                let (tr_dl, tg_dl, tb_dl) = if on_download_btn {
                    (100.0, 180.0, 100.0)
                } else {
                    (80.0, 80.0, 80.0)
                };
                // Speaker button: orange when loading, blue when speaking, gray when idle
                let (tr_sp, tg_sp, tb_sp) = if tts_loading {
                    (255.0, 180.0, 50.0) // Orange/yellow for loading
                } else if is_speaking {
                    (80.0, 150.0, 220.0) // Blue for speaking
                } else if on_speaker_btn {
                    (128.0, 128.0, 128.0) // Highlight on hover
                } else {
                    (80.0, 80.0, 80.0) // Default gray
                };

                let b_start_y = (cy - radius - 4.0) as i32;
                let b_end_y = (cy + radius + 4.0) as i32;
                let show_undo = history_count > 0 && !is_browsing;
                let show_redo = redo_count > 0 && !is_browsing;
                let show_forward = is_browsing && navigation_depth < max_navigation_depth;
                let show_speaker = !is_browsing; // Always show speaker when not browsing
                let border_inner_radius = radius - 1.5;

                for y in b_start_y.max(0)..b_end_y.min(height) {
                    for x in 0..width {
                        let fx = x as f32;
                        let fy = y as f32;
                        let dy = (fy - cy).abs();

                        let mut hit = false;
                        let mut t_r = 0.0;
                        let mut t_g = 0.0;
                        let mut t_b = 0.0;
                        let mut alpha = 0.0;
                        let mut border_alpha = 0.0;
                        let mut icon_alpha = 0.0;

                        if is_browsing {
                            // BROWSING MODE: Only show Back (left) and Forward (right) buttons

                            // BACK BUTTON (Left side)
                            if x < width / 2 {
                                let dx = (fx - cx_back).abs();
                                let dist = (dx * dx + dy * dy).sqrt();
                                let aa = (radius + 0.5 - dist).clamp(0.0, 1.0);
                                if aa > 0.0 {
                                    hit = true;
                                    alpha = aa;
                                    t_r = tr_b;
                                    t_g = tg_b;
                                    t_b = tb_b;
                                    border_alpha = ((radius + 0.5 - dist).clamp(0.0, 1.0)
                                        * ((dist - (border_inner_radius - 0.5)).clamp(0.0, 1.0)))
                                        * 0.6;

                                    // Back Arrow (Left Arrow)
                                    let tip_x = cx_back - 3.5;
                                    let tail_x = cx_back + 3.5;
                                    let d_shaft = dist_segment(fx, fy, tip_x, cy, tail_x, cy);
                                    let d_wing1 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x + 3.0, cy - 3.0);
                                    let d_wing2 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x + 3.0, cy + 3.0);
                                    let d_arrow = d_shaft.min(d_wing1).min(d_wing2);
                                    icon_alpha = (1.3 - d_arrow).clamp(0.0, 1.0);
                                }
                            }

                            // FORWARD BUTTON (Right side)
                            if !hit && show_forward && x > width / 2 {
                                let dx = (fx - cx_forward).abs();
                                let dist = (dx * dx + dy * dy).sqrt();
                                let aa = (radius + 0.5 - dist).clamp(0.0, 1.0);
                                if aa > 0.0 {
                                    hit = true;
                                    alpha = aa;
                                    t_r = tr_f;
                                    t_g = tg_f;
                                    t_b = tb_f;
                                    border_alpha = ((radius + 0.5 - dist).clamp(0.0, 1.0)
                                        * ((dist - (border_inner_radius - 0.5)).clamp(0.0, 1.0)))
                                        * 0.6;

                                    // Forward Arrow (Right Arrow)
                                    let tip_x = cx_forward + 3.5;
                                    let tail_x = cx_forward - 3.5;
                                    let d_shaft = dist_segment(fx, fy, tail_x, cy, tip_x, cy);
                                    let d_wing1 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x - 3.0, cy - 3.0);
                                    let d_wing2 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x - 3.0, cy + 3.0);
                                    let d_arrow = d_shaft.min(d_wing1).min(d_wing2);
                                    icon_alpha = (1.3 - d_arrow).clamp(0.0, 1.0);
                                }
                            }
                        } else {
                            // RESULT MODE: Show all result UI buttons on the right side

                            // COPY
                            let dx_c = (fx - cx_copy).abs();
                            let dist_c = (dx_c * dx_c + dy * dy).sqrt();
                            let aa_c = (radius + 0.5 - dist_c).clamp(0.0, 1.0);
                            if aa_c > 0.0 {
                                hit = true;
                                alpha = aa_c;
                                t_r = tr_c;
                                t_g = tg_c;
                                t_b = tb_c;
                                border_alpha = ((radius + 0.5 - dist_c).clamp(0.0, 1.0)
                                    * ((dist_c - (border_inner_radius - 0.5)).clamp(0.0, 1.0)))
                                    * 0.6;
                                if copy_success {
                                    let d1 = dist_segment(
                                        fx,
                                        fy,
                                        cx_copy - 4.0,
                                        cy,
                                        cx_copy - 1.0,
                                        cy + 3.0,
                                    );
                                    let d2 = dist_segment(
                                        fx,
                                        fy,
                                        cx_copy - 1.0,
                                        cy + 3.0,
                                        cx_copy + 4.0,
                                        cy - 4.0,
                                    );
                                    icon_alpha = (1.8 - d1.min(d2)).clamp(0.0, 1.0);
                                } else {
                                    let back_d = sd_box(fx, fy, cx_copy - 2.0, cy - 2.0, 3.0, 4.0);
                                    let back_outline = (1.25 - back_d.abs()).clamp(0.0, 1.0);
                                    let front_d = sd_box(fx, fy, cx_copy + 2.0, cy + 2.0, 3.0, 4.0);
                                    let front_fill = (0.8 - front_d).clamp(0.0, 1.0);
                                    let mask_d = sd_box(fx, fy, cx_copy + 2.0, cy + 2.0, 4.5, 5.5);
                                    icon_alpha = (front_fill
                                        + back_outline * mask_d.clamp(0.0, 1.0))
                                    .clamp(0.0, 1.0);
                                }
                            }

                            // EDIT
                            if !hit {
                                let dx_e = (fx - cx_edit).abs();
                                let dist_e = (dx_e * dx_e + dy * dy).sqrt();
                                let aa_e = (radius + 0.5 - dist_e).clamp(0.0, 1.0);
                                if aa_e > 0.0 {
                                    hit = true;
                                    alpha = aa_e;
                                    t_r = tr_e;
                                    t_g = tg_e;
                                    t_b = tb_e;
                                    border_alpha = ((radius + 0.5 - dist_e).clamp(0.0, 1.0)
                                        * ((dist_e - (border_inner_radius - 0.5)).clamp(0.0, 1.0)))
                                        * 0.6;
                                    let sx = (fx - cx_edit).abs();
                                    let sy = (fy - cy).abs();
                                    let star_dist =
                                        (sx.powf(0.6) + sy.powf(0.6)).powf(1.0 / 0.6) - 4.5;
                                    let mut ia = (1.2 - star_dist).clamp(0.0, 1.0);
                                    let sx2 = (fx - (cx_edit + 4.5)).abs();
                                    let sy2 = (fy - (cy - 3.5)).abs();
                                    let star2_dist =
                                        (sx2.powf(0.6) + sy2.powf(0.6)).powf(1.0 / 0.6) - 2.2;
                                    ia = ia.max((1.2 - star2_dist).clamp(0.0, 1.0));
                                    icon_alpha = ia;
                                }
                            }

                            // MARKDOWN
                            if !hit {
                                let dx_m = (fx - cx_md).abs();
                                let dist_m = (dx_m * dx_m + dy * dy).sqrt();
                                let aa_m = (radius + 0.5 - dist_m).clamp(0.0, 1.0);
                                if aa_m > 0.0 {
                                    hit = true;
                                    alpha = aa_m;
                                    t_r = tr_m;
                                    t_g = tg_m;
                                    t_b = tb_m;
                                    border_alpha = ((radius + 0.5 - dist_m).clamp(0.0, 1.0)
                                        * ((dist_m - (border_inner_radius - 0.5)).clamp(0.0, 1.0)))
                                        * 0.6;
                                    let d_m1 = dist_segment(
                                        fx,
                                        fy,
                                        cx_md - 4.0,
                                        cy + 4.0,
                                        cx_md - 4.0,
                                        cy - 4.0,
                                    );
                                    let d_m2 = dist_segment(
                                        fx,
                                        fy,
                                        cx_md - 4.0,
                                        cy - 4.0,
                                        cx_md,
                                        cy + 1.0,
                                    );
                                    let d_m3 = dist_segment(
                                        fx,
                                        fy,
                                        cx_md,
                                        cy + 1.0,
                                        cx_md + 4.0,
                                        cy - 4.0,
                                    );
                                    let d_m4 = dist_segment(
                                        fx,
                                        fy,
                                        cx_md + 4.0,
                                        cy - 4.0,
                                        cx_md + 4.0,
                                        cy + 4.0,
                                    );
                                    let d_m = d_m1.min(d_m2).min(d_m3).min(d_m4);
                                    icon_alpha = (1.5 - d_m).clamp(0.0, 1.0);
                                }
                            }

                            // DOWNLOAD
                            if !hit {
                                let dx_dl = (fx - cx_dl).abs();
                                let dist_dl = (dx_dl * dx_dl + dy * dy).sqrt();
                                let aa_dl = (radius + 0.5 - dist_dl).clamp(0.0, 1.0);
                                if aa_dl > 0.0 {
                                    hit = true;
                                    alpha = aa_dl;
                                    t_r = tr_dl;
                                    t_g = tg_dl;
                                    t_b = tb_dl;
                                    border_alpha = ((radius + 0.5 - dist_dl).clamp(0.0, 1.0)
                                        * ((dist_dl - (border_inner_radius - 0.5))
                                            .clamp(0.0, 1.0)))
                                        * 0.6;
                                    let d_line =
                                        dist_segment(fx, fy, cx_dl, cy - 4.0, cx_dl, cy + 2.0);
                                    let d_arrow1 = dist_segment(
                                        fx,
                                        fy,
                                        cx_dl - 3.5,
                                        cy - 0.5,
                                        cx_dl,
                                        cy + 3.5,
                                    );
                                    let d_arrow2 = dist_segment(
                                        fx,
                                        fy,
                                        cx_dl + 3.5,
                                        cy - 0.5,
                                        cx_dl,
                                        cy + 3.5,
                                    );
                                    let d_tray = dist_segment(
                                        fx,
                                        fy,
                                        cx_dl - 4.0,
                                        cy + 4.5,
                                        cx_dl + 4.0,
                                        cy + 4.5,
                                    );
                                    let d_icon = d_line.min(d_arrow1).min(d_arrow2).min(d_tray);
                                    icon_alpha = (1.5 - d_icon).clamp(0.0, 1.0);
                                }
                            }

                            // UNDO
                            if !hit && show_undo {
                                let dx_u = (fx - cx_undo).abs();
                                let dist_u = (dx_u * dx_u + dy * dy).sqrt();
                                let aa_u = (radius + 0.5 - dist_u).clamp(0.0, 1.0);
                                if aa_u > 0.0 {
                                    hit = true;
                                    alpha = aa_u;
                                    t_r = tr_u;
                                    t_g = tg_u;
                                    t_b = tb_u;
                                    border_alpha = ((radius + 0.5 - dist_u).clamp(0.0, 1.0)
                                        * ((dist_u - (border_inner_radius - 0.5)).clamp(0.0, 1.0)))
                                        * 0.6;
                                    let tip_x = cx_undo - 3.5;
                                    let tail_x = cx_undo + 3.5;
                                    let d_shaft = dist_segment(fx, fy, tip_x, cy, tail_x, cy);
                                    let d_wing1 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x + 3.0, cy - 3.0);
                                    let d_wing2 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x + 3.0, cy + 3.0);
                                    let d_arrow = d_shaft.min(d_wing1).min(d_wing2);
                                    icon_alpha = (1.3 - d_arrow).clamp(0.0, 1.0);
                                }
                            }

                            // REDO
                            if !hit && show_redo {
                                let dx_rd = (fx - cx_redo).abs();
                                let dist_rd = (dx_rd * dx_rd + dy * dy).sqrt();
                                let aa_rd = (radius + 0.5 - dist_rd).clamp(0.0, 1.0);
                                if aa_rd > 0.0 {
                                    hit = true;
                                    alpha = aa_rd;
                                    t_r = tr_rd;
                                    t_g = tg_rd;
                                    t_b = tb_rd;
                                    border_alpha = ((radius + 0.5 - dist_rd).clamp(0.0, 1.0)
                                        * ((dist_rd - (border_inner_radius - 0.5))
                                            .clamp(0.0, 1.0)))
                                        * 0.6;
                                    let tip_x = cx_redo + 3.5;
                                    let tail_x = cx_redo - 3.5;
                                    let d_shaft = dist_segment(fx, fy, tail_x, cy, tip_x, cy);
                                    let d_wing1 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x - 3.0, cy - 3.0);
                                    let d_wing2 =
                                        dist_segment(fx, fy, tip_x, cy, tip_x - 3.0, cy + 3.0);
                                    let d_arrow = d_shaft.min(d_wing1).min(d_wing2);
                                    icon_alpha = (1.3 - d_arrow).clamp(0.0, 1.0);
                                }
                            }

                            // SPEAKER (TTS)
                            if !hit && show_speaker {
                                let dx_sp = (fx - cx_speaker).abs();
                                let dist_sp = (dx_sp * dx_sp + dy * dy).sqrt();
                                let aa_sp = (radius + 0.5 - dist_sp).clamp(0.0, 1.0);
                                if aa_sp > 0.0 {
                                    hit = true;
                                    alpha = aa_sp;
                                    t_r = tr_sp;
                                    t_g = tg_sp;
                                    t_b = tb_sp;
                                    border_alpha = ((radius + 0.5 - dist_sp).clamp(0.0, 1.0)
                                        * ((dist_sp - (border_inner_radius - 0.5))
                                            .clamp(0.0, 1.0)))
                                        * 0.6;

                                    // Speaker icon: cone + sound waves
                                    // Speaker cone (left side)
                                    let cone_l = cx_speaker - 4.0;
                                    let cone_r = cx_speaker - 1.0;
                                    let cone_t = cy - 2.0;
                                    let cone_b = cy + 2.0;
                                    let d_cone = sd_box(
                                        fx,
                                        fy,
                                        (cone_l + cone_r) / 2.0,
                                        cy,
                                        (cone_r - cone_l) / 2.0,
                                        (cone_b - cone_t) / 2.0,
                                    );

                                    // Speaker "bell" (trapezoid-ish, made with lines)
                                    let d_bell1 = dist_segment(
                                        fx,
                                        fy,
                                        cone_r,
                                        cone_t,
                                        cone_r + 2.5,
                                        cy - 4.0,
                                    );
                                    let d_bell2 = dist_segment(
                                        fx,
                                        fy,
                                        cone_r + 2.5,
                                        cy - 4.0,
                                        cone_r + 2.5,
                                        cy + 4.0,
                                    );
                                    let d_bell3 = dist_segment(
                                        fx,
                                        fy,
                                        cone_r + 2.5,
                                        cy + 4.0,
                                        cone_r,
                                        cone_b,
                                    );
                                    let d_bell = d_bell1.min(d_bell2).min(d_bell3);

                                    // Sound waves (arcs to the right)
                                    let wave_cx = cx_speaker + 2.0;
                                    let px = fx - wave_cx;
                                    let py_wave = fy - cy;
                                    let angle = py_wave.atan2(px);

                                    // Only draw waves on the right side (facing direction)
                                    let mut d_wave = 100.0f32;
                                    if px > 0.0 && angle.abs() < std::f32::consts::FRAC_PI_3 {
                                        let dist_from_center = (px * px + py_wave * py_wave).sqrt();
                                        // Two wave arcs at different distances
                                        let d_wave1 = (dist_from_center - 3.5).abs() - 0.8;
                                        let d_wave2 = (dist_from_center - 6.0).abs() - 0.8;
                                        d_wave = d_wave1.min(d_wave2);
                                    }

                                    let d_speaker = d_cone.min(d_bell).min(d_wave);
                                    icon_alpha = (1.5 - d_speaker).clamp(0.0, 1.0);
                                }
                            }
                        }

                        if hit {
                            let idx = (y * width + x) as usize;
                            let bg = raw_pixels[idx];
                            let bg_b = (bg & 0xFF) as f32;
                            let bg_g = ((bg >> 8) & 0xFF) as f32;
                            let bg_r = ((bg >> 16) & 0xFF) as f32;

                            let mut final_r = bg_r;
                            let mut final_g = bg_g;
                            let mut final_b = bg_b;

                            if alpha > 0.0 {
                                let a = 0.9 * alpha;
                                final_r = t_r * a + final_r * (1.0 - a);
                                final_g = t_g * a + final_g * (1.0 - a);
                                final_b = t_b * a + final_b * (1.0 - a);
                            }
                            if border_alpha > 0.0 {
                                final_r += 255.0 * border_alpha;
                                final_g += 255.0 * border_alpha;
                                final_b += 255.0 * border_alpha;
                            }
                            if icon_alpha > 0.0 {
                                final_r = 255.0 * icon_alpha + final_r * (1.0 - icon_alpha);
                                final_g = 255.0 * icon_alpha + final_g * (1.0 - icon_alpha);
                                final_b = 255.0 * icon_alpha + final_b * (1.0 - icon_alpha);
                            }

                            raw_pixels[idx] = (255 << 24)
                                | ((final_r.min(255.0) as u32) << 16)
                                | ((final_g.min(255.0) as u32) << 8)
                                | (final_b.min(255.0) as u32);
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
        } else {
            None
        };

        if let Some((px, py, hbm)) = broom_bitmap_data {
            if !hbm.is_invalid() {
                let broom_dc = CreateCompatibleDC(Some(hdc));
                let old_hbm_broom = SelectObject(broom_dc, hbm.into());
                let mut bf = BLENDFUNCTION::default();
                bf.BlendOp = AC_SRC_OVER as u8;
                bf.SourceConstantAlpha = 255;
                bf.AlphaFormat = AC_SRC_ALPHA as u8;
                let draw_x = px as i32 - (BROOM_W / 2);
                let draw_y = py as i32 - (BROOM_H as f32 * 0.65) as i32;
                let _ = GdiAlphaBlend(
                    mem_dc, draw_x, draw_y, BROOM_W, BROOM_H, broom_dc, 0, 0, BROOM_W, BROOM_H, bf,
                );
                SelectObject(broom_dc, old_hbm_broom);
                let _ = DeleteDC(broom_dc);
                let _ = DeleteObject(hbm.into());
            }
        }

        // --- PHASE 6: FINAL BLIT ---
        let _ = BitBlt(hdc, 0, 0, width, height, Some(mem_dc), 0, 0, SRCCOPY).ok();

        SelectObject(mem_dc, old_scratch);
        let _ = DeleteObject(scratch_bitmap.into());
        let _ = DeleteDC(mem_dc);

        let _ = EndPaint(hwnd, &mut ps);
    }
}
