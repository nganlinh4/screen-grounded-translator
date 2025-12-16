// Preset Wheel Overlay - Shows a wheel of preset options for MASTER presets
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::core::*;
use std::sync::{Once, atomic::{AtomicBool, AtomicI32, Ordering}};
use crate::APP;
use crate::config::Preset;
use super::utils::to_wstring;
use crate::gui::settings_ui::get_localized_preset_name;

static REGISTER_WHEEL_CLASS: Once = Once::new();
static mut WHEEL_HWND: HWND = HWND(0);
static mut WHEEL_BUTTONS: Vec<WheelButton> = Vec::new();
static mut HOVERED_BUTTON: i32 = -1;
static mut SELECTED_PRESET_IDX: Option<usize> = None;

// Result communication
pub static WHEEL_RESULT: AtomicI32 = AtomicI32::new(-1); // -1 = pending, -2 = dismissed, >=0 = preset index
pub static WHEEL_ACTIVE: AtomicBool = AtomicBool::new(false);

const BUTTON_WIDTH: i32 = 140;
const BUTTON_HEIGHT: i32 = 32;
const BUTTON_MARGIN: i32 = 4;

struct WheelButton {
    rect: RECT,
    preset_idx: usize,
    label: String,
    is_dismiss: bool,
    color_idx: usize,  // For unique button colors
}

/// Calculate spiral positions using snake algorithm
/// Center first, then: right, down, left, left, up, up, right, right, right, ...
fn calculate_spiral_positions(count: usize, center: POINT) -> Vec<POINT> {
    if count == 0 { return vec![]; }
    
    let cell_w = BUTTON_WIDTH + BUTTON_MARGIN;
    let cell_h = BUTTON_HEIGHT + BUTTON_MARGIN;
    
    let mut positions = Vec::with_capacity(count);
    
    // First position is center (for dismiss button)
    positions.push(center);
    if count == 1 { return positions; }
    
    // Snake/spiral outward
    // Directions: 0=right, 1=down, 2=left, 3=up
    let dx = [1, 0, -1, 0];
    let dy = [0, 1, 0, -1];
    
    let mut x = 0i32;
    let mut y = 0i32;
    let mut direction = 0; // Start going right
    let mut steps_in_direction = 1;
    let mut steps_taken = 0;
    let mut direction_changes = 0;
    
    for _ in 1..count {
        // Move in current direction
        x += dx[direction];
        y += dy[direction];
        
        positions.push(POINT {
            x: center.x + x * cell_w,
            y: center.y + y * cell_h,
        });
        
        steps_taken += 1;
        
        // Check if we need to change direction
        if steps_taken >= steps_in_direction {
            steps_taken = 0;
            direction = (direction + 1) % 4;
            direction_changes += 1;
            
            // Increase step count every 2 direction changes
            if direction_changes % 2 == 0 {
                steps_in_direction += 1;
            }
        }
    }
    
    positions
}

/// Show preset wheel and return selected preset index (or None if dismissed)
/// This function blocks until user makes a selection
pub fn show_preset_wheel(
    filter_type: &str,      // "image", "text", or "audio"
    filter_mode: Option<&str>, // For text: "select" or "type"; For audio: "mic" or "device"
    center_pos: POINT,
) -> Option<usize> {
    unsafe {
        if WHEEL_HWND.0 != 0 { return None; } // Already showing
        
        // Reset state
        WHEEL_RESULT.store(-1, Ordering::SeqCst);
        WHEEL_ACTIVE.store(true, Ordering::SeqCst);
        SELECTED_PRESET_IDX = None;
        HOVERED_BUTTON = -1;
        WHEEL_BUTTONS.clear();
        
        // Get filtered presets
        let (presets, ui_lang) = {
            let app = APP.lock().unwrap();
            (app.config.presets.clone(), app.config.ui_language.clone())
        };
        
        // Filter presets based on type and mode
        let mut filtered: Vec<(usize, &Preset)> = presets.iter()
            .enumerate()
            .filter(|(_, p)| {
                // Exclude MASTER presets from the wheel
                if p.is_master { return false; }
                // Exclude upcoming presets
                if p.is_upcoming { return false; }
                
                // Filter by type
                if p.preset_type != filter_type { return false; }
                
                // Filter by mode if specified
                if let Some(mode) = filter_mode {
                    match filter_type {
                        "text" => {
                            if p.text_input_mode != mode { return false; }
                        },
                        "audio" => {
                            if p.audio_source != mode { return false; }
                        },
                        _ => {}
                    }
                }
                
                true
            })
            .collect();
        
        if filtered.is_empty() { 
            WHEEL_ACTIVE.store(false, Ordering::SeqCst);
            return None; 
        }
        
        // Calculate positions (first is dismiss, rest are presets)
        let positions = calculate_spiral_positions(filtered.len() + 1, center_pos);
        
        // Create dismiss button first (at center)
        let dismiss_label = match ui_lang.as_str() {
            "vi" => "Hủy",
            "ko" => "취소",
            _ => "Dismiss",
        };
        
        WHEEL_BUTTONS.push(WheelButton {
            rect: RECT {
                left: positions[0].x - BUTTON_WIDTH / 2,
                top: positions[0].y - BUTTON_HEIGHT / 2,
                right: positions[0].x + BUTTON_WIDTH / 2,
                bottom: positions[0].y + BUTTON_HEIGHT / 2,
            },
            preset_idx: usize::MAX,
            label: dismiss_label.to_string(),
            is_dismiss: true,
            color_idx: 0,
        });
        
        // Create preset buttons
        for (i, (preset_idx, preset)) in filtered.iter().enumerate() {
            let pos_idx = i + 1; // +1 because 0 is dismiss
            if pos_idx >= positions.len() { break; }
            
            let pos = positions[pos_idx];
            let label = get_localized_preset_name(&preset.id, &ui_lang);
            
            WHEEL_BUTTONS.push(WheelButton {
                rect: RECT {
                    left: pos.x - BUTTON_WIDTH / 2,
                    top: pos.y - BUTTON_HEIGHT / 2,
                    right: pos.x + BUTTON_WIDTH / 2,
                    bottom: pos.y + BUTTON_HEIGHT / 2,
                },
                preset_idx: *preset_idx,
                label,
                is_dismiss: false,
                color_idx: i,  // Unique color per preset
            });
        }
        
        // Calculate window bounds to encompass all buttons
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        
        for btn in &WHEEL_BUTTONS {
            min_x = min_x.min(btn.rect.left);
            min_y = min_y.min(btn.rect.top);
            max_x = max_x.max(btn.rect.right);
            max_y = max_y.max(btn.rect.bottom);
        }
        
        // Add padding
        let padding = 20;
        min_x -= padding;
        min_y -= padding;
        max_x += padding;
        max_y += padding;
        
        let win_width = max_x - min_x;
        let win_height = max_y - min_y;
        
        // Adjust button rects relative to window origin
        for btn in WHEEL_BUTTONS.iter_mut() {
            btn.rect.left -= min_x;
            btn.rect.right -= min_x;
            btn.rect.top -= min_y;
            btn.rect.bottom -= min_y;
        }
        
        // Create window
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_PresetWheel");
        
        REGISTER_WHEEL_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(wheel_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassW(&wc);
        });
        
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT Preset Wheel"),
            WS_POPUP,
            min_x, min_y, win_width, win_height,
            None, None, instance, None
        );
        
        WHEEL_HWND = hwnd;
        
        // Paint initial state
        paint_wheel_window(hwnd, win_width, win_height);
        
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        
        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            
            // Check if we got a result
            let result = WHEEL_RESULT.load(Ordering::SeqCst);
            if result != -1 {
                break;
            }
        }
        
        // Cleanup
        WHEEL_BUTTONS.clear();
        WHEEL_HWND = HWND(0);
        WHEEL_ACTIVE.store(false, Ordering::SeqCst);
        
        SELECTED_PRESET_IDX
    }
}

unsafe extern "system" fn wheel_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let pt = POINT { x, y };
            
            let mut new_hover = -1i32;
            for (i, btn) in WHEEL_BUTTONS.iter().enumerate() {
                if PtInRect(&btn.rect, pt).into() {
                    new_hover = i as i32;
                    break;
                }
            }
            
            if new_hover != HOVERED_BUTTON {
                HOVERED_BUTTON = new_hover;
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                paint_wheel_window(hwnd, rect.right, rect.bottom);
            }
            
            LRESULT(0)
        }
        
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let pt = POINT { x, y };
            
            for btn in WHEEL_BUTTONS.iter() {
                if PtInRect(&btn.rect, pt).into() {
                    if btn.is_dismiss {
                        SELECTED_PRESET_IDX = None;
                        WHEEL_RESULT.store(-2, Ordering::SeqCst); // Dismissed
                    } else {
                        SELECTED_PRESET_IDX = Some(btn.preset_idx);
                        WHEEL_RESULT.store(btn.preset_idx as i32, Ordering::SeqCst);
                    }
                    DestroyWindow(hwnd);
                    // NOTE: Do NOT call PostQuitMessage! This wheel may run nested
                    // inside another message loop (e.g., text_input). WM_QUIT would
                    // kill the outer loop too. The loop checks WHEEL_RESULT to break.
                    break;
                }
            }
            
            LRESULT(0)
        }
        
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B { // VK_ESCAPE
                SELECTED_PRESET_IDX = None;
                WHEEL_RESULT.store(-2, Ordering::SeqCst);
                DestroyWindow(hwnd);
                // No PostQuitMessage - see WM_LBUTTONDOWN comment
            }
            LRESULT(0)
        }
        
        WM_CLOSE => {
            if WHEEL_RESULT.load(Ordering::SeqCst) == -1 {
                WHEEL_RESULT.store(-2, Ordering::SeqCst);
            }
            DestroyWindow(hwnd);
            // No PostQuitMessage - see WM_LBUTTONDOWN comment
            LRESULT(0)
        }
        
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_wheel_window(hwnd: HWND, width: i32, height: i32) {
    let screen_dc = GetDC(None);
    let mem_dc = CreateCompatibleDC(screen_dc);
    
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
    let bitmap = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut p_bits, None, 0).unwrap();
    let old_bitmap = SelectObject(mem_dc, bitmap);
    
    // Clear to transparent
    let pixels = std::slice::from_raw_parts_mut(p_bits as *mut u32, (width * height) as usize);
    for p in pixels.iter_mut() {
        *p = 0x00000000; // Fully transparent
    }
    
    // Draw buttons
    for (i, btn) in WHEEL_BUTTONS.iter().enumerate() {
        let is_hovered = i as i32 == HOVERED_BUTTON;
        draw_button(mem_dc, pixels, width, &btn.rect, &btn.label, btn.is_dismiss, is_hovered, btn.color_idx);
    }
    
    // Update layered window
    let pt_src = POINT { x: 0, y: 0 };
    let size = SIZE { cx: width, cy: height };
    let mut bl = BLENDFUNCTION::default();
    bl.BlendOp = AC_SRC_OVER as u8;
    bl.SourceConstantAlpha = 255;
    bl.AlphaFormat = AC_SRC_ALPHA as u8;
    UpdateLayeredWindow(hwnd, HDC(0), None, Some(&size), mem_dc, Some(&pt_src), COLORREF(0), Some(&bl), ULW_ALPHA);
    
    SelectObject(mem_dc, old_bitmap);
    let _ = DeleteObject(bitmap);
    DeleteDC(mem_dc);
    ReleaseDC(None, screen_dc);
}

unsafe fn draw_button(dc: CreatedHDC, pixels: &mut [u32], stride: i32, rect: &RECT, label: &str, is_dismiss: bool, is_hovered: bool, color_idx: usize) {
    // Beautiful color palette for presets (unhovered state)
    // Each preset gets a unique, aesthetically pleasing dark color
    const PRESET_COLORS: [u32; 12] = [
        0xDD1E3A5F,  // Deep Blue
        0xDD2D4A22,  // Forest Green
        0xDD4A2C2C,  // Wine Red
        0xDD3D2B4A,  // Royal Purple
        0xDD4A3B22,  // Warm Brown
        0xDD1A4040,  // Teal
        0xDD3B2244,  // Plum
        0xDD2B3D4A,  // Steel Blue
        0xDD3D3D22,  // Olive
        0xDD4A2244,  // Magenta Dark
        0xDD224440,  // Dark Cyan
        0xDD44332B,  // Sienna
    ];
    
    // Hover colors - brighter, more saturated versions
    const HOVER_COLORS: [u32; 12] = [
        0xFF3366CC,  // Bright Blue
        0xFF4CAF50,  // Green
        0xFFE53935,  // Red
        0xFF7E57C2,  // Purple
        0xFFFF8F00,  // Amber
        0xFF00ACC1,  // Cyan
        0xFFAB47BC,  // Violet
        0xFF42A5F5,  // Light Blue
        0xFF9CCC65,  // Light Green
        0xFFEC407A,  // Pink
        0xFF26C6DA,  // Turquoise
        0xFFFF7043,  // Deep Orange
    ];
    
    let (bg_color, text_color) = if is_dismiss {
        if is_hovered {
            (0xFF442244u32, 0xFFFFFFFFu32) // Dark purple hover
        } else {
            (0xDD333333u32, 0xFFCCCCCCu32) // Dark gray
        }
    } else {
        let idx = color_idx % PRESET_COLORS.len();
        if is_hovered {
            (HOVER_COLORS[idx], 0xFFFFFFFFu32)
        } else {
            (PRESET_COLORS[idx], 0xFFE8E8E8u32)
        }
    };
    
    let corner_radius = 8.0f32;  // Slightly larger radius for smoother look
    let feather = 1.5f32;        // Anti-aliasing feather width
    
    let w = (rect.right - rect.left) as f32;
    let h = (rect.bottom - rect.top) as f32;
    
    // Draw rounded rectangle background with proper SDF anti-aliasing
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            if x < 0 || y < 0 || x >= stride || y >= (pixels.len() as i32 / stride) { continue; }
            
            let idx = (y * stride + x) as usize;
            if idx >= pixels.len() { continue; }
            
            // Calculate local coords relative to rect
            let lx = (x - rect.left) as f32;
            let ly = (y - rect.top) as f32;
            
            // Signed distance to rounded rectangle
            // For each corner, calculate distance to the corner circle
            let dist = rounded_rect_sdf(lx, ly, w, h, corner_radius);
            
            // Anti-aliasing: smooth transition at edges
            let alpha_mult = if dist < -feather {
                1.0  // Fully inside
            } else if dist > feather {
                0.0  // Fully outside
            } else {
                // Smooth hermite interpolation for AA
                let t = (dist + feather) / (2.0 * feather);
                1.0 - t * t * (3.0 - 2.0 * t)  // smoothstep
            };
            
            if alpha_mult <= 0.0 { continue; }
            
            let base_alpha = ((bg_color >> 24) & 0xFF) as f32 / 255.0;
            let final_alpha = (base_alpha * alpha_mult * 255.0) as u32;
            
            // Premultiplied alpha
            let r = (((bg_color >> 16) & 0xFF) * final_alpha / 255) as u32;
            let g = (((bg_color >> 8) & 0xFF) * final_alpha / 255) as u32;
            let b = ((bg_color & 0xFF) * final_alpha / 255) as u32;
            
            pixels[idx] = (final_alpha << 24) | (r << 16) | (g << 8) | b;
        }
    }
    
    // Draw text
    let font = CreateFontW(
        14, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0,
        DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32,
        (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI")
    );
    let old_font = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(text_color & 0x00FFFFFF));
    
    let mut text_rect = *rect;
    let mut text_w = to_wstring(label);
    DrawTextW(dc, &mut text_w, &mut text_rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    
    // Fix text alpha by finding bright pixels and making them opaque
    GdiFlush();
    for y in rect.top.max(0)..rect.bottom.min(pixels.len() as i32 / stride) {
        for x in rect.left.max(0)..rect.right.min(stride) {
            let idx = (y * stride + x) as usize;
            if idx >= pixels.len() { continue; }
            
            let val = pixels[idx];
            let a = (val >> 24) & 0xFF;
            let r = (val >> 16) & 0xFF;
            let g = (val >> 8) & 0xFF;
            let b = val & 0xFF;
            
            // If any color channel is brighter than alpha, it's text
            let max_c = r.max(g).max(b);
            if max_c > a {
                pixels[idx] = (max_c << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }
    
    SelectObject(dc, old_font);
    let _ = DeleteObject(font);
}

/// Signed Distance Field for rounded rectangle
/// Returns negative distance if inside, positive if outside
fn rounded_rect_sdf(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    // Transform to first quadrant (symmetry)
    let px = (x - w / 2.0).abs();
    let py = (y - h / 2.0).abs();
    
    // Half-size minus radius
    let hx = w / 2.0 - r;
    let hy = h / 2.0 - r;
    
    // Distance to corner region
    let dx = (px - hx).max(0.0);
    let dy = (py - hy).max(0.0);
    
    // Outside corner: euclidean distance to corner circle minus radius
    // Inside: max of distances to edges
    let outside_dist = (dx * dx + dy * dy).sqrt() - r;
    let inside_dist = (px - hx).max(py - hy).min(0.0);
    
    outside_dist.max(inside_dist)
}

/// Check if preset wheel is currently showing
pub fn is_wheel_active() -> bool {
    WHEEL_ACTIVE.load(Ordering::SeqCst)
}

/// Dismiss the wheel if it's showing
pub fn dismiss_wheel() {
    unsafe {
        if WHEEL_HWND.0 != 0 {
            PostMessageW(WHEEL_HWND, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}
