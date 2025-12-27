// Favorite Bubble Overlay - WebView2-based floating panel for quick access to favorite presets
// Uses a hybrid approach: transparent layered window for collapsed state, WebView2 panel when expanded

use crate::gui::settings_ui::get_localized_preset_name;
use crate::APP;
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Once,
};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebView, WebViewBuilder};

static REGISTER_BUBBLE_CLASS: Once = Once::new();
static REGISTER_PANEL_CLASS: Once = Once::new();
static BUBBLE_ACTIVE: AtomicBool = AtomicBool::new(false);
static BUBBLE_HWND: AtomicIsize = AtomicIsize::new(0);
static PANEL_HWND: AtomicIsize = AtomicIsize::new(0);
static IS_EXPANDED: AtomicBool = AtomicBool::new(false);
static IS_HOVERED: AtomicBool = AtomicBool::new(false);
static IS_DRAGGING: AtomicBool = AtomicBool::new(false);
static IS_DRAGGING_MOVED: AtomicBool = AtomicBool::new(false);
static DRAG_START_X: AtomicIsize = AtomicIsize::new(0);
static DRAG_START_Y: AtomicIsize = AtomicIsize::new(0);
const DRAG_THRESHOLD: i32 = 5; // Pixels of movement before counting as a drag

thread_local! {
    static PANEL_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
}

const BUBBLE_SIZE: i32 = 52;
const PANEL_WIDTH: i32 = 220;
const PANEL_MAX_HEIGHT: i32 = 350;
const OPACITY_INACTIVE: u8 = 80; // ~31% opacity when not hovered
const OPACITY_ACTIVE: u8 = 255; // 100% opacity when hovered/expanded

// App icon embedded at compile time
const ICON_PNG_BYTES: &[u8] = include_bytes!("../../assets/app-icon-small.png");

// Cached decoded RGBA pixels
lazy_static::lazy_static! {
    static ref ICON_RGBA: Vec<u8> = {
        if let Ok(img) = image::load_from_memory(ICON_PNG_BYTES) {
            let resized = img.resize_exact(
                BUBBLE_SIZE as u32,
                BUBBLE_SIZE as u32,
                image::imageops::FilterType::Lanczos3
            );
            resized.to_rgba8().into_raw()
        } else {
            vec![]
        }
    };
}

// HWND wrapper for wry
struct HwndWrapper(HWND);
unsafe impl Send for HwndWrapper {}
unsafe impl Sync for HwndWrapper {}
impl raw_window_handle::HasWindowHandle for HwndWrapper {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::Win32WindowHandle::new(
            std::num::NonZeroIsize::new(self.0 .0 as isize).expect("HWND cannot be null"),
        );
        let handle = raw_window_handle::RawWindowHandle::Win32(raw);
        unsafe { Ok(raw_window_handle::WindowHandle::borrow_raw(handle)) }
    }
}

/// Show the favorite bubble overlay
pub fn show_favorite_bubble() {
    // Prevent duplicates
    if BUBBLE_ACTIVE.swap(true, Ordering::SeqCst) {
        return; // Already active
    }

    std::thread::spawn(|| {
        create_bubble_window();
    });
}

/// Hide the favorite bubble overlay
pub fn hide_favorite_bubble() {
    if !BUBBLE_ACTIVE.load(Ordering::SeqCst) {
        return;
    }

    let hwnd_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn get_favorite_presets_html() -> String {
    let mut html_items = String::new();
    let mut image_items = String::new();
    let mut text_items = String::new();
    let mut audio_items = String::new();

    if let Ok(app) = APP.lock() {
        let lang = &app.config.ui_language;
        for (idx, preset) in app.config.presets.iter().enumerate() {
            if preset.is_favorite && !preset.is_upcoming && !preset.is_master {
                let name = if preset.id.starts_with("preset_") {
                    get_localized_preset_name(&preset.id, lang)
                } else {
                    preset.name.clone()
                };

                let icon = match preset.preset_type.as_str() {
                    "text" => "📝",
                    "audio" => "🎤",
                    _ => "📷",
                };

                let item = format!(
                    r#"<div class="preset-item" onclick="trigger({})">{} {}</div>"#,
                    idx,
                    icon,
                    html_escape(&name)
                );

                match preset.preset_type.as_str() {
                    "text" => text_items.push_str(&item),
                    "audio" => audio_items.push_str(&item),
                    _ => image_items.push_str(&item),
                }
            }
        }
    }

    // Build grouped HTML
    if !image_items.is_empty() {
        html_items.push_str(r#"<div class="group"><div class="group-header">📷 Image</div>"#);
        html_items.push_str(&image_items);
        html_items.push_str("</div>");
    }
    if !text_items.is_empty() {
        html_items.push_str(r#"<div class="group"><div class="group-header">📝 Text</div>"#);
        html_items.push_str(&text_items);
        html_items.push_str("</div>");
    }
    if !audio_items.is_empty() {
        html_items.push_str(r#"<div class="group"><div class="group-header">🎤 Audio</div>"#);
        html_items.push_str(&audio_items);
        html_items.push_str("</div>");
    }

    if html_items.is_empty() {
        html_items = r#"<div class="empty">No favorites yet<br><small>Click ⭐ on presets to add them</small></div>"#.to_string();
    }

    html_items
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn generate_panel_html() -> String {
    let favorites_html = get_favorite_presets_html();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #1e1e28;
    font-family: 'Segoe UI', system-ui, sans-serif;
    user-select: none;
}}

.container {{
    display: flex;
    flex-direction: column;
    height: 100%;
}}

.header {{
    padding: 10px 14px;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: grab;
}}

.header:active {{ cursor: grabbing; }}

.title {{
    color: white;
    font-size: 13px;
    font-weight: 600;
}}

.close-btn {{
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: rgba(255,255,255,0.2);
    border: none;
    color: white;
    font-size: 14px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
}}

.close-btn:hover {{ background: rgba(255,255,255,0.3); }}

.list {{
    flex: 1;
    overflow-y: auto;
    padding: 6px;
}}

.list::-webkit-scrollbar {{ width: 5px; }}
.list::-webkit-scrollbar-thumb {{ background: rgba(255,255,255,0.2); border-radius: 3px; }}

.group {{ margin-bottom: 6px; }}

.group-header {{
    color: rgba(255,255,255,0.4);
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    padding: 4px 8px;
}}

.preset-item {{
    padding: 9px 12px;
    border-radius: 8px;
    cursor: pointer;
    color: white;
    font-size: 12px;
    margin-bottom: 3px;
    background: rgba(255,255,255,0.05);
    transition: all 0.15s ease;
}}

.preset-item:hover {{
    background: rgba(102, 126, 234, 0.4);
    padding-left: 16px;
}}

.empty {{
    color: rgba(255,255,255,0.4);
    text-align: center;
    padding: 30px 15px;
    font-size: 12px;
    line-height: 1.6;
}}
</style>
</head>
<body>
<div class="container">
    <div class="list">{favorites}</div>
</div>
<script>
function startDrag(e) {{
    if (e.button === 0) window.ipc.postMessage('drag');
}}
function closePanel() {{
    window.ipc.postMessage('close');
}}
function trigger(idx) {{
    window.ipc.postMessage('trigger:' + idx);
}}
</script>
</body>
</html>"#,
        favorites = favorites_html
    )
}

fn create_bubble_window() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTFavoriteBubble");

        REGISTER_BUBBLE_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(bubble_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        // Get saved position or use default
        let (initial_x, initial_y) = if let Ok(app) = APP.lock() {
            app.config.favorite_bubble_position.unwrap_or_else(|| {
                let screen_w = GetSystemMetrics(SM_CXSCREEN);
                let screen_h = GetSystemMetrics(SM_CYSCREEN);
                (screen_w - BUBBLE_SIZE - 30, screen_h - BUBBLE_SIZE - 150)
            })
        } else {
            (100, 100)
        };

        // Create layered window for transparency
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("FavBubble"),
            WS_POPUP,
            initial_x,
            initial_y,
            BUBBLE_SIZE,
            BUBBLE_SIZE,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if hwnd.is_invalid() {
            BUBBLE_ACTIVE.store(false, Ordering::SeqCst);
            return;
        }

        BUBBLE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // Paint the bubble
        update_bubble_visual(hwnd);

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        // Warmup: Create panel hidden immediately
        ensure_panel_created(hwnd);

        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup
        destroy_panel();
        BUBBLE_ACTIVE.store(false, Ordering::SeqCst);
        BUBBLE_HWND.store(0, Ordering::SeqCst);
    }
}

// Warmup the panel by creating it hidden
fn ensure_panel_created(bubble_hwnd: HWND) {
    if PANEL_HWND.load(Ordering::SeqCst) != 0 {
        return;
    }

    // Reuse logic from show_panel but keep hidden
    // We can just call show_panel logic but modify it to accept a "show" flag?
    // Or better: Create a dedicated creation function.
    create_panel_window_internal(bubble_hwnd);
}

fn escape_js(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "")
}

fn update_panel_content(html: &str) {
    PANEL_WEBVIEW.with(|wv| {
        if let Some(webview) = wv.borrow().as_ref() {
            let escaped = escape_js(html);
            let script = format!(
                "document.querySelector('.list').innerHTML = \"{}\";",
                escaped
            );
            let _ = webview.evaluate_script(&script);
        }
    });
}

fn update_bubble_visual(hwnd: HWND) {
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

fn draw_bubble_pixels(pixels: &mut [u32], size: i32, is_active: bool) {
    let opacity = if is_active {
        OPACITY_ACTIVE
    } else {
        OPACITY_INACTIVE
    };

    // Use embedded icon if available
    if !ICON_RGBA.is_empty() {
        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) as usize;
                let src_idx = idx * 4; // RGBA

                if src_idx + 3 < ICON_RGBA.len() {
                    let r = ICON_RGBA[src_idx] as u32;
                    let g = ICON_RGBA[src_idx + 1] as u32;
                    let b = ICON_RGBA[src_idx + 2] as u32;
                    let a = ICON_RGBA[src_idx + 3] as u32;

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

fn get_bubble_color(t: f32, is_active: bool) -> (u8, u8, u8) {
    // Vibrant purple gradient
    if is_active {
        // Active: Bright vibrant purple
        let r = (120.0 + 40.0 * t) as u8;
        let g = (80.0 + 30.0 * t) as u8;
        let b = (220.0 + 35.0 * (1.0 - t)) as u8;
        (r, g, b)
    } else {
        // Inactive: Slightly darker but still visible purple
        let r = (100.0 + 30.0 * t) as u8;
        let g = (70.0 + 20.0 * t) as u8;
        let b = (180.0 + 30.0 * (1.0 - t)) as u8;
        (r, g, b)
    }
}

fn draw_border_ring(pixels: &mut [u32], size: i32, center: f32, radius: f32, is_active: bool) {
    let border_inner = radius - 2.5;
    let border_outer = radius;

    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) as usize;
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            let dx = fx - center;
            let dy = fy - center;
            let dist = (dx * dx + dy * dy).sqrt();

            // Check if in border region
            if dist > border_inner && dist < border_outer + 1.0 {
                // Border color - bright glow
                let glow_t = 1.0
                    - ((dist - border_inner) / (border_outer - border_inner + 1.0)).clamp(0.0, 1.0);
                let alpha = (glow_t * 200.0) as u32;

                if alpha > 0 {
                    let (br, bg, bb) = if is_active {
                        (200u8, 180u8, 255u8) // Bright glow
                    } else {
                        (160u8, 140u8, 220u8) // Subtle glow
                    };

                    // Blend with existing pixel
                    let existing = pixels[idx];
                    let ea = (existing >> 24) & 0xFF;
                    let er = (existing >> 16) & 0xFF;
                    let eg = (existing >> 8) & 0xFF;
                    let eb = existing & 0xFF;

                    let blend = alpha as f32 / 255.0;
                    let inv = 1.0 - blend;

                    let nr = (br as f32 * blend + er as f32 * inv) as u32;
                    let ng = (bg as f32 * blend + eg as f32 * inv) as u32;
                    let nb = (bb as f32 * blend + eb as f32 * inv) as u32;
                    let na = ea.max(alpha);

                    pixels[idx] = (na << 24) | (nr << 16) | (ng << 8) | nb;
                }
            }
        }
    }
}

fn draw_star_pixels(pixels: &mut [u32], size: i32, cx: i32, cy: i32, star_size: i32) {
    use std::f32::consts::PI;

    let outer_r = star_size as f32;
    let inner_r = star_size as f32 * 0.4;

    // Star polygon vertices
    let mut verts: [(f32, f32); 10] = [(0.0, 0.0); 10];
    for i in 0..10 {
        let angle = (i as f32 * PI / 5.0) - PI / 2.0;
        let r = if i % 2 == 0 { outer_r } else { inner_r };
        verts[i] = (cx as f32 + r * angle.cos(), cy as f32 + r * angle.sin());
    }

    // Draw star using scanline fill with anti-aliasing
    let y_min = (cy - star_size - 2).max(0);
    let y_max = (cy + star_size + 2).min(size);

    for y in y_min..y_max {
        for x in (cx - star_size - 2).max(0)..(cx + star_size + 2).min(size) {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            // Point-in-polygon test with anti-aliasing
            let dist = point_to_star_distance(fx, fy, &verts);

            if dist < 0.0 {
                // Inside star
                let idx = (y * size + x) as usize;
                let alpha = (-dist).min(1.5) / 1.5;

                // Gold color with slight gradient
                let t = ((fy - (cy - star_size) as f32) / (star_size as f32 * 2.0)).clamp(0.0, 1.0);
                let r = (255.0 - 20.0 * t) as u8;
                let g = (215.0 - 30.0 * t) as u8;
                let b = (0.0 + 50.0 * t) as u8;

                let a = (alpha * 255.0) as u32;
                let r_pm = (r as u32 * a / 255);
                let g_pm = (g as u32 * a / 255);
                let b_pm = (b as u32 * a / 255);

                // Blend with existing
                let existing = pixels[idx];
                let ea = (existing >> 24) & 0xFF;

                if a >= ea {
                    pixels[idx] = (a << 24) | (r_pm << 16) | (g_pm << 8) | b_pm;
                }
            } else if dist < 1.5 {
                // Edge of star - anti-alias
                let idx = (y * size + x) as usize;
                let alpha = (1.0 - dist / 1.5).clamp(0.0, 1.0);

                let r = 255u8;
                let g = 200u8;
                let b = 50u8;

                let a = (alpha * 255.0) as u32;

                // Blend with existing
                let existing = pixels[idx];
                let ea = (existing >> 24) & 0xFF;
                let er = (existing >> 16) & 0xFF;
                let eg = (existing >> 8) & 0xFF;
                let eb = existing & 0xFF;

                let blend = alpha;
                let inv = 1.0 - blend;

                let nr = (r as f32 * blend + er as f32 * inv) as u32;
                let ng = (g as f32 * blend + eg as f32 * inv) as u32;
                let nb = (b as f32 * blend + eb as f32 * inv) as u32;
                let na = ea.max(a);

                pixels[idx] = (na << 24) | (nr << 16) | (ng << 8) | nb;
            }
        }
    }
}

fn point_to_star_distance(px: f32, py: f32, verts: &[(f32, f32); 10]) -> f32 {
    // Simplified - check if inside using winding number and estimate distance
    let mut winding = 0i32;
    let mut min_dist = f32::MAX;

    for i in 0..10 {
        let (x1, y1) = verts[i];
        let (x2, y2) = verts[(i + 1) % 10];

        // Winding number contribution
        if y1 <= py {
            if y2 > py {
                if ((x2 - x1) * (py - y1) - (px - x1) * (y2 - y1)) > 0.0 {
                    winding += 1;
                }
            }
        } else {
            if y2 <= py {
                if ((x2 - x1) * (py - y1) - (px - x1) * (y2 - y1)) < 0.0 {
                    winding -= 1;
                }
            }
        }

        // Distance to edge
        let edge_dist = point_to_line_dist(px, py, x1, y1, x2, y2);
        min_dist = min_dist.min(edge_dist);
    }

    if winding != 0 {
        -min_dist // Inside
    } else {
        min_dist // Outside
    }
}

fn point_to_line_dist(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 0.0001 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let closest_x = x1 + t * dx;
    let closest_y = y1 + t * dy;

    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

unsafe extern "system" fn bubble_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    const WM_MOUSELEAVE: u32 = 0x02A3;

    match msg {
        WM_LBUTTONDOWN => {
            IS_DRAGGING.store(true, Ordering::SeqCst);
            IS_DRAGGING_MOVED.store(false, Ordering::SeqCst);

            // Store initial click position for threshold check
            let x = (lparam.0 as i32) & 0xFFFF;
            let y = ((lparam.0 as i32) >> 16) & 0xFFFF;
            DRAG_START_X.store(x as isize, Ordering::SeqCst);
            DRAG_START_Y.store(y as isize, Ordering::SeqCst);

            let _ = SetCapture(hwnd);
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            let was_dragging_moved = IS_DRAGGING_MOVED.load(Ordering::SeqCst);
            IS_DRAGGING.store(false, Ordering::SeqCst);
            let _ = ReleaseCapture();

            // Only toggle if we didn't drag/move the bubble
            if !was_dragging_moved {
                if IS_EXPANDED.load(Ordering::SeqCst) {
                    close_panel();
                } else {
                    show_panel(hwnd);
                }
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            if IS_DRAGGING.load(Ordering::SeqCst) && (wparam.0 & 0x0001) != 0 {
                // Left button held - check for drag
                let x = (lparam.0 as i32) & 0xFFFF;
                let y = ((lparam.0 as i32) >> 16) & 0xFFFF;

                // Convert to signed 16-bit to handle negative coordinates properly
                let x = x as i16 as i32;
                let y = y as i16 as i32;

                // Check if we've exceeded the drag threshold
                if !IS_DRAGGING_MOVED.load(Ordering::SeqCst) {
                    let start_x = DRAG_START_X.load(Ordering::SeqCst) as i32;
                    let start_y = DRAG_START_Y.load(Ordering::SeqCst) as i32;
                    let dx = (x - start_x).abs();
                    let dy = (y - start_y).abs();

                    if dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD {
                        IS_DRAGGING_MOVED.store(true, Ordering::SeqCst);
                    }
                }

                // Only actually move the window if threshold was exceeded
                if IS_DRAGGING_MOVED.load(Ordering::SeqCst) {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);

                    let new_x = rect.left + x - BUBBLE_SIZE / 2;
                    let new_y = rect.top + y - BUBBLE_SIZE / 2;

                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        new_x,
                        new_y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );

                    // Move panel if open
                    if IS_EXPANDED.load(Ordering::SeqCst) {
                        move_panel_to_bubble(new_x, new_y);
                    }
                }
            }

            if !IS_HOVERED.load(Ordering::SeqCst) {
                IS_HOVERED.store(true, Ordering::SeqCst);
                update_bubble_visual(hwnd);

                // Track mouse leave
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            IS_HOVERED.store(false, Ordering::SeqCst);
            if !IS_EXPANDED.load(Ordering::SeqCst) {
                update_bubble_visual(hwnd);
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            close_panel();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn show_panel(bubble_hwnd: HWND) {
    if IS_EXPANDED.load(Ordering::SeqCst) {
        return;
    }

    // Ensure window exists
    ensure_panel_created(bubble_hwnd);

    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val == 0 {
        return;
    }

    unsafe {
        let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);

        // Update content (favorites might have changed)
        let favorites_html = get_favorite_presets_html();
        update_panel_content(&favorites_html);

        // Recalculate size/pos
        let mut bubble_rect = RECT::default();
        let _ = GetWindowRect(bubble_hwnd, &mut bubble_rect);

        let fav_count = APP
            .lock()
            .map(|app| {
                app.config
                    .presets
                    .iter()
                    .filter(|p| p.is_favorite && !p.is_upcoming && !p.is_master)
                    .count()
            })
            .unwrap_or(0);

        let panel_height = (70 + fav_count as i32 * 36).min(PANEL_MAX_HEIGHT).max(100);
        let screen_w = GetSystemMetrics(SM_CXSCREEN);

        let (panel_x, panel_y) = if bubble_rect.left > screen_w / 2 {
            (
                bubble_rect.left - PANEL_WIDTH - 8,
                bubble_rect.top - panel_height / 2 + BUBBLE_SIZE / 2,
            )
        } else {
            (
                bubble_rect.right + 8,
                bubble_rect.top - panel_height / 2 + BUBBLE_SIZE / 2,
            )
        };

        // Resize WebView as well
        PANEL_WEBVIEW.with(|wv| {
            if let Some(webview) = wv.borrow().as_ref() {
                let _ = webview.set_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        PANEL_WIDTH as u32,
                        panel_height as u32,
                    )),
                });
            }
        });

        // Show and Move
        let _ = SetWindowPos(
            panel_hwnd,
            None,
            panel_x,
            panel_y.max(10),
            PANEL_WIDTH,
            panel_height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );

        IS_EXPANDED.store(true, Ordering::SeqCst);
        update_bubble_visual(bubble_hwnd);
    }
}

fn create_panel_window_internal(bubble_hwnd: HWND) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTFavoritePanel");

        REGISTER_PANEL_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(panel_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        // Initial creation (using default size, will be resized on show)
        // Hidden by default (no WS_VISIBLE)
        let panel_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("FavPanel"),
            WS_POPUP, // Removed WS_VISIBLE
            0,
            0,
            PANEL_WIDTH,
            100, // Dummy height
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if !panel_hwnd.is_invalid() {
            // Rounded corners
            let corner_pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                panel_hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&corner_pref) as u32,
            );

            PANEL_HWND.store(panel_hwnd.0 as isize, Ordering::SeqCst);
            create_panel_webview(panel_hwnd);
        }
    }
}

fn move_panel_to_bubble(bubble_x: i32, bubble_y: i32) {
    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val == 0 {
        return;
    }

    unsafe {
        let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
        let mut panel_rect = RECT::default();
        let _ = GetWindowRect(panel_hwnd, &mut panel_rect);
        let panel_h = panel_rect.bottom - panel_rect.top;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let (panel_x, panel_y) = if bubble_x > screen_w / 2 {
            (
                bubble_x - PANEL_WIDTH - 8,
                bubble_y - panel_h / 2 + BUBBLE_SIZE / 2,
            )
        } else {
            (
                bubble_x + BUBBLE_SIZE + 8,
                bubble_y - panel_h / 2 + BUBBLE_SIZE / 2,
            )
        };

        let _ = SetWindowPos(
            panel_hwnd,
            None,
            panel_x,
            panel_y.max(10),
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

// Hides the panel but keeps it alive (warm)
fn close_panel() {
    if !IS_EXPANDED.swap(false, Ordering::SeqCst) {
        return;
    }

    let panel_val = PANEL_HWND.load(Ordering::SeqCst);
    if panel_val != 0 {
        unsafe {
            let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
            let _ = ShowWindow(panel_hwnd, SW_HIDE);
        }
    }

    // Update bubble visual
    let bubble_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if bubble_val != 0 {
        let bubble_hwnd = HWND(bubble_val as *mut std::ffi::c_void);
        update_bubble_visual(bubble_hwnd);
    }

    // Save position
    save_bubble_position();
}

// Actually destroys the panel (cleanup)
fn destroy_panel() {
    let panel_val = PANEL_HWND.swap(0, Ordering::SeqCst);
    if panel_val != 0 {
        PANEL_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = None;
        });

        unsafe {
            let panel_hwnd = HWND(panel_val as *mut std::ffi::c_void);
            let _ = DestroyWindow(panel_hwnd);
        }
    }
}

fn save_bubble_position() {
    let bubble_val = BUBBLE_HWND.load(Ordering::SeqCst);
    if bubble_val == 0 {
        return;
    }

    unsafe {
        let bubble_hwnd = HWND(bubble_val as *mut std::ffi::c_void);
        let mut rect = RECT::default();
        let _ = GetWindowRect(bubble_hwnd, &mut rect);

        if let Ok(mut app) = APP.lock() {
            app.config.favorite_bubble_position = Some((rect.left, rect.top));
            crate::config::save_config(&app.config);
        }
    }
}

fn create_panel_webview(panel_hwnd: HWND) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(panel_hwnd, &mut rect);
    }

    let html = generate_panel_html();
    let wrapper = HwndWrapper(panel_hwnd);

    let result = WebViewBuilder::new()
        .with_bounds(Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            )),
        })
        .with_html(&html)
        .with_transparent(false)
        .with_ipc_handler(move |msg: wry::http::Request<String>| {
            let body = msg.body();

            if body == "drag" {
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
                    let _ = ReleaseCapture();
                    SendMessageW(
                        panel_hwnd,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION as usize)),
                        Some(LPARAM(0)),
                    );
                }
            } else if body == "close" {
                close_panel();
            } else if body.starts_with("trigger:") {
                if let Ok(idx) = body[8..].parse::<usize>() {
                    close_panel();
                    trigger_preset(idx);
                }
            }
        })
        .build_as_child(&wrapper);

    if let Ok(webview) = result {
        PANEL_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = Some(webview);
        });
    }
}

unsafe extern "system" fn panel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLOSE => {
            close_panel();
            LRESULT(0)
        }

        WM_KILLFOCUS => {
            // Don't close immediately - check if focus went to bubble
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn trigger_preset(preset_idx: usize) {
    unsafe {
        let class = w!("HotkeyListenerClass");
        let title = w!("Listener");
        let hwnd = FindWindowW(class, title).unwrap_or_default();

        if !hwnd.is_invalid() {
            let hotkey_id = (preset_idx as i32 * 1000) + 1;
            let _ = PostMessageW(Some(hwnd), WM_HOTKEY, WPARAM(hotkey_id as usize), LPARAM(0));
        }
    }
}

use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
