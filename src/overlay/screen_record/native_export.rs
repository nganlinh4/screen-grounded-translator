use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::gpu_export::{create_uniforms, GpuCompositor};
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
    pub video_data: Option<Vec<u8>>,
    pub audio_data: Option<Vec<u8>>,
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

// --- GRADIENT COLORS ---
// Colors in sRGB, will be used directly (shader outputs to sRGB texture)

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn hex_to_linear(r: u8, g: u8, b: u8) -> [f32; 4] {
    [
        srgb_to_linear(r as f32 / 255.0),
        srgb_to_linear(g as f32 / 255.0),
        srgb_to_linear(b as f32 / 255.0),
        1.0,
    ]
}

fn get_gradient_colors(bg_type: &str) -> ([f32; 4], [f32; 4]) {
    match bg_type {
        "gradient1" => (
            hex_to_linear(0x25, 0x63, 0xEB), // #2563EB Blue
            hex_to_linear(0x7C, 0x3A, 0xED), // #7C3AED Violet
        ),
        "gradient2" => (
            hex_to_linear(0xFB, 0x71, 0x85), // #FB7185 Rose
            hex_to_linear(0xFD, 0xBA, 0x74), // #FDBA74 Orange
        ),
        "gradient3" => (
            hex_to_linear(0x10, 0xB9, 0x81), // #10B981 Emerald
            hex_to_linear(0x2D, 0xD4, 0xBF), // #2DD4BF Teal
        ),
        _ => (
            hex_to_linear(0x0A, 0x0A, 0x0A), // #0A0A0A Dark
            hex_to_linear(0x00, 0x00, 0x00), // #000000 Black
        ),
    }
}

// --- INTERPOLATION ---

fn interpolate_zoom(
    time: f64,
    motion_path: &Option<Vec<MotionPoint>>,
    default_x: f64,
    default_y: f64,
) -> (f64, f64, f64) {
    if let Some(path) = motion_path {
        if !path.is_empty() {
            let idx = path
                .iter()
                .position(|p| p.time >= time)
                .unwrap_or(path.len().saturating_sub(1));

            if idx == 0 {
                let p = &path[0];
                return (p.x, p.y, p.zoom);
            }

            let p2 = &path[idx.min(path.len() - 1)];
            let p1 = &path[(idx - 1).max(0)];
            let t = ((time - p1.time) / (p2.time - p1.time).max(0.001)).clamp(0.0, 1.0);

            let x = p1.x + (p2.x - p1.x) * t;
            let y = p1.y + (p2.y - p1.y) * t;
            let zoom = p1.zoom + (p2.zoom - p1.zoom) * t;
            return (x, y, zoom);
        }
    }
    (default_x, default_y, 1.0)
}

// --- MAIN EXPORT FUNCTION ---

pub fn start_native_export(args: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut config: ExportConfig = serde_json::from_value(args).map_err(|e| e.to_string())?;

    println!("[Export] Starting GPU-accelerated export...");
    println!(
        "[Export] Config dimensions from frontend: {}x{}",
        config.width, config.height
    );
    println!("[Export] Segment crop: {:?}", config.segment.crop);

    if let Some(path) = &config.segment.smooth_motion_path {
        println!("[Export] Motion Path points: {}", path.len());
        if !path.is_empty() {
            println!("[Export] First motion point: {:?}", path[0]);
        }
    } else {
        println!("[Export] No motion path provided (using default center)");
    }

    // 0. Handle Source Video/Audio
    let mut temp_video_path: Option<PathBuf> = None;
    let mut temp_audio_path: Option<PathBuf> = None;

    let source_video_path = if let Some(video_data) = config.video_data.take() {
        let path = std::env::temp_dir().join("sgt_temp_source.mp4");
        println!("[Export] Writing {} bytes to temp video", video_data.len());
        fs::write(&path, video_data).map_err(|e| format!("Failed to write temp video: {}", e))?;
        temp_video_path = Some(path.clone());
        path.to_string_lossy().to_string()
    } else {
        unsafe { VIDEO_PATH.clone() }.ok_or("No source video found")?
    };

    let source_audio_path = if let Some(audio_data) = config.audio_data.take() {
        let path = std::env::temp_dir().join("sgt_temp_source_audio.wav");
        println!("[Export] Writing {} bytes to temp audio", audio_data.len());
        fs::write(&path, audio_data).map_err(|e| format!("Failed to write temp audio: {}", e))?;
        temp_audio_path = Some(path.clone());
        Some(path.to_string_lossy().to_string())
    } else if !config.audio_path.is_empty() {
        Some(config.audio_path.clone())
    } else {
        None
    };

    let output_path = dirs::download_dir()
        .unwrap_or(PathBuf::from("."))
        .join(format!(
            "SGT_Export_{}.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        ));

    // 1. Setup FFmpeg
    let ffmpeg_path = super::get_ffmpeg_path();
    let ffprobe_path = super::get_ffprobe_path();

    if !ffmpeg_path.exists() {
        return Err("FFmpeg not found.".to_string());
    }

    // 2. Probe source dimensions
    let probe = Command::new(&ffprobe_path)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=s=x:p=0",
            &source_video_path,
        ])
        .output()
        .map_err(|e| format!("Probe failed: {}", e))?;

    let dim_str = String::from_utf8_lossy(&probe.stdout);
    let dims: Vec<&str> = dim_str.trim().split('x').collect();
    let src_w: u32 = dims.get(0).and_then(|s| s.parse().ok()).unwrap_or(1920);
    let src_h: u32 = dims.get(1).and_then(|s| s.parse().ok()).unwrap_or(1080);

    println!("[Export] Source: {}x{}", src_w, src_h);

    // 3. Calculate dimensions - USE CROP DIMENSIONS FOR ASPECT RATIO
    // Apply crop to get the actual visible portion dimensions
    let crop = &config.segment.crop;
    let crop_w = if let Some(c) = crop {
        (src_w as f64 * c.width) as u32
    } else {
        src_w
    };
    let crop_h = if let Some(c) = crop {
        (src_h as f64 * c.height) as u32
    } else {
        src_h
    };

    // Calculate absolute offset of the crop in source coordinates
    let crop_x_offset = if let Some(c) = crop {
        src_w as f64 * c.x
    } else {
        0.0
    };
    let crop_y_offset = if let Some(c) = crop {
        src_h as f64 * c.y
    } else {
        0.0
    };

    // Default camera center (Global coordinates) = Center of the Crop
    // This fixes the shift issue: if motion path is missing, we center on the crop, not the full source.
    let default_cam_x = crop_x_offset + crop_w as f64 / 2.0;
    let default_cam_y = crop_y_offset + crop_h as f64 / 2.0;

    println!(
        "[Export] Crop: {}x{} at ({},{}), Default Cam: ({},{})",
        crop_w, crop_h, crop_x_offset, crop_y_offset, default_cam_x, default_cam_y
    );

    // If config dimensions are 0, use cropped dimensions (matching preview behavior)
    let out_w = if config.width == 0 {
        crop_w
    } else {
        config.width
    };
    let out_h = if config.height == 0 {
        crop_h
    } else {
        config.height
    };
    let out_w = out_w - (out_w % 2);
    let out_h = out_h - (out_h % 2);

    // Calculate video size maintaining CROPPED aspect ratio (not source)
    let scale_factor = config.background_config.scale / 100.0;
    let crop_aspect = crop_w as f64 / crop_h as f64;
    let out_aspect = out_w as f64 / out_h as f64;

    // Fit video within output while maintaining cropped aspect ratio
    let (video_w, video_h) = if crop_aspect > out_aspect {
        // Cropped source is wider - fit to width
        let w = (out_w as f64 * scale_factor) as u32;
        let h = (w as f64 / crop_aspect) as u32;
        (w & !1, h & !1)
    } else {
        // Cropped source is taller - fit to height
        let h = (out_h as f64 * scale_factor) as u32;
        let w = (h as f64 * crop_aspect) as u32;
        (w & !1, h & !1)
    };

    let pad_x = (out_w - video_w) / 2;
    let pad_y = (out_h - video_h) / 2;

    println!(
        "[Export] Output: {}x{}, Video: {}x{}, Offset: {},{}",
        out_w, out_h, video_w, video_h, pad_x, pad_y
    );

    // 4. Initialize GPU compositor - use CROPPED dimensions as video input size
    println!("[Export] Initializing GPU...");
    let compositor = GpuCompositor::new(out_w, out_h, crop_w, crop_h)
        .map_err(|e| format!("GPU init failed: {}", e))?;

    // 5. Start FFmpeg decoder - apply crop filter to extract the cropped region
    // Build crop filter if we have a crop rect
    let crop_filter = if let Some(c) = crop {
        let crop_x = (src_w as f64 * c.x) as u32;
        let crop_y = (src_h as f64 * c.y) as u32;
        format!("crop={}:{}:{}:{}", crop_w, crop_h, crop_x, crop_y)
    } else {
        "null".to_string()
    };

    println!("[Export] Using crop filter: {}", crop_filter);

    let mut decoder = Command::new(&ffmpeg_path)
        .args([
            "-ss",
            &config.trim_start.to_string(),
            "-t",
            &config.duration.to_string(),
            "-i",
            &source_video_path,
            "-vf",
            &crop_filter,
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &format!("{}x{}", crop_w, crop_h),
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Decoder failed: {}", e))?;

    let mut decoder_stdout = decoder
        .stdout
        .take()
        .ok_or("Failed to open decoder stdout")?;

    // 6. Start FFmpeg encoder
    let has_audio = source_audio_path.is_some();

    let mut encoder_args = vec![
        "-y".to_string(),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgba".to_string(),
        "-s".to_string(),
        format!("{}x{}", out_w, out_h),
        "-r".to_string(),
        config.framerate.to_string(),
        "-i".to_string(),
        "-".to_string(),
    ];

    if let Some(audio) = &source_audio_path {
        let audio_filter = if config.speed != 1.0 {
            format!("atempo={}", config.speed.clamp(0.5, 2.0))
        } else {
            "anull".to_string()
        };
        encoder_args.extend([
            "-ss".to_string(),
            config.trim_start.to_string(),
            "-t".to_string(),
            config.duration.to_string(),
            "-i".to_string(),
            audio.clone(),
            "-af".to_string(),
            audio_filter,
        ]);
    }

    encoder_args.extend([
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "fast".to_string(),
        "-crf".to_string(),
        "18".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
    ]);

    if has_audio {
        encoder_args.extend([
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            "192k".to_string(),
            "-shortest".to_string(),
        ]);
    }

    encoder_args.extend([
        "-movflags".to_string(),
        "+faststart".to_string(),
        output_path.to_str().unwrap().to_string(),
    ]);

    let mut encoder = Command::new(&ffmpeg_path)
        .args(&encoder_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Encoder failed: {}", e))?;

    let mut encoder_stdin = encoder.stdin.take().ok_or("Failed to open encoder stdin")?;

    // 7. Process frames
    let (gradient1, gradient2) = get_gradient_colors(&config.background_config.background_type);

    // Use CROPPED dimensions for frame buffer size (decoder outputs cropped frames)
    let frame_size = (crop_w * crop_h * 4) as usize;
    let mut buffer = vec![0u8; frame_size];

    let dt = 1.0 / config.framerate as f64;
    let step = dt * config.speed;
    let mut current_time = config.trim_start;
    let end_time = config.trim_start + config.duration;
    let mut frame_count = 0u32;

    let start = std::time::Instant::now();
    println!("[Export] Processing frames with GPU...");

    while current_time < end_time {
        // Read frame from decoder (cropped frame)
        if std::io::Read::read_exact(&mut decoder_stdout, &mut buffer).is_err() {
            println!("[Export] Decoder finished at frame {}", frame_count);
            break;
        }

        // Get zoom/pan for this frame
        // Pass corrected default coordinates (Center of Crop)
        let (raw_cam_x, raw_cam_y, zoom) = interpolate_zoom(
            current_time - config.trim_start,
            &config.segment.smooth_motion_path,
            default_cam_x,
            default_cam_y,
        );

        // Adjust camera coordinate to be relative to the CROPPED frame
        // If the original camera was at (1000, 500) and we cropped the region starting at (500, 0),
        // the new camera center is at (1000-500, 500-0) = (500, 500) inside the crop.
        let cam_x = raw_cam_x - crop_x_offset;
        let cam_y = raw_cam_y - crop_y_offset;

        // Upload frame to GPU
        compositor.upload_frame(&buffer);

        // Calculate video position based on zoom
        let zoomed_video_w = video_w as f64 * zoom;
        let zoomed_video_h = video_h as f64 * zoom;

        // Calculate max possible shift (slack) based on output dimensions and zoom
        // This prevents the view from panning past the edges of the video container
        let effective_zoomed_out_w = out_w as f64 * zoom;
        let effective_zoomed_out_h = out_h as f64 * zoom;

        let slack_x = (effective_zoomed_out_w - out_w as f64).max(0.0);
        let slack_y = (effective_zoomed_out_h - out_h as f64).max(0.0);

        // Calculate desired shift based on camera position relative to crop center
        // Ratio maps crop pixels to zoomed output pixels
        // We use effective_zoomed_out_w for the ratio to match frontend's "Zoom the Container" model
        let raw_shift_x = (cam_x - crop_w as f64 / 2.0) * (effective_zoomed_out_w / crop_w as f64);
        let raw_shift_y = (cam_y - crop_h as f64 / 2.0) * (effective_zoomed_out_h / crop_h as f64);

        let clamped_shift_x = raw_shift_x.clamp(-slack_x / 2.0, slack_x / 2.0);
        let clamped_shift_y = raw_shift_y.clamp(-slack_y / 2.0, slack_y / 2.0);

        // Calculate final offsets
        // Start with centered position: (out_w - zoomed_video_w) / 2
        // Subtract shift (Camera move Right -> Image move Left)
        let offset_x = ((out_w as f64 - zoomed_video_w) / 2.0 - clamped_shift_x) / out_w as f64;
        let offset_y = ((out_h as f64 - zoomed_video_h) / 2.0 - clamped_shift_y) / out_h as f64;

        if frame_count == 0 {
            println!("[Export] Frame 0 Debug:");
            println!("  raw_cam: ({}, {}), zoom: {}", raw_cam_x, raw_cam_y, zoom);
            println!("  adjusted cam: ({}, {})", cam_x, cam_y);
            println!("  offset: ({}, {})", offset_x, offset_y);
            println!("  zoomed size: {}x{}", zoomed_video_w, zoomed_video_h);
        }

        // Create uniforms
        let uniforms = create_uniforms(
            (offset_x as f32, offset_y as f32),
            (
                zoomed_video_w as f32 / out_w as f32,
                zoomed_video_h as f32 / out_h as f32,
            ),
            (out_w as f32, out_h as f32),
            (zoomed_video_w as f32, zoomed_video_h as f32),
            config.background_config.border_radius as f32,
            config.background_config.shadow as f32 / 4.0,
            config.background_config.shadow as f32 / 2.0,
            (config.background_config.shadow / 100.0).min(0.5) as f32,
            gradient1,
            gradient2,
            (current_time - config.trim_start) as f32,
        );

        // Render on GPU
        let rendered = compositor.render_frame(&uniforms);

        // Write to encoder
        encoder_stdin
            .write_all(&rendered)
            .map_err(|e| e.to_string())?;

        frame_count += 1;
        current_time += step;

        if frame_count % 60 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let fps = frame_count as f64 / elapsed;
            println!("[Export] Frame {}, {:.1} fps", frame_count, fps);
        }
    }

    // Cleanup
    drop(encoder_stdin);
    let _ = decoder.wait();
    let encoder_result = encoder.wait();

    if let Some(p) = temp_video_path {
        let _ = fs::remove_file(p);
    }
    if let Some(p) = temp_audio_path {
        let _ = fs::remove_file(p);
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "[Export] Completed {} frames in {:.1}s ({:.1} fps)",
        frame_count,
        elapsed,
        frame_count as f64 / elapsed
    );

    match encoder_result {
        Ok(status) if status.success() => {
            println!("[Export] Success! Output: {:?}", output_path);
            Ok(serde_json::json!({
                "status": "success",
                "path": output_path.to_string_lossy(),
                "frames": frame_count,
                "duration_seconds": elapsed,
                "fps": frame_count as f64 / elapsed
            }))
        }
        _ => Err("Encoder failed".to_string()),
    }
}
