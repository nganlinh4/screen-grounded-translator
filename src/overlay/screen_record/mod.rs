use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::borrow::Cow;
use std::num::NonZeroIsize;
use std::sync::{Arc, Once};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebViewBuilder};
use serde::Deserialize;
use crate::APP;
use crate::config::Hotkey;
use std::process::Child;
use std::sync::Mutex;
use std::path::PathBuf;

const WM_RELOAD_HOTKEYS: u32 = WM_USER + 101;
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

pub mod engine;
pub mod audio_engine;
pub mod keyviz;
pub mod gpu_export;
pub mod native_export;

use engine::{
    get_monitors, CaptureHandler, AUDIO_ENCODING_FINISHED, ENCODING_FINISHED, MOUSE_POSITIONS,
    SHOULD_STOP, VIDEO_PATH, AUDIO_PATH
};
use windows_capture::capture::GraphicsCaptureApiHandler;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings,
    SecondaryWindowSettings, MinimumUpdateIntervalSettings, DirtyRegionSettings
};
use windows_capture::monitor::Monitor;
use tiny_http::{Server, Response, StatusCode};
use std::fs::File;
use std::io::{Read, Seek};
use std::thread;

use crate::win_types::SendHwnd;

static REGISTER_SR_CLASS: Once = Once::new();
static mut SR_HWND: SendHwnd = SendHwnd(HWND(std::ptr::null_mut()));
static mut IS_WARMED_UP: bool = false;
static mut IS_INITIALIZING: bool = false;
const WM_APP_SHOW: u32 = WM_USER + 110;
const WM_APP_TOGGLE: u32 = WM_USER + 111;
const WM_APP_RUN_SCRIPT: u32 = WM_USER + 112;
const WM_UNREGISTER_HOTKEYS: u32 = WM_USER + 103;
const WM_REGISTER_HOTKEYS: u32 = WM_USER + 104;

thread_local! {
    static SR_WEBVIEW: std::cell::RefCell<Option<std::sync::Arc<wry::WebView>>> = std::cell::RefCell::new(None);
    static SR_WEB_CONTEXT: std::cell::RefCell<Option<WebContext>> = std::cell::RefCell::new(None);
}

lazy_static::lazy_static! {
    static ref SERVER_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    static ref FFMPEG_PROCESS: Mutex<Option<Child>> = Mutex::new(None);
}

#[derive(Deserialize)]
struct IpcRequest {
    id: String,
    cmd: String,
    args: serde_json::Value,
}

// Assets
const INDEX_HTML: &[u8] = include_bytes!("dist/index.html");
const ASSET_INDEX_JS: &[u8] = include_bytes!("dist/assets/index.js");
const ASSET_INDEX_CSS: &[u8] = include_bytes!("dist/assets/index.css");
const ASSET_VITE_SVG: &[u8] = include_bytes!("dist/vite.svg");
const ASSET_TAURI_SVG: &[u8] = include_bytes!("dist/tauri.svg");
const ASSET_POINTER_SVG: &[u8] = include_bytes!("dist/pointer.svg");
const ASSET_SCREENSHOT_PNG: &[u8] = include_bytes!("dist/screenshot.png");

unsafe extern "system" fn sr_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW => {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_NCCALCSIZE => LRESULT(0),
        WM_NCHITTEST => {
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            let x = (lparam.0 & 0xFFFF) as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i32;
            
            let border = 8;
            let title_height = 44; 

            if y < rect.top + border {
                if x < rect.left + border { return LRESULT(HTTOPLEFT as isize); }
                if x > rect.right - border { return LRESULT(HTTOPRIGHT as isize); }
                return LRESULT(HTTOP as isize);
            }
            if y > rect.bottom - border {
                if x < rect.left + border { return LRESULT(HTBOTTOMLEFT as isize); }
                if x > rect.right - border { return LRESULT(HTBOTTOMRIGHT as isize); }
                return LRESULT(HTBOTTOM as isize);
            }
            if x < rect.left + border { return LRESULT(HTLEFT as isize); }
            if x > rect.right - border { return LRESULT(HTRIGHT as isize); }

            if y < rect.top + title_height {
                return LRESULT(HTCLIENT as isize);
            }

            LRESULT(HTCLIENT as isize)
        }
        WM_SIZE => {
            SR_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let width = r.right - r.left;
                    let height = r.bottom - r.top;
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(width as u32, height as u32)),
                    });
                }
            });
            LRESULT(0)
        }
        WM_APP_TOGGLE => {
            SR_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let _ = webview.evaluate_script("window.dispatchEvent(new CustomEvent('toggle-recording'));");
                }
            });
            LRESULT(0)
        }
        WM_SETFOCUS => {
            SR_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let _ = webview.focus();
                }
            });
            LRESULT(0)
        }
        WM_APP_RUN_SCRIPT => {
            let script_ptr = lparam.0 as *mut String;
            if !script_ptr.is_null() {
                let script = unsafe { Box::from_raw(script_ptr) };
                SR_WEBVIEW.with(|wv| {
                    if let Some(webview) = wv.borrow().as_ref() {
                        let _ = webview.evaluate_script(&script);
                    }
                });
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

struct HwndWrapper(HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.0 .0 as isize;
        if hwnd == 0 {
            return Err(HandleError::Unavailable);
        }
        if let Some(non_zero) = NonZeroIsize::new(hwnd) {
            let mut handle = Win32WindowHandle::new(non_zero);
            handle.hinstance = None;
            let raw = RawWindowHandle::Win32(handle);
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        } else {
            Err(HandleError::Unavailable)
        }
    }
}

fn wnd_http_response(
    status: u16,
    content_type: &str,
    body: Cow<'static, [u8]>,
) -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_else(|_| {
            wry::http::Response::builder()
                .status(500)
                .body(Cow::Borrowed(b"Internal Error".as_slice()))
                .unwrap()
        })
}

pub fn show_screen_record() {
    unsafe {
        if !IS_WARMED_UP {
            if !IS_INITIALIZING {
                IS_INITIALIZING = true;
                std::thread::spawn(|| {
                    internal_create_sr_loop();
                });
            }

            std::thread::spawn(|| {
                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let hwnd_wrapper = std::ptr::addr_of!(SR_HWND).read();
                    if IS_WARMED_UP && !hwnd_wrapper.is_invalid() {
                        let _ =
                            PostMessageW(Some(hwnd_wrapper.0), WM_APP_SHOW, WPARAM(0), LPARAM(0));
                        return;
                    }
                }
            });
            return;
        }

        let hwnd_wrapper = std::ptr::addr_of!(SR_HWND).read();
        if !hwnd_wrapper.is_invalid() {
            let _ = PostMessageW(Some(hwnd_wrapper.0), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn toggle_recording() {
    unsafe {
        let hwnd_wrapper = std::ptr::addr_of!(SR_HWND).read();
        
        if hwnd_wrapper.is_invalid() {
            show_screen_record();
        } else {
            if IsWindowVisible(hwnd_wrapper.0).as_bool() {
                let _ = PostMessageW(Some(hwnd_wrapper.0), WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
            } else {
                show_screen_record();
            }
        }
    }
}

unsafe fn internal_create_sr_loop() {
    let instance = GetModuleHandleW(None).unwrap();
    let class_name = windows::core::w!("ScreenRecord_Class");

    REGISTER_SR_CLASS.call_once(|| {
        let mut wc = WNDCLASSW::default();
        wc.lpfnWndProc = Some(sr_wnd_proc);
        wc.hInstance = instance.into();
        wc.lpszClassName = class_name;
        wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
        wc.hbrBackground = HBRUSH(std::ptr::null_mut());
        let _ = RegisterClassW(&wc);
    });

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);

    let width = 1300;
    let height = 850;
    let x = (screen_w - width) / 2;
    let y = (screen_h - height) / 2;

    let hwnd = CreateWindowExW(
        WS_EX_APPWINDOW,
        class_name,
        windows::core::w!("Screen Record"),
        WS_POPUP | WS_THICKFRAME | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
        x,
        y,
        width,
        height,
        None,
        None,
        Some(instance.into()),
        None,
    )
    .unwrap();

    SR_HWND = SendHwnd(hwnd);

    let corner_pref = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner_pref as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&corner_pref) as u32,
    );

    let wrapper = HwndWrapper(hwnd);

    SR_WEB_CONTEXT.with(|ctx| {
        if ctx.borrow().is_none() {
            let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("common"));
            *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
        }
    });

    std::thread::sleep(std::time::Duration::from_millis(100));

    let webview_result = {
        let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();

        SR_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let mut builder = WebViewBuilder::new_with_web_context(ctx_ref.as_mut().unwrap())
                .with_custom_protocol("screenrecord".to_string(), move |_id, request| {
                    let path = request.uri().path();
                    let (content, mime) = if path == "/" || path == "/index.html" {
                        (Cow::Borrowed(INDEX_HTML), "text/html")
                    } else if path.ends_with("index.js") {
                        (Cow::Borrowed(ASSET_INDEX_JS), "application/javascript")
                    } else if path.ends_with("index.css") {
                        (Cow::Borrowed(ASSET_INDEX_CSS), "text/css")
                    } else if path.ends_with("vite.svg") {
                        (Cow::Borrowed(ASSET_VITE_SVG), "image/svg+xml")
                    } else if path.ends_with("tauri.svg") {
                        (Cow::Borrowed(ASSET_TAURI_SVG), "image/svg+xml")
                    } else if path.ends_with("pointer.svg") {
                        (Cow::Borrowed(ASSET_POINTER_SVG), "image/svg+xml")
                    } else if path.ends_with("screenshot.png") {
                        (Cow::Borrowed(ASSET_SCREENSHOT_PNG), "image/png")
                    } else {
                        return wnd_http_response(
                            404,
                            "text/plain",
                            Cow::Borrowed(b"Not Found".as_slice()),
                        );
                    };
                    wnd_http_response(200, mime, content)
                })
                .with_initialization_script(r#"
                    (function() {
                        const originalPostMessage = window.ipc.postMessage;
                        window.__TAURI_INTERNALS__ = {
                            invoke: async (cmd, args) => {
                                return new Promise((resolve, reject) => {
                                    const id = Math.random().toString(36).substring(7);
                                    const handler = (e) => {
                                        if (e.detail && e.detail.id === id) {
                                            window.removeEventListener('ipc-reply', handler);
                                            if (e.detail.error) reject(e.detail.error);
                                            else resolve(e.detail.result);
                                        }
                                    };
                                    window.addEventListener('ipc-reply', handler);
                                    originalPostMessage(JSON.stringify({ id, cmd, args }));
                                });
                            }
                        };
                        window.__TAURI__ = {
                            core: {
                                invoke: window.__TAURI_INTERNALS__.invoke
                            }
                        };
                    })();
                "#)
                .with_ipc_handler({
                    let send_hwnd = SendHwnd(hwnd);
                    move |msg: wry::http::Request<String>| {
                        let body = msg.body().as_str();
                        let hwnd = send_hwnd.0;
                        if body == "drag_window" {
                            let _ = ReleaseCapture();
                            let _ = SendMessageW(
                                hwnd,
                                WM_NCLBUTTONDOWN,
                                Some(WPARAM(HTCAPTION as usize)),
                                Some(LPARAM(0)),
                            );
                        } else if body == "minimize_window" {
                            let _ = ShowWindow(hwnd, SW_MINIMIZE);
                        } else if body == "toggle_maximize" {
                            if unsafe { IsZoomed(hwnd).as_bool() } {
                                let _ = ShowWindow(hwnd, SW_RESTORE);
                            } else {
                                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                            }
                        } else if body == "close_window" {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        } else if let Ok(req) = serde_json::from_str::<IpcRequest>(body) {
                            let id = req.id;
                            let cmd = req.cmd;
                            let args = req.args;
                            let target_hwnd_val = send_hwnd.as_isize();
                            
                            thread::spawn(move || {
                                let result = handle_ipc_command(cmd, args);
                                let json_res = match result {
                                    Ok(res) => serde_json::json!({ "id": id, "result": res }),
                                    Err(err) => serde_json::json!({ "id": id, "error": err }),
                                };
                                let script = format!(
                                    "window.dispatchEvent(new CustomEvent('ipc-reply', {{ detail: {} }}))",
                                    json_res.to_string()
                                );
                                
                                let script_ptr = Box::into_raw(Box::new(script));
                                unsafe {
                                    let _ = PostMessageW(
                                        Some(HWND(target_hwnd_val as *mut std::ffi::c_void)), 
                                        WM_APP_RUN_SCRIPT, 
                                        WPARAM(0), 
                                        LPARAM(script_ptr as isize)
                                    );
                                }
                            });
                        }
                    }
                })
                .with_url("screenrecord://localhost/index.html");

            builder = crate::overlay::html_components::font_manager::configure_webview(builder);
            builder.build_as_child(&wrapper)
        })
    };

    let webview = match webview_result {
        Ok(wv) => wv,
        Err(e) => {
            eprintln!("Failed to create ScreenRecord WebView: {:?}", e);
            let _ = DestroyWindow(hwnd);
            SR_HWND = SendHwnd::default();
            return;
        }
    };
    let webview_arc = Arc::new(webview);

    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    let _ = webview_arc.set_bounds(Rect {
        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
            (r.right - r.left) as u32,
            (r.bottom - r.top) as u32,
        )),
    });

    SR_WEBVIEW.with(|wv| {
        *wv.borrow_mut() = Some(webview_arc);
    });

    unsafe {
        IS_WARMED_UP = true;
    }

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }

    SR_WEBVIEW.with(|wv| {
        *wv.borrow_mut() = None;
    });
    unsafe {
        SR_HWND = SendHwnd::default();
        IS_WARMED_UP = false;
        IS_INITIALIZING = false;
    }
}

fn get_ffmpeg_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("screen-goated-toolbox")
        .join("bin")
        .join("ffmpeg.exe")
}

pub fn get_ffprobe_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("screen-goated-toolbox")
        .join("bin")
        .join("ffprobe.exe")
}

fn get_bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("screen-goated-toolbox")
        .join("bin")
}

fn start_ffmpeg_installation() {
    use std::io::{Read as IoRead, Write};
    
    // Check if already downloading
    {
        let status = FFMPEG_INSTALL_STATUS.lock().unwrap();
        if matches!(*status, FfmpegInstallStatus::Downloading(_) | FfmpegInstallStatus::Extracting) {
            return;
        }
    }
    
    *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Downloading(0.0);
    
    thread::spawn(move || {
        let bin_dir = get_bin_dir();
        if !bin_dir.exists() {
            let _ = std::fs::create_dir_all(&bin_dir);
        }
        
        let url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
        let zip_path = bin_dir.join("ffmpeg.zip");
        
        // Download with progress from a more reliable mirror if needed, or just with a User-Agent
        match ureq::get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .call() {
            Ok(response) => {
                let total_size = response.headers()
                    .get("Content-Length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                
                let mut reader = response.into_body().into_reader();
                let mut file = match std::fs::File::create(&zip_path) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Error(format!("Failed to create zip file: {}", e));
                        return;
                    }
                };
                
                let mut downloaded: u64 = 0;
                let mut buffer = [0u8; 16384]; // Larger buffer
                
                loop {
                    // Check for cancellation
                    if matches!(*FFMPEG_INSTALL_STATUS.lock().unwrap(), FfmpegInstallStatus::Cancelled) {
                        drop(file);
                        let _ = std::fs::remove_file(&zip_path);
                        return;
                    }
                    
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Err(e) = file.write_all(&buffer[..n]) {
                                *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Error(format!("Write error: {}", e));
                                return;
                            }
                            downloaded += n as u64;
                            
                            // Update progress frequently
                            let mut status = FFMPEG_INSTALL_STATUS.lock().unwrap();
                            if total_size > 0 {
                                let progress = (downloaded as f32 / total_size as f32) * 100.0;
                                *status = FfmpegInstallStatus::Downloading(progress);
                            } else {
                                // If size is unknown, just show that we are downloading (0.1 to avoid showing 0%)
                                *status = FfmpegInstallStatus::Downloading(0.1);
                            }
                        }
                        Err(e) => {
                            *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Error(format!("Read error: {}", e));
                            return;
                        }
                    }
                }
                
                // Ensure file is flushed and closed
                let _ = file.sync_all();
                drop(file);
                
                // Check for cancellation before extracting
                if matches!(*FFMPEG_INSTALL_STATUS.lock().unwrap(), FfmpegInstallStatus::Cancelled) {
                    let _ = std::fs::remove_file(&zip_path);
                    return;
                }
                
                *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Extracting;
                
                // Extract ffmpeg and ffprobe
                match extract_ffmpeg_zip(&zip_path, &bin_dir) {
                    Ok(_) => {
                        let _ = std::fs::remove_file(&zip_path);
                        *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Installed;
                    }
                    Err(e) => {
                        *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Error(e);
                    }
                }
            }
            Err(e) => {
                *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Error(e.to_string());
            }
        }
    });
}

fn extract_ffmpeg_zip(zip_path: &PathBuf, bin_dir: &PathBuf) -> Result<(), String> {
    use std::io::{Read as IoRead, Write};
    
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    
    let mut found_ffmpeg = false;
    let mut found_ffprobe = false;
    
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        
        // Extract ffmpeg.exe
        if name.ends_with("ffmpeg.exe") {
            let mut out = std::fs::File::create(bin_dir.join("ffmpeg.exe")).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            found_ffmpeg = true;
        }
        
        // Extract ffprobe.exe
        if name.ends_with("ffprobe.exe") {
            let mut out = std::fs::File::create(bin_dir.join("ffprobe.exe")).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            found_ffprobe = true;
        }
        
        if found_ffmpeg && found_ffprobe {
            break;
        }
    }
    
    if !found_ffmpeg {
        return Err("ffmpeg.exe not found in archive".to_string());
    }
    if !found_ffprobe {
        return Err("ffprobe.exe not found in archive".to_string());
    }
    
    Ok(())
}

// FFmpeg installation state for screen record
lazy_static::lazy_static! {
    static ref FFMPEG_INSTALL_STATUS: Mutex<FfmpegInstallStatus> = Mutex::new(FfmpegInstallStatus::Idle);
}

#[derive(Clone, serde::Serialize)]
pub enum FfmpegInstallStatus {
    Idle,
    Downloading(f32), // progress 0.0-1.0
    Extracting,
    Installed,
    Error(String),
    Cancelled,
}

fn handle_ipc_command(cmd: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
    match cmd.as_str() {
        "check_ffmpeg_status" => {
            let ffmpeg_path = get_ffmpeg_path();
            let ffprobe_path = get_ffprobe_path();
            let ffmpeg_missing = !ffmpeg_path.exists();
            let ffprobe_missing = !ffprobe_path.exists();
            Ok(serde_json::json!({
                "ffmpegMissing": ffmpeg_missing,
                "ffprobeMissing": ffprobe_missing
            }))
        }
        "start_ffmpeg_install" => {
            start_ffmpeg_installation();
            Ok(serde_json::Value::Null)
        }
        "get_ffmpeg_install_progress" => {
            let status = FFMPEG_INSTALL_STATUS.lock().unwrap().clone();
            Ok(serde_json::to_value(&status).unwrap())
        }
        "cancel_ffmpeg_install" => {
            *FFMPEG_INSTALL_STATUS.lock().unwrap() = FfmpegInstallStatus::Cancelled;
            Ok(serde_json::Value::Null)
        }
        "start_export_server" => {
            native_export::start_native_export(args)
        }
        "get_monitors" => {
            let monitors = get_monitors();
            Ok(serde_json::to_value(monitors).unwrap())
        }
        "install_keyviz" => {
            std::thread::spawn(|| {
                if let Err(e) = keyviz::install_keyviz() {
                    crate::log_info!("Keyviz install failed: {}", e);
                }
            });
            Ok(serde_json::Value::Null)
        }
        "set_keyviz_enabled" => {
            let enabled = args["enabled"].as_bool().unwrap_or(false);
            keyviz::set_enabled(enabled);
            Ok(serde_json::Value::Null)
        }
        "get_keyviz_status" => {
            Ok(serde_json::json!({
                "installed": keyviz::is_installed(),
                "enabled": keyviz::is_enabled()
            }))
        }
        "start_recording" => {
            let monitor_id = args["monitorId"].as_str().unwrap_or("0");
            let monitor_index = monitor_id.parse::<usize>().unwrap_or(0);
            
            SHOULD_STOP.store(false, std::sync::atomic::Ordering::SeqCst);
            crate::overlay::screen_record::engine::IS_MOUSE_CLICKED.store(false, std::sync::atomic::Ordering::SeqCst);
            crate::overlay::screen_record::engine::CLICK_CAPTURED.store(false, std::sync::atomic::Ordering::SeqCst);
            crate::overlay::screen_record::engine::MOUSE_POSITIONS.lock().clear();
            
            let monitor = Monitor::from_index(monitor_index + 1).map_err(|e| e.to_string())?;

            unsafe {
                let mut monitors: Vec<windows::Win32::Graphics::Gdi::HMONITOR> = Vec::new();
                let _ = windows::Win32::Graphics::Gdi::EnumDisplayMonitors(
                    None,
                    None,
                    Some(crate::overlay::screen_record::engine::monitor_enum_proc),
                    windows::Win32::Foundation::LPARAM(&mut monitors as *mut _ as isize),
                );
                if let Some(&hmonitor) = monitors.get(monitor_index) {
                    let mut info: windows::Win32::Graphics::Gdi::MONITORINFOEXW = std::mem::zeroed();
                    info.monitorInfo.cbSize = std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFOEXW>() as u32;
                    if windows::Win32::Graphics::Gdi::GetMonitorInfoW(hmonitor, &mut info.monitorInfo as *mut _).as_bool() {
                        crate::overlay::screen_record::engine::MONITOR_X = info.monitorInfo.rcMonitor.left;
                        crate::overlay::screen_record::engine::MONITOR_Y = info.monitorInfo.rcMonitor.top;
                    }
                }
            }

            let settings = Settings::new(
                monitor,
                CursorCaptureSettings::WithoutCursor,
                DrawBorderSettings::Default,
                SecondaryWindowSettings::Include,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                monitor_id.to_string(),
            );

            let _ = keyviz::start();

            std::thread::spawn(move || {
                let _ = CaptureHandler::start_free_threaded(settings);
            });

            Ok(serde_json::Value::Null)
        }
        "stop_recording" => {
            SHOULD_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = keyviz::stop();
            
            let start = std::time::Instant::now();
            while (!ENCODING_FINISHED.load(std::sync::atomic::Ordering::SeqCst) || 
                   !AUDIO_ENCODING_FINISHED.load(std::sync::atomic::Ordering::SeqCst)) && 
                  start.elapsed().as_secs() < 10 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let video_path = unsafe { VIDEO_PATH.clone() }.ok_or("No video path")?;
            let audio_path = unsafe { AUDIO_PATH.clone() }.ok_or("No audio path")?;
            
            let port = start_media_server(video_path, audio_path.clone())?;
            
            let mouse_positions = MOUSE_POSITIONS.lock().drain(..).collect::<Vec<_>>();
            
            let video_url = format!("http://localhost:{}/video", port);
            let audio_url = format!("http://localhost:{}/audio", port);
            
            let audio_file_path = audio_path; 

            Ok(serde_json::json!([video_url, audio_url, mouse_positions, audio_file_path]))
        }
        "get_hotkeys" => {
            let app = APP.lock().unwrap();
            Ok(serde_json::to_value(&app.config.screen_record_hotkeys).unwrap())
        }
        "remove_hotkey" => {
            let index = args["index"].as_u64().ok_or("Missing index")? as usize;
            {
                let mut app = APP.lock().unwrap();
                if index < app.config.screen_record_hotkeys.len() {
                    app.config.screen_record_hotkeys.remove(index);
                    crate::config::save_config(&app.config);
                }
            }
            trigger_hotkey_reload();
            Ok(serde_json::Value::Null)
        }
        "set_hotkey" => {
            let code_str = args["code"].as_str().ok_or("Missing code")?;
            let mods_arr = args["modifiers"].as_array().ok_or("Missing modifiers")?;
            let key_name = args["key"].as_str().unwrap_or("Unknown");

            let vk_code = js_code_to_vk(code_str).ok_or(format!("Unsupported key code: {}", code_str))?;
            
            let mut modifiers = 0;
            for m in mods_arr {
                match m.as_str() {
                    Some("Control") => modifiers |= MOD_CONTROL,
                    Some("Alt") => modifiers |= MOD_ALT,
                    Some("Shift") => modifiers |= MOD_SHIFT,
                    Some("Meta") => modifiers |= MOD_WIN,
                    _ => {}
                }
            }

            {
                let app = APP.lock().unwrap();
                if let Some(msg) = app.config.check_hotkey_conflict(vk_code, modifiers, None) {
                    return Err(msg);
                }
            }

            let mut name_parts = Vec::new();
            if (modifiers & MOD_CONTROL) != 0 { name_parts.push("Ctrl"); }
            if (modifiers & MOD_ALT) != 0 { name_parts.push("Alt"); }
            if (modifiers & MOD_SHIFT) != 0 { name_parts.push("Shift"); }
            if (modifiers & MOD_WIN) != 0 { name_parts.push("Win"); }
            
            let formatted_key = if key_name.len() == 1 {
                key_name.to_uppercase()
            } else {
                match key_name {
                    " " => "Space".to_string(),
                    _ => key_name.to_string(),
                }
            };
            name_parts.push(&formatted_key);
            
            let hotkey = Hotkey {
                code: vk_code,
                modifiers,
                name: name_parts.join(" + "),
            };

            {
                let mut app = APP.lock().unwrap();
                app.config.screen_record_hotkeys.push(hotkey.clone());
                crate::config::save_config(&app.config);
            }

            trigger_hotkey_reload();

            Ok(serde_json::to_value(&hotkey).unwrap())
        }
        "unregister_hotkeys" => {
            unsafe {
                if let Ok(hwnd) = FindWindowW(windows::core::w!("HotkeyListenerClass"), windows::core::w!("Listener")) {
                    if !hwnd.is_invalid() {
                        let _ = PostMessageW(Some(hwnd), WM_UNREGISTER_HOTKEYS, WPARAM(0), LPARAM(0));
                    }
                }
            }
            Ok(serde_json::Value::Null)
        }
        "register_hotkeys" => {
            unsafe {
                if let Ok(hwnd) = FindWindowW(windows::core::w!("HotkeyListenerClass"), windows::core::w!("Listener")) {
                    if !hwnd.is_invalid() {
                        let _ = PostMessageW(Some(hwnd), WM_REGISTER_HOTKEYS, WPARAM(0), LPARAM(0));
                    }
                }
            }
            Ok(serde_json::Value::Null)
        }
        "minimize_window" => {
            unsafe {
                let hwnd = std::ptr::addr_of!(SR_HWND).read();
                if !hwnd.is_invalid() {
                    let _ = ShowWindow(hwnd.0, SW_MINIMIZE);
                }
            }
            Ok(serde_json::Value::Null)
        }
        "toggle_maximize" => {
            unsafe {
                let hwnd = std::ptr::addr_of!(SR_HWND).read();
                if !hwnd.is_invalid() {
                    if IsZoomed(hwnd.0).as_bool() {
                        let _ = ShowWindow(hwnd.0, SW_RESTORE);
                    } else {
                        let _ = ShowWindow(hwnd.0, SW_MAXIMIZE);
                    }
                }
            }
            Ok(serde_json::Value::Null)
        }
        "close_window" => {
            unsafe {
                let hwnd = std::ptr::addr_of!(SR_HWND).read();
                if !hwnd.is_invalid() {
                    let _ = ShowWindow(hwnd.0, SW_HIDE);
                }
            }
            Ok(serde_json::Value::Null)
        }
        "is_maximized" => {
            unsafe {
                let hwnd = std::ptr::addr_of!(SR_HWND).read();
                let maximized = if !hwnd.is_invalid() {
                    IsZoomed(hwnd.0).as_bool()
                } else {
                    false
                };
                Ok(serde_json::json!(maximized))
            }
        }
        _ => Err(format!("Unknown command: {}", cmd)),
    }
}

fn trigger_hotkey_reload() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(windows::core::w!("HotkeyListenerClass"), windows::core::w!("Listener")) {
            if !hwnd.is_invalid() {
                let _ = PostMessageW(Some(hwnd), WM_RELOAD_HOTKEYS, WPARAM(0), LPARAM(0));
            }
        }
    }
}

fn js_code_to_vk(code: &str) -> Option<u32> {
    match code {
        c if c.starts_with("Key") => {
            let chars: Vec<char> = c.chars().collect();
            if chars.len() == 4 {
                Some(chars[3] as u32) 
            } else { None }
        },
        c if c.starts_with("Digit") => {
            let chars: Vec<char> = c.chars().collect();
            if chars.len() == 6 {
                Some(chars[5] as u32) 
            } else { None }
        },
        c if c.starts_with("F") && c.len() <= 3 => {
             c[1..].parse::<u32>().ok().map(|n| 0x70 + n - 1)
        },
        "Space" => Some(0x20),
        "Enter" => Some(0x0D),
        "Escape" => Some(0x1B),
        "Backspace" => Some(0x08),
        "Tab" => Some(0x09),
        "Delete" => Some(0x2E),
        "Insert" => Some(0x2D),
        "Home" => Some(0x24),
        "End" => Some(0x23),
        "PageUp" => Some(0x21),
        "PageDown" => Some(0x22),
        "ArrowUp" => Some(0x26),
        "ArrowDown" => Some(0x28),
        "ArrowLeft" => Some(0x25),
        "ArrowRight" => Some(0x27),
        "Backquote" => Some(0xC0),
        "Minus" => Some(0xBD),
        "Equal" => Some(0xBB),
        "BracketLeft" => Some(0xDB),
        "BracketRight" => Some(0xDD),
        "Backslash" => Some(0xDC),
        "Semicolon" => Some(0xBA),
        "Quote" => Some(0xDE),
        "Comma" => Some(0xBC),
        "Period" => Some(0xBE),
        "Slash" => Some(0xBF),
        c if c.starts_with("Numpad") => {
            let chars: Vec<char> = c.chars().collect();
            if chars.len() == 7 {
                Some(chars[6] as u32 + 0x30) 
            } else { None }
        },
        _ => None,
    }
}

fn start_media_server(video_path: String, audio_path: String) -> Result<u16, String> {
    let mut port = 8000;
    let server = loop {
        match Server::http(format!("127.0.0.1:{}", port)) {
            Ok(s) => break s,
            Err(_) => {
                port += 1;
                if port > 9000 { return Err("No port available".to_string()); }
            }
        }
    };

    let actual_port = port;
    SERVER_PORT.store(actual_port, std::sync::atomic::Ordering::SeqCst);

    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            if request.method() == &tiny_http::Method::Options {
                let mut res = Response::empty(204);
                res.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                res.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, OPTIONS"[..]).unwrap());
                res.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Range"[..]).unwrap());
                let _ = request.respond(res);
                continue;
            }

            let url = request.url();
            let is_audio = url.contains("audio");
            let media_path = if is_audio { &audio_path } else { &video_path };
            let content_type = if is_audio { "audio/wav" } else { "video/mp4" };

            if let Ok(file) = File::open(media_path) {
                let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
                let mut start = 0;
                let mut end = file_size.saturating_sub(1);

                if let Some(range) = request.headers().iter().find(|h| h.field.as_str() == "Range") {
                    if let Some(r) = range.value.as_str().strip_prefix("bytes=") {
                        let parts: Vec<&str> = r.split('-').collect();
                        if parts.len() == 2 {
                            if let Ok(s) = parts[0].parse::<u64>() { start = s; }
                            if let Ok(e) = parts[1].parse::<u64>() {
                                if !parts[1].is_empty() { end = e; }
                            }
                        }
                    }
                }

                if let Ok(mut f) = File::open(media_path) {
                    let _ = f.seek(std::io::SeekFrom::Start(start));
                    let mut res = Response::new(
                        if start == 0 && end == file_size.saturating_sub(1) { StatusCode(200) } else { StatusCode(206) },
                        vec![
                            tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
                            tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
                        ],
                        Box::new(f.take(end - start + 1)) as Box<dyn Read + Send>,
                        Some((end - start + 1) as usize),
                        None,
                    );
                    if start != 0 || end != file_size.saturating_sub(1) {
                        res.add_header(tiny_http::Header::from_bytes(&b"Content-Range"[..], format!("bytes {}-{}/{}", start, end, file_size).as_bytes()).unwrap());
                    }
                    let _ = request.respond(res);
                }
            } else {
                let _ = request.respond(Response::from_string("File not found").with_status_code(404));
            }
        }
    });

    Ok(actual_port)
}