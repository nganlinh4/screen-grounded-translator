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

const WM_RELOAD_HOTKEYS: u32 = WM_USER + 101;
const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;

pub mod engine;
use engine::{CaptureHandler, get_monitors, MOUSE_POSITIONS, SHOULD_STOP, ENCODING_FINISHED, ENCODER_ACTIVE, VIDEO_PATH};
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

// Thread-local storage for WebView
thread_local! {
    static SR_WEBVIEW: std::cell::RefCell<Option<Arc<wry::WebView>>> = std::cell::RefCell::new(None);
    static SR_WEB_CONTEXT: std::cell::RefCell<Option<WebContext>> = std::cell::RefCell::new(None);
}

lazy_static::lazy_static! {
    static ref SERVER_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
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
        WM_NCCALCSIZE => {
            if wparam.0 != 0 {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_SIZE => {
            SR_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    let width = r.right - r.left;
                    let height = r.bottom - r.top;
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            width as u32,
                            height as u32,
                        )),
                    });
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// Wrapper for HWND
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
    if ENCODER_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
        // Stop recording
        SHOULD_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    } else {
        // Show overlay to start recording
        show_screen_record();
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

    let width = 1000;
    let height = 700;
    let x = (screen_w - width) / 2;
    let y = (screen_h - height) / 2;

    let hwnd = CreateWindowExW(
        WS_EX_APPWINDOW,
        class_name,
        windows::core::w!("Screen Record"),
        WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_SYSMENU,
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
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    let body = msg.body().as_str();
                    if body == "drag_window" {
                        let _ = ReleaseCapture();
                        let _ = SendMessageW(
                            hwnd,
                            WM_NCLBUTTONDOWN,
                            Some(WPARAM(HTCAPTION as usize)),
                            Some(LPARAM(0)),
                        );
                    } else if body == "close_window" {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    } else if let Ok(req) = serde_json::from_str::<IpcRequest>(body) {
                        let id = req.id.clone();
                        let cmd = req.cmd.clone();
                        
                        thread::spawn(move || {
                            let result = handle_ipc_command(cmd, req.args);
                            SR_WEBVIEW.with(|wv| {
                                if let Some(webview) = wv.borrow().as_ref() {
                                    let json_res = match result {
                                        Ok(res) => serde_json::json!({ "id": id, "result": res }),
                                        Err(err) => serde_json::json!({ "id": id, "error": err }),
                                    };
                                    let script = format!(
                                        "window.dispatchEvent(new CustomEvent('ipc-reply', {{ detail: {} }}))",
                                        json_res.to_string()
                                    );
                                    let _ = webview.evaluate_script(&script);
                                }
                            });
                        });
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

    IS_WARMED_UP = true;

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        let _ = DispatchMessageW(&msg);
    }

    SR_WEBVIEW.with(|wv| {
        *wv.borrow_mut() = None;
    });
    SR_HWND = SendHwnd::default();
    IS_WARMED_UP = false;
    IS_INITIALIZING = false;
}

fn handle_ipc_command(cmd: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
    match cmd.as_str() {
        "get_monitors" => {
            let monitors = get_monitors();
            Ok(serde_json::to_value(monitors).unwrap())
        }
        "start_recording" => {
            let monitor_id = args["monitorId"].as_str().unwrap_or("0");
            let monitor_index = monitor_id.parse::<usize>().unwrap_or(0);
            
            let monitor = Monitor::from_index(monitor_index + 1).map_err(|e| e.to_string())?;

            // Set monitor coordinates for mouse tracking
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
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                monitor_id.to_string(),
            );

            std::thread::spawn(move || {
                let _ = CaptureHandler::start_free_threaded(settings);
            });

            Ok(serde_json::Value::Null)
        }
        "stop_recording" => {
            SHOULD_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
            
            // Wait for encoding to finish
            let start = std::time::Instant::now();
            while !ENCODING_FINISHED.load(std::sync::atomic::Ordering::SeqCst) && start.elapsed().as_secs() < 10 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let video_path = unsafe { VIDEO_PATH.clone() }.ok_or("No video path")?;
            let port = start_video_server(video_path)?;
            
            let mouse_positions = MOUSE_POSITIONS.lock().drain(..).collect::<Vec<_>>();
            let url = format!("http://localhost:{}", port);
            
            Ok(serde_json::json!([url, mouse_positions]))
        }
        "get_hotkey" => {
            let app = APP.lock().unwrap();
            Ok(serde_json::to_value(&app.config.screen_record_hotkey).unwrap())
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

            // Construct pretty name
            let mut name_parts = Vec::new();
            if (modifiers & MOD_CONTROL) != 0 { name_parts.push("Ctrl"); }
            if (modifiers & MOD_ALT) != 0 { name_parts.push("Alt"); }
            if (modifiers & MOD_SHIFT) != 0 { name_parts.push("Shift"); }
            if (modifiers & MOD_WIN) != 0 { name_parts.push("Win"); }
            
            // Format key name (uppercase if single letter)
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
                app.config.screen_record_hotkey = hotkey.clone();
                crate::config::save_config(&app.config);
            }

            // Trigger reload
            trigger_hotkey_reload();

            Ok(serde_json::to_value(&hotkey).unwrap())
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
                Some(chars[3] as u32) // KeyA -> 'A' -> 65
            } else { None }
        },
        c if c.starts_with("Digit") => {
            let chars: Vec<char> = c.chars().collect();
            if chars.len() == 6 {
                Some(chars[5] as u32) // Digit0 -> '0' -> 48
            } else { None }
        },
        c if c.starts_with("F") && c.len() <= 3 => {
             // F1..F12
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
        _ => None,
    }
}

fn start_video_server(video_path: String) -> Result<u16, String> {
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
        if let Ok(file) = File::open(&video_path) {
            let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
            for request in server.incoming_requests() {
                if request.method() == &tiny_http::Method::Options {
                    let mut res = Response::empty(204);
                    res.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                    res.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, OPTIONS"[..]).unwrap());
                    res.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Range"[..]).unwrap());
                    let _ = request.respond(res);
                    continue;
                }

                let mut start = 0;
                let mut end = file_size.saturating_sub(1);

                if let Some(range) = request.headers().iter().find(|h| h.field.as_str() == "Range") {
                    if let Some(r) = range.value.as_str().strip_prefix("bytes=") {
                        let parts: Vec<&str> = r.split('-').collect();
                        if parts.len() == 2 {
                            if let Ok(s) = parts[0].parse::<u64>() { start = s; }
                            if let Ok(e) = parts[1].parse::<u64>() { end = e; }
                        }
                    }
                }

                if let Ok(mut f) = File::open(&video_path) {
                    let _ = f.seek(std::io::SeekFrom::Start(start));
                    let mut res = Response::new(
                        if start == 0 { StatusCode(200) } else { StatusCode(206) },
                        vec![
                            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"video/mp4"[..]).unwrap(),
                            tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
                        ],
                        Box::new(f.take(end - start + 1)) as Box<dyn Read + Send>,
                        Some((end - start + 1) as usize),
                        None,
                    );
                    if start != 0 {
                        res.add_header(tiny_http::Header::from_bytes(&b"Content-Range"[..], format!("bytes {}-{}/{}", start, end, file_size).as_bytes()).unwrap());
                    }
                    let _ = request.respond(res);
                }
            }
        }
    });

    Ok(actual_port)
}
