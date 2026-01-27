use crate::overlay::screen_record::audio_engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::mem::zeroed;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorInfo, GetCursorPos, LoadCursorW, CURSORINFO, IDC_ARROW, IDC_HAND, IDC_IBEAM,
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MousePosition {
    pub x: i32,
    pub y: i32,
    pub timestamp: f64,
    pub is_clicked: bool,
    pub cursor_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

lazy_static::lazy_static! {
    pub static ref MOUSE_POSITIONS: Mutex<VecDeque<MousePosition>> = Mutex::new(VecDeque::new());
    pub static ref IS_RECORDING: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref SHOULD_STOP: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref SHOULD_STOP_AUDIO: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref ENCODING_FINISHED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_ENCODING_FINISHED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref ENCODER_ACTIVE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref IS_MOUSE_CLICKED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    // Track if we already captured the click event (to only record one frame as clicked)
    pub static ref CLICK_CAPTURED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

pub static mut VIDEO_PATH: Option<String> = None;
pub static mut AUDIO_PATH: Option<String> = None;
pub static mut MONITOR_X: i32 = 0;
pub static mut MONITOR_Y: i32 = 0;

pub struct CaptureHandler {
    encoder: Option<VideoEncoder>,
    start: Instant,
    last_mouse_capture: Instant,
    frame_count: u32,
    last_frame_time: Instant,
    dropped_frames: u32,
}

fn get_cursor_type() -> String {
    unsafe {
        let mut cursor_info: CURSORINFO = std::mem::zeroed();
        cursor_info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;

        if GetCursorInfo(&mut cursor_info).is_ok() && cursor_info.flags.0 != 0 {
            let current_handle = cursor_info.hCursor.0;

            let arrow = LoadCursorW(None, IDC_ARROW).unwrap().0;
            let ibeam = LoadCursorW(None, IDC_IBEAM).unwrap().0;
            let hand = LoadCursorW(None, IDC_HAND).unwrap().0;

            if current_handle == arrow {
                "default".to_string()
            } else if current_handle == ibeam {
                "text".to_string()
            } else if current_handle == hand {
                "pointer".to_string()
            } else {
                "other".to_string()
            }
        } else {
            "default".to_string()
        }
    }
}

impl GraphicsCaptureApiHandler for CaptureHandler {
    type Flags = String;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let monitor_index = ctx.flags.parse::<usize>().unwrap_or(0);

        let monitor = Monitor::from_index(monitor_index + 1)?;
        let mut width = monitor.width()?;
        let mut height = monitor.height()?;

        // Ensure even dimensions for encoding
        if width % 2 != 0 {
            width -= 1;
        }
        if height % 2 != 0 {
            height -= 1;
        }

        let app_data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| std::env::temp_dir())
            .join("screen-goated-toolbox")
            .join("recordings");

        std::fs::create_dir_all(&app_data_dir)?;

        let video_path = app_data_dir.join(format!(
            "recording_{}.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));

        let audio_path = app_data_dir.join(format!(
            "recording_{}.wav",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));

        unsafe {
            VIDEO_PATH = Some(video_path.to_string_lossy().to_string());
            AUDIO_PATH = Some(audio_path.to_string_lossy().to_string());
        }

        let video_settings = VideoSettingsBuilder::new(width, height)
            .frame_rate(60)
            .bitrate(15_000_000); // 15Mbps

        println!(
            "Initializing VideoEncoder: {}x{} @ 60fps, Monitor Index: {}",
            width, height, monitor_index
        );

        let encoder = VideoEncoder::new(
            video_settings,
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            &video_path,
        )?;

        SHOULD_STOP_AUDIO.store(false, Ordering::SeqCst);
        AUDIO_ENCODING_FINISHED.store(false, Ordering::SeqCst);
        audio_engine::record_audio(
            audio_path.to_string_lossy().to_string(),
            SHOULD_STOP_AUDIO.clone(),
            AUDIO_ENCODING_FINISHED.clone(),
        );

        ENCODER_ACTIVE.store(true, Ordering::SeqCst);
        ENCODING_FINISHED.store(false, Ordering::SeqCst);

        Ok(Self {
            encoder: Some(encoder),
            start: Instant::now(),
            last_mouse_capture: Instant::now(),
            frame_count: 0,
            last_frame_time: Instant::now(),
            dropped_frames: 0,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if !ENCODER_ACTIVE.load(Ordering::SeqCst) {
            return Ok(());
        }

        if let Err(e) = self.encoder.as_mut().unwrap().send_frame(frame) {
            eprintln!("Encoder error: {}", e);
        }

        if self.frame_count % 60 == 0 {
            // Debug log every ~1 second
            println!("Captured frame {}", self.frame_count);
        }
        self.frame_count += 1;

        if self.last_mouse_capture.elapsed().as_millis() >= 16 {
            unsafe {
                let mut point = POINT::default();
                if GetCursorPos(&mut point).is_ok() {
                    // Record actual held state - cursor should stay squished while held
                    let is_clicked = IS_MOUSE_CLICKED.load(Ordering::SeqCst);
                    let cursor_type = get_cursor_type();

                    let mouse_pos = MousePosition {
                        x: point.x - MONITOR_X,
                        y: point.y - MONITOR_Y,
                        timestamp: self.start.elapsed().as_secs_f64(),
                        is_clicked,
                        cursor_type,
                    };

                    MOUSE_POSITIONS.lock().push_back(mouse_pos);
                }
            }
            self.last_mouse_capture = Instant::now();
        }

        if SHOULD_STOP.load(Ordering::SeqCst) {
            ENCODER_ACTIVE.store(false, Ordering::SeqCst);
            SHOULD_STOP_AUDIO.store(true, Ordering::SeqCst);
            if let Some(encoder) = self.encoder.take() {
                std::thread::spawn(move || {
                    let _ = encoder.finish();
                    ENCODING_FINISHED.store(true, Ordering::SeqCst);
                });
            }
            capture_control.stop();
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub fn get_monitors() -> Vec<MonitorInfo> {
    let mut monitors_vec: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut monitors_vec as *mut _ as isize),
        );

        let mut monitor_infos = Vec::new();
        for (index, &hmonitor) in monitors_vec.iter().enumerate() {
            let mut info: MONITORINFOEXW = zeroed();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

            if GetMonitorInfoW(hmonitor, &mut info.monitorInfo as *mut _).as_bool() {
                let rect = info.monitorInfo.rcMonitor;
                monitor_infos.push(MonitorInfo {
                    id: index.to_string(),
                    name: format!("Display {}", index + 1),
                    x: rect.left,
                    y: rect.top,
                    width: (rect.right - rect.left) as u32,
                    height: (rect.bottom - rect.top) as u32,
                    is_primary: info.monitorInfo.dwFlags & 1 == 1,
                });
            }
        }
        monitor_infos
    }
}

pub unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = &mut *(lparam.0 as *mut Vec<HMONITOR>);
    monitors.push(hmonitor);
    true.into()
}
