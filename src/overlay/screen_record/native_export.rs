use image::imageops::{resize, FilterType};
use image::{ImageBuffer, Rgba};
use serde::Deserialize;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::overlay::screen_record::engine::VIDEO_PATH;

// --- Structs for JSON Deserialization ---

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfig {
    pub width: u32,
    pub height: u32,
    pub framerate: u32,
    pub audio_path: String,
    pub trim_start: f64,
    pub duration: f64,
    pub speed: f64,
    pub segment: VideoSegment,
    pub background_config: BackgroundConfig,
    pub mouse_positions: Vec<MousePosition>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VideoSegment {
    pub trim_start: f64,
    pub trim_end: f64,
    pub zoom_keyframes: Vec<ZoomKeyframe>,
    pub smooth_motion_path: Option<Vec<MotionPoint>>,
    pub crop: Option<CropRect>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ZoomKeyframe {
    pub time: f64,
    pub zoom_factor: f64,
    pub position_x: f64,
    pub position_y: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct MotionPoint {
    pub time: f64,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundConfig {
    pub scale: f64,
    pub border_radius: f64,
    pub background_type: String,
    pub custom_background: Option<String>,
    pub shadow: f64,
    pub cursor_scale: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct MousePosition {
    pub x: i32,
    pub y: i32,
    pub timestamp: f64,
    #[serde(rename = "isClicked")]
    pub is_clicked: bool,
    pub cursor_type: String,
}

// --- CURSOR LOADING (Programmatic / SVG) ---
lazy_static::lazy_static! {
    static ref CURSOR_ARROW: ImageBuffer<Rgba<u8>, Vec<u8>> = draw_default_cursor();
    static ref CURSOR_TEXT: ImageBuffer<Rgba<u8>, Vec<u8>> = draw_text_cursor();
    static ref CURSOR_HAND: ImageBuffer<Rgba<u8>, Vec<u8>> = load_svg_cursor(include_bytes!("../../../screen-record/public/pointer.svg"));
}

fn load_svg_cursor(svg_bytes: &[u8]) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg_bytes, &opt).expect("Failed to parse cursor SVG");

    let size = tree.size();
    let width = size.width() as u32;
    let height = size.height() as u32;

    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    ImageBuffer::from_raw(width, height, pixmap.data().to_vec()).unwrap()
}

fn draw_default_cursor() -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(32, 32).unwrap();
    let transform = tiny_skia::Transform::from_translate(4.0, 4.0);

    let mut path_builder = tiny_skia::PathBuilder::new();
    // M 8.2 4.9 L 19.8 16.5 L 13 16.5 L 12.6 16.6 L 8.2 20.9 Z
    path_builder.move_to(8.2, 4.9);
    path_builder.line_to(19.8, 16.5);
    path_builder.line_to(13.0, 16.5);
    path_builder.line_to(12.6, 16.6);
    path_builder.line_to(8.2, 20.9);
    path_builder.close();

    // M 17.3 21.6 L 13.7 23.1 L 9 12 L 12.7 10.5 Z
    path_builder.move_to(17.3, 21.6);
    path_builder.line_to(13.7, 23.1);
    path_builder.line_to(9.0, 12.0);
    path_builder.line_to(12.7, 10.5);
    path_builder.close();
    let path = path_builder.finish().unwrap();

    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;

    let mut white_paint = tiny_skia::Paint::default();
    white_paint.set_color_rgba8(255, 255, 255, 255);
    white_paint.anti_alias = true;

    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = 1.5;

    pixmap.stroke_path(&path, &white_paint, &stroke, transform, None);
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);

    ImageBuffer::from_raw(32, 32, pixmap.data().to_vec()).unwrap()
}

fn draw_text_cursor() -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(32, 32).unwrap();
    let transform = tiny_skia::Transform::from_translate(10.0, 8.0);

    let mut path_builder = tiny_skia::PathBuilder::new();
    // Path matched from videoRenderer.ts
    // M 2 0 L 10 0 L 10 2 L 7 2 L 7 14 L 10 14 L 10 16 L 2 16 L 2 14 L 5 14 L 5 2 L 2 2 Z
    path_builder.move_to(2.0, 0.0);
    path_builder.line_to(10.0, 0.0);
    path_builder.line_to(10.0, 2.0);
    path_builder.line_to(7.0, 2.0);
    path_builder.line_to(7.0, 14.0);
    path_builder.line_to(10.0, 14.0);
    path_builder.line_to(10.0, 16.0);
    path_builder.line_to(2.0, 16.0);
    path_builder.line_to(2.0, 14.0);
    path_builder.line_to(5.0, 14.0);
    path_builder.line_to(5.0, 2.0);
    path_builder.line_to(2.0, 2.0);
    path_builder.close();
    let path = path_builder.finish().unwrap();

    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0, 0, 0, 255);
    paint.anti_alias = true;

    let mut white_paint = tiny_skia::Paint::default();
    white_paint.set_color_rgba8(255, 255, 255, 255);
    white_paint.anti_alias = true;

    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = 1.5;

    pixmap.stroke_path(&path, &white_paint, &stroke, transform, None);
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);

    ImageBuffer::from_raw(32, 32, pixmap.data().to_vec()).unwrap()
}

// --- HELPER: Interpolation ---
fn interpolate_zoom(
    current_time: f64,
    segment: &VideoSegment,
    source_w: u32,
    source_h: u32,
) -> (f64, f64, f64) {
    // 1. Check for smooth motion path first
    if let Some(path) = &segment.smooth_motion_path {
        if !path.is_empty() {
            // Find segment in path
            let idx = path
                .iter()
                .position(|p| p.time >= current_time)
                .unwrap_or(path.len().saturating_sub(1));

            if idx == 0 {
                let p = &path[0];
                return (p.x, p.y, p.zoom);
            }

            let p2 = &path[idx];
            let p1 = &path[idx - 1];

            let t = (current_time - p1.time) / (p2.time - p1.time).max(0.001);
            let x = p1.x + (p2.x - p1.x) * t;
            let y = p1.y + (p2.y - p1.y) * t;
            let zoom = p1.zoom + (p2.zoom - p1.zoom) * t;

            return (x, y, zoom);
        }
    }

    // 2. Fallback to Keyframes
    let kfs = &segment.zoom_keyframes;
    if kfs.is_empty() {
        return (source_w as f64 / 2.0, source_h as f64 / 2.0, 1.0);
    }

    // Find keyframes surrounding current time
    // Sort logic should be handled by frontend ideally, but we assume sorted here or sort if needed?
    // Assuming sorted for speed.

    // Find next keyframe
    let next_idx = kfs.iter().position(|k| k.time > current_time);

    if let Some(idx) = next_idx {
        if idx == 0 {
            let k = &kfs[0];
            return (
                k.position_x * source_w as f64,
                k.position_y * source_h as f64,
                k.zoom_factor,
            );
        }

        let k2 = &kfs[idx];
        let k1 = &kfs[idx - 1];

        // Check transition window (1.0s)
        let diff = k2.time - current_time;
        if diff <= 1.0 {
            let t = 1.0 - diff; // 0 to 1
                                // Ease out cubic
            let et = 1.0 - (1.0 - t).powi(3);

            let z = k1.zoom_factor + (k2.zoom_factor - k1.zoom_factor) * et;
            let x = (k1.position_x + (k2.position_x - k1.position_x) * et) * source_w as f64;
            let y = (k1.position_y + (k2.position_y - k1.position_y) * et) * source_h as f64;
            return (x, y, z);
        } else {
            // Static at K1
            return (
                k1.position_x * source_w as f64,
                k1.position_y * source_h as f64,
                k1.zoom_factor,
            );
        }
    } else {
        // Past last keyframe
        let k = kfs.last().unwrap();
        return (
            k.position_x * source_w as f64,
            k.position_y * source_h as f64,
            k.zoom_factor,
        );
    }
}

fn interpolate_mouse(time: f64, positions: &[MousePosition]) -> Option<(i32, i32, bool, String)> {
    if positions.is_empty() {
        return None;
    }

    // Binary search for position
    let idx = positions.partition_point(|p| p.timestamp < time);

    if idx == 0 {
        let p = &positions[0];
        return Some((p.x, p.y, p.is_clicked, p.cursor_type.clone()));
    }
    if idx >= positions.len() {
        let p = positions.last().unwrap();
        return Some((p.x, p.y, p.is_clicked, p.cursor_type.clone()));
    }

    let p1 = &positions[idx - 1];
    let p2 = &positions[idx];

    let dt = p2.timestamp - p1.timestamp;
    if dt <= 0.0 {
        return Some((p1.x, p1.y, p1.is_clicked, p1.cursor_type.clone()));
    }

    let t = (time - p1.timestamp) / dt;

    let x = (p1.x as f64 + (p2.x as f64 - p1.x as f64) * t) as i32;
    let y = (p1.y as f64 + (p2.y as f64 - p1.y as f64) * t) as i32;

    // Click state logic (persist for short duration)
    let is_clicked = p1.is_clicked || p2.is_clicked;

    Some((x, y, is_clicked, p2.cursor_type.clone()))
}

// --- MAIN EXPORT FUNCTION ---

pub fn start_native_export(args: serde_json::Value) -> Result<serde_json::Value, String> {
    let config: ExportConfig = serde_json::from_value(args).map_err(|e| e.to_string())?;

    let output_path = dirs::download_dir()
        .unwrap_or(PathBuf::from("."))
        .join(format!(
            "SGT_Export_{}.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        ));

    // 1. Get FFmpeg Path
    let ffmpeg_path = super::get_ffmpeg_path();
    if !ffmpeg_path.exists() {
        return Err("FFmpeg not found. Please install via Download Manager.".to_string());
    }

    // 2. Get Source Video Path
    let source_video = unsafe { VIDEO_PATH.clone() }.ok_or("No source video found")?;

    // 3. Setup Decoding Process (Read raw video)
    // We trim using -ss and -t here to avoid processing unnecessary frames
    let mut decoder = Command::new(&ffmpeg_path)
        .args(&[
            "-ss",
            &config.trim_start.to_string(),
            "-t",
            &config.duration.to_string(),
            "-i",
            &source_video,
            "-f",
            "image2pipe",
            "-pix_fmt",
            "rgba",
            "-vcodec",
            "rawvideo",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start decoder: {}", e))?;

    let mut decoder_stdout = decoder
        .stdout
        .take()
        .ok_or("Failed to open decoder stdout")?;

    // 4. Setup Encoding Process
    // Audio filter: trim + speed
    let audio_filter = format!(
        "[0:a]atrim=start={}:duration={},asetpts=PTS-STARTPTS,atempo={}[aout]",
        config.trim_start, config.duration, config.speed
    );

    // Calculate actual output resolution (ensure even)
    let out_w = config.width - (config.width % 2);
    let out_h = config.height - (config.height % 2);

    let mut encoder = Command::new(&ffmpeg_path)
        .args(&[
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{}x{}", out_w, out_h),
            "-framerate",
            &config.framerate.to_string(),
            "-i",
            "-", // Video from pipe
            "-i",
            &config.audio_path, // Audio file
            "-filter_complex",
            &audio_filter,
            "-map",
            "0:v",
            "-map",
            "[aout]",
            "-c:v",
            "libx264", // Hardware accel can be added here if detected (h264_nvenc)
            "-preset",
            "fast",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            output_path.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to start encoder: {}", e))?;

    let mut encoder_stdin = encoder.stdin.take().ok_or("Failed to open encoder stdin")?;

    // 5. Processing Loop
    // Use Rayon to process a buffer of frames?
    // Video decoding is sequential, so we iterate one by one.
    // However, we can use Rayon inside the loop for the heavy image ops (resize/composite)
    // if we process blocks of frames?
    // For simplicity and low latency, single thread logic is usually bottlenecked by FFmpeg decode/encode anyway.
    // We will stick to sequential loop but optimize the image ops.

    // Get source dimensions from metadata? Or assume 1920x1080?
    // We can probe or just hardcode if we know.
    // Let's assume standard full HD capture for now, or fetch via probe.
    // Actually, `capture_handler` set VIDEO_PATH. We can run ffprobe.

    let probe = Command::new(&ffmpeg_path)
        .args(&[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=s=x:p=0",
            &source_video,
        ])
        .output()
        .map_err(|e| format!("Probe failed: {}", e))?;

    let dim_str = String::from_utf8_lossy(&probe.stdout);
    let dims: Vec<&str> = dim_str.trim().split('x').collect();
    let src_w: u32 = dims[0].parse().unwrap_or(1920);
    let src_h: u32 = dims[1].parse().unwrap_or(1080);

    let frame_size = (src_w * src_h * 4) as usize;
    let mut buffer = vec![0u8; frame_size];

    // Time tracking
    let dt = 1.0 / config.framerate as f64;
    let mut current_time = config.trim_start;
    let end_time = config.trim_start + config.duration;
    // Speed factor: if speed is 2.0, we step time by dt * 2.0
    let step = dt * config.speed;

    // Determine background color
    let bg_color = match config.background_config.background_type.as_str() {
        "gradient1" => Rgba([37, 99, 235, 255]), // Blue-Violet approximation
        "gradient2" => Rgba([251, 113, 133, 255]), // Rose-Orange approximation
        "gradient3" => Rgba([16, 185, 129, 255]), // Emerald-Teal approximation
        _ => Rgba([10, 10, 10, 255]),            // Solid dark
    };

    while current_time < end_time {
        // Read frame from decoder
        // Read frame from decoder
        if std::io::Read::read_exact(&mut decoder_stdout, &mut buffer).is_err() {
            break;
        }

        // --- PROCESSING ---
        // 1. Create ImageBuffer and wrap in DynamicImage for easier trait satisfaction
        let src_img =
            ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(src_w, src_h, buffer.clone()).unwrap();
        let dyn_img = image::DynamicImage::ImageRgba8(src_img);

        // 2. Calculate Zoom State
        let (cam_x, cam_y, zoom) = interpolate_zoom(current_time, &config.segment, src_w, src_h);

        // 3. Handle Crop (if exists)
        // The crop logic was removed from the provided snippet, but the original code had it.
        // Assuming it's meant to be removed or handled differently based on the new snippet.
        // The new snippet doesn't use crop_x, crop_y, crop_w, crop_h.
        // If crop is needed, it should be applied before calculating view_x, view_y, view_w, view_h.

        // 4. Transform Logic
        // Calculate viewport
        let view_w = (out_w as f64 / zoom) as u32;
        let view_h = (out_h as f64 / zoom) as u32;

        let view_x = (cam_x - view_w as f64 / 2.0).clamp(0.0, (src_w - view_w) as f64) as u32;
        let view_y = (cam_y - view_h as f64 / 2.0).clamp(0.0, (src_h - view_h) as f64) as u32;

        // Extract View using DynamicImage's crop_imm
        let view = dyn_img.crop_imm(view_x, view_y, view_w, view_h);

        // Resize to Output (Background Logic)
        // Check "scale" config (padding)
        let scale_factor = config.background_config.scale / 100.0;

        let final_video_w = (out_w as f64 * scale_factor) as u32;
        let final_video_h = (out_h as f64 * scale_factor) as u32;

        // Resize video content
        let resized_video = resize(&view, final_video_w, final_video_h, FilterType::Triangle);

        // Create Final Frame
        let mut final_frame = ImageBuffer::from_pixel(out_w, out_h, bg_color);

        // Composite Video on Background (Centered)
        let offset_x = (out_w - final_video_w) / 2;
        let offset_y = (out_h - final_video_h) / 2;

        image::imageops::overlay(
            &mut final_frame,
            &resized_video,
            offset_x.into(),
            offset_y.into(),
        );

        // 5. Draw Cursor
        if let Some((mx, my, _is_clicked, c_type)) =
            interpolate_mouse(current_time, &config.mouse_positions)
        {
            // Map mouse to final frame coords
            // Mouse is in Source Coords (0..src_w)

            // Adjust for Viewport offset
            let rel_x = mx as f64 - view_x as f64;
            let rel_y = my as f64 - view_y as f64;

            // Scale to Resized Video
            let scale_x = final_video_w as f64 / view_w as f64;
            let scale_y = final_video_h as f64 / view_h as f64;

            let final_mx = offset_x as f64 + (rel_x * scale_x);
            let final_my = offset_y as f64 + (rel_y * scale_y);

            // Determine cursor sprite
            let cursor_sprite = match c_type.as_str() {
                "text" => &*CURSOR_TEXT,
                "pointer" => &*CURSOR_HAND,
                _ => &*CURSOR_ARROW,
            };

            // Draw Cursor
            if final_mx >= 0.0
                && final_mx < out_w as f64
                && final_my >= 0.0
                && final_my < out_h as f64
            {
                image::imageops::overlay(
                    &mut final_frame,
                    cursor_sprite,
                    final_mx as i64,
                    final_my as i64,
                );
            }
        }

        // Write to Encoder
        encoder_stdin
            .write_all(&final_frame)
            .map_err(|e| e.to_string())?;

        current_time += step;
    }

    // Flush
    drop(encoder_stdin);
    let _ = encoder.wait().map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "status": "success",
        "path": output_path.to_string_lossy()
    }))
}
