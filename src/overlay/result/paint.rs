use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;
use std::mem::size_of;
use crate::overlay::broom_assets::{BroomState, BROOM_W, BROOM_H};
use super::state::{WINDOW_STATES, AnimationMode, state_to_idx};

// Helper: Convert Vec<u32> to HBITMAP
pub fn create_bitmap_from_pixels(pixels: &[u32]) -> HBITMAP {
    unsafe {
        let hdc = GetDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: BROOM_W,
                biHeight: -BROOM_H, // Top-down
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
            std::ptr::copy_nonoverlapping(
                pixels.as_ptr() as *const u8, 
                bits as *mut u8, 
                pixels.len() * 4
            );
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

        // Fetch State
        let (bg_color, is_hovered, copy_success, render_data) = {
            let mut states = WINDOW_STATES.lock().unwrap();
            if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
                let bmp_idx = match state.physics.mode {
                    AnimationMode::Idle => state.physics.idle_frame,
                    AnimationMode::MovingLeft => state_to_idx(BroomState::Left),
                    AnimationMode::MovingRight => state_to_idx(BroomState::Right),
                    AnimationMode::Sweeping => match state.physics.sweep_stage {
                        0 => state_to_idx(BroomState::Sweep1Windup),
                        1 => state_to_idx(BroomState::Sweep2Smash),
                        2 => state_to_idx(BroomState::Sweep3DragR),
                        3 => state_to_idx(BroomState::Sweep4DragL),
                        _ => state_to_idx(BroomState::Sweep5Lift),
                    }
                };

                let hbm = state.physics.bitmaps.get(&bmp_idx).cloned().unwrap_or(HBITMAP(0));
                
                let particles: Vec<(f32, f32, f32)> = state.physics.particles.iter()
                    .map(|p| (p.x, p.y, p.life)).collect();
                
                // Show Broom if: Hovering AND (Not on button OR Sweeping)
                let show_broom = (state.is_hovered && !state.on_copy_btn) || state.physics.mode == AnimationMode::Sweeping;

                (state.bg_color, state.is_hovered, state.copy_success, 
                 if show_broom {
                     Some((state.physics.x, state.physics.y, hbm, particles))
                 } else {
                     None
                 })
            } else {
                (0x00222222, false, false, None)
            }
        };

        // Draw Background
        let dark_brush = CreateSolidBrush(COLORREF(bg_color));
        FillRect(mem_dc, &rect, dark_brush);
        DeleteObject(dark_brush);
        
        SetBkMode(mem_dc, TRANSPARENT);
        SetTextColor(mem_dc, COLORREF(0x00FFFFFF));

        // Draw Text
        let text_len = GetWindowTextLengthW(hwnd) + 1;
        let mut buf = vec![0u16; text_len as usize];
        GetWindowTextW(hwnd, &mut buf);
        let mut draw_rect = rect;
        draw_rect.left += 5; draw_rect.right -= 5; draw_rect.top += 5;
        
        let hfont = CreateFontW(16, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        let old_font = SelectObject(mem_dc, hfont);
        DrawTextW(mem_dc, &mut buf, &mut draw_rect, DT_LEFT | DT_WORDBREAK);
        SelectObject(mem_dc, old_font);
        DeleteObject(hfont);

        // Draw Copy Button
        if is_hovered {
            let btn_size = 24;
            let btn_rect = RECT { left: width - btn_size, top: height - btn_size, right: width, bottom: height };
            let btn_brush = CreateSolidBrush(COLORREF(0x00444444));
            FillRect(mem_dc, &btn_rect, btn_brush);
            DeleteObject(btn_brush);

            let icon_pen = if copy_success { CreatePen(PS_SOLID, 2, COLORREF(0x0000FF00)) } 
                           else { CreatePen(PS_SOLID, 2, COLORREF(0x00AAAAAA)) };
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

        if let Some((px, py, hbm, particles)) = render_data {
            // Draw Dust
            let particle_brush = CreateSolidBrush(COLORREF(0x00CCCCCC));
            for (d_x, d_y, life) in particles {
                let sz = (life * 4.0) as i32;
                if sz > 0 {
                    let d_rect = RECT { left: d_x as i32, top: d_y as i32, right: d_x as i32 + sz, bottom: d_y as i32 + sz };
                    FillRect(mem_dc, &d_rect, particle_brush);
                }
            }
            DeleteObject(particle_brush);

            // Draw Broom
            if hbm.0 != 0 {
                let broom_dc = CreateCompatibleDC(hdc);
                SelectObject(broom_dc, hbm);
                
                let mut bf = BLENDFUNCTION::default();
                bf.BlendOp = AC_SRC_OVER as u8;
                bf.SourceConstantAlpha = 255;
                bf.AlphaFormat = AC_SRC_ALPHA as u8;

                // Hotspot adjustment (Center Bottom)
                let draw_x = px as i32 - 16;
                let draw_y = py as i32 - 28;

                GdiAlphaBlend(
                    mem_dc, draw_x, draw_y, BROOM_W, BROOM_H,
                    broom_dc, 0, 0, BROOM_W, BROOM_H,
                    bf
                );
                DeleteDC(broom_dc);
            }
        }

        let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY).ok();
        
        SelectObject(mem_dc, old_bitmap);
        DeleteObject(mem_bitmap);
        DeleteDC(mem_dc);
        
        EndPaint(hwnd, &mut ps);
    }
}
