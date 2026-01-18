use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::borrow::Cow;
use std::num::NonZeroIsize;
use std::sync::{Arc, Once};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebViewBuilder};

use crate::win_types::SendHwnd;

static REGISTER_PDJ_CLASS: Once = Once::new();
static mut PDJ_HWND: SendHwnd = SendHwnd(HWND(std::ptr::null_mut()));
static mut IS_WARMED_UP: bool = false;
static mut IS_INITIALIZING: bool = false;
const WM_APP_SHOW: u32 = WM_USER + 101;
const WM_APP_UPDATE_SETTINGS: u32 = WM_USER + 102;

// Thread-local storage for WebView
thread_local! {
    static PDJ_WEBVIEW: std::cell::RefCell<Option<Arc<wry::WebView>>> = std::cell::RefCell::new(None);
    static PDJ_WEB_CONTEXT: std::cell::RefCell<Option<WebContext>> = std::cell::RefCell::new(None);
}

// Assets
const INDEX_HTML: &[u8] = include_bytes!("dist/index.html");
const ASSET_INDEX_JS: &[u8] = include_bytes!("dist/assets/index.js");
const ASSET_INDEX_CSS: &[u8] = include_bytes!("dist/assets/index.css");
const ASSET_CUBIC_JS: &[u8] = include_bytes!("dist/assets/cubic.js");
const ASSET_MORPH_JS: &[u8] = include_bytes!("dist/assets/morph-fixed.js");
const ASSET_ROUNDED_JS: &[u8] = include_bytes!("dist/assets/roundedPolygon.js");
const ASSET_UTILS_JS: &[u8] = include_bytes!("dist/assets/utils.js");

lazy_static::lazy_static! {
    static ref CHILD_PIDS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
}

fn update_child_pids() {
    let current_pid = unsafe { GetCurrentProcessId() };

    // Use wmic to get all processes (PID, PPID) - fast and standard
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = std::process::Command::new("wmic");
    cmd.args(&["process", "get", "ProcessId,ParentProcessId", "/format:csv"]);

    // CREATE_NO_WINDOW = 0x08000000 - prevents console window flash
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    let output = cmd.output();

    if let Ok(o) = output {
        if let Ok(s) = String::from_utf8(o.stdout) {
            let mut tree = std::collections::HashMap::new();

            // Parse CSV output
            for line in s.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let parts: Vec<&str> = line.split(',').collect();
                // Format is: Node, ParentProcessId, ProcessId (usually)
                // But wmic csv header is: Node,ParentProcessId,ProcessId
                if parts.len() >= 3 {
                    if let (Ok(ppid), Ok(pid)) = (
                        parts[1].trim().parse::<u32>(),
                        parts[2].trim().parse::<u32>(),
                    ) {
                        tree.entry(ppid).or_insert_with(Vec::new).push(pid);
                    }
                }
            }

            // Find all descendants recursively
            let mut descendants = Vec::new();
            let mut queue = vec![current_pid];
            let mut visited = std::collections::HashSet::new();
            visited.insert(current_pid);

            while let Some(pid) = queue.pop() {
                if let Some(children) = tree.get(&pid) {
                    for &child in children {
                        if visited.insert(child) {
                            descendants.push(child);
                            queue.push(child);
                        }
                    }
                }
            }

            if let Ok(mut lock) = CHILD_PIDS.lock() {
                *lock = descendants;
            }
        }
    }
}

unsafe fn set_app_volume(volume: f32) -> Result<()> {
    // Access cache
    let current_pid = GetCurrentProcessId();
    let child_pids = CHILD_PIDS.lock().unwrap_or_else(|e| e.into_inner()).clone();

    // We try to initialize COM, but ignore error if already initialized
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

    let device_enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    let device = device_enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
    let session_manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
    let session_enumerator = session_manager.GetSessionEnumerator()?;
    let count = session_enumerator.GetCount()?;

    for i in 0..count {
        if let Ok(session_control) = session_enumerator.GetSession(i) {
            if let Ok(session_control2) = session_control.cast::<IAudioSessionControl2>() {
                if let Ok(pid) = session_control2.GetProcessId() {
                    // Match Main Process OR known Children
                    if pid == current_pid || child_pids.contains(&pid) {
                        if let Ok(simple_volume) = session_control.cast::<ISimpleAudioVolume>() {
                            let _ = simple_volume.SetMasterVolume(volume, std::ptr::null());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

unsafe extern "system" fn pdj_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW => {
            // Update lang and theme if needed
            let (api_key, lang, theme_mode) = {
                let app = crate::APP.lock().unwrap();
                (
                    app.config.gemini_api_key.clone(),
                    app.config.ui_language.clone(),
                    app.config.theme_mode.clone(),
                )
            };

            let theme_str = match theme_mode {
                crate::config::ThemeMode::Dark => "dark",
                crate::config::ThemeMode::Light => "light",
                crate::config::ThemeMode::System => {
                    if crate::gui::utils::is_system_in_dark_mode() {
                        "dark"
                    } else {
                        "light"
                    }
                }
            };

            // Update window icon based on theme
            let is_dark = theme_str == "dark";
            crate::gui::utils::set_window_icon(hwnd, is_dark);

            PDJ_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let script = format!(
                        r#"
                        if (window.postMessage) {{
                            window.postMessage({{ type: 'pm-dj-set-api-key', apiKey: '{}', lang: '{}' }}, '*');
                            window.postMessage({{ type: 'pm-dj-set-theme', theme: '{}' }}, '*');
                        }}
                        "#,
                        api_key, lang, theme_str
                    );
                    let _ = webview.evaluate_script(&script);
                }
            });

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            LRESULT(0)
        }
        WM_APP_UPDATE_SETTINGS => {
            // Update lang and theme immediately even if hidden
            let (api_key, lang, theme_mode) = {
                let app = crate::APP.lock().unwrap();
                (
                    app.config.gemini_api_key.clone(),
                    app.config.ui_language.clone(),
                    app.config.theme_mode.clone(),
                )
            };

            let theme_str = match theme_mode {
                crate::config::ThemeMode::Dark => "dark",
                crate::config::ThemeMode::Light => "light",
                crate::config::ThemeMode::System => {
                    if crate::gui::utils::is_system_in_dark_mode() {
                        "dark"
                    } else {
                        "light"
                    }
                }
            };

            let is_dark = theme_str == "dark";
            crate::gui::utils::set_window_icon(hwnd, is_dark);

            PDJ_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let script = format!(
                        r#"
                        if (window.postMessage) {{
                            window.postMessage({{ type: 'pm-dj-set-api-key', apiKey: '{}', lang: '{}' }}, '*');
                            window.postMessage({{ type: 'pm-dj-set-theme', theme: '{}' }}, '*');
                        }}
                        "#,
                        api_key, lang, theme_str
                    );
                    let _ = webview.evaluate_script(&script);
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            PDJ_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let _ = webview
                        .evaluate_script("window.postMessage({ type: 'pm-dj-stop-audio' }, '*')");
                }
            });
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
            PDJ_WEBVIEW.with(|wv| {
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

pub fn warmup() {}

pub fn show_prompt_dj() {
    unsafe {
        // Initialize on-demand if not warmed up
        if !IS_WARMED_UP {
            if !IS_INITIALIZING {
                IS_INITIALIZING = true;
                std::thread::spawn(|| unsafe {
                    internal_create_pdj_loop();
                });
            }

            // Polling thread to auto-show once ready
            std::thread::spawn(|| {
                // Poll for 10 seconds (100 * 100ms)
                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    unsafe {
                        let hwnd_wrapper = std::ptr::addr_of!(PDJ_HWND).read();
                        if IS_WARMED_UP && !hwnd_wrapper.is_invalid() {
                            let _ = PostMessageW(
                                Some(hwnd_wrapper.0),
                                WM_APP_SHOW,
                                WPARAM(0),
                                LPARAM(0),
                            );
                            return;
                        }
                    }
                }
            });
            return;
        }

        let hwnd_wrapper = std::ptr::addr_of!(PDJ_HWND).read();
        if !hwnd_wrapper.is_invalid() {
            let _ = PostMessageW(Some(hwnd_wrapper.0), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn update_settings() {
    unsafe {
        if !std::ptr::addr_of!(PDJ_HWND).read().is_invalid() {
            let _ = PostMessageW(
                Some(PDJ_HWND.0),
                WM_APP_UPDATE_SETTINGS,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

unsafe fn internal_create_pdj_loop() {
    // 1. Create Window
    let instance = GetModuleHandleW(None).unwrap();
    let class_name = w!("PromptDJ_Class_Persistent");

    REGISTER_PDJ_CLASS.call_once(|| {
        let mut wc = WNDCLASSW::default();
        wc.lpfnWndProc = Some(pdj_wnd_proc);
        wc.hInstance = instance.into();
        wc.lpszClassName = class_name;
        wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
        wc.hbrBackground = HBRUSH(std::ptr::null_mut()); // Transparent background
        let _ = RegisterClassW(&wc);
    });

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);

    // Adaptive sizing based on screen aspect ratio:
    // - Width: Use 70% of screen width, capped between 1200 and 1600 pixels
    // - Height: Scales inversely with aspect ratio for consistent UI appearance
    //   - At 16:9 (1.78:1): ~72% of screen height → 775px on 1080p
    //   - At 21:9 (2.37:1): ~60% of screen height → 650px on 1080p ultrawide
    let aspect_ratio = screen_w as f64 / screen_h as f64;
    let base_aspect = 16.0 / 9.0; // 1.778
    let height_pct = (0.72 - (aspect_ratio - base_aspect) * 0.20).clamp(0.50, 0.80);

    let width = ((screen_w as f64 * 0.70) as i32).clamp(1200, 1600);
    let height = ((screen_h as f64 * height_pct) as i32).clamp(550, 900);
    let x = (screen_w - width) / 2;
    let y = (screen_h - height) / 2;

    let (api_key, lang, theme_mode) = {
        let app = crate::APP.lock().unwrap();
        (
            app.config.gemini_api_key.clone(),
            app.config.ui_language.clone(),
            app.config.theme_mode.clone(),
        )
    };

    let title_str = crate::gui::locale::LocaleText::get(&lang).prompt_dj_title;
    let title_wide = windows::core::HSTRING::from(title_str);

    let hwnd = CreateWindowExW(
        WS_EX_APPWINDOW,
        class_name,
        PCWSTR(title_wide.as_ptr()),
        WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_SYSMENU, // Start hidden (no WS_VISIBLE)
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

    PDJ_HWND = SendHwnd(hwnd);

    // Enable rounded corners
    let corner_pref = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner_pref as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&corner_pref) as u32,
    );

    // Set Window Icon
    let is_dark = match theme_mode {
        crate::config::ThemeMode::Dark => true,
        crate::config::ThemeMode::Light => false,
        crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
    };
    crate::gui::utils::set_window_icon(hwnd, is_dark);

    // 2. Create WebView
    let wrapper = HwndWrapper(hwnd);

    let theme_str = match theme_mode {
        crate::config::ThemeMode::Dark => "dark",
        crate::config::ThemeMode::Light => "light",
        crate::config::ThemeMode::System => "dark",
    };

    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    let init_script = format!(
        r#"
        // --- High-Priority Audio Hook ---
        (function() {{
            window._currentVolume = 1.0;
            window._activeMasterGains = [];
            
            const OriginalAC = window.AudioContext || window.webkitAudioContext;
            if (OriginalAC) {{
                const proto = OriginalAC.prototype;
                const desc = Object.getOwnPropertyDescriptor(proto, 'destination');
                if (desc && desc.get) {{
                    Object.defineProperty(proto, 'destination', {{
                        configurable: true,
                        enumerable: true,
                        get: function() {{
                            if (!this._masterGain) {{
                                const realDest = desc.get.call(this);
                                this._masterGain = this.createGain();
                                this._masterGain.gain.value = window._currentVolume;
                                this._masterGain.connect(realDest);
                                window._activeMasterGains.push(this._masterGain);
                            }}
                            return this._masterGain;
                        }}
                    }});
                }}
            }}
        }})();

        window.addEventListener('load', () => {{
            const style = document.createElement('style');
            style.innerHTML = `{}` + `
                body {{
                    margin: 0;
                    padding: 0;
                    font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif !important;
                    background-color: transparent !important;
                    overflow: hidden;
                }}
                #dj-drag-header {{
                    position: fixed;
                    top: 0;
                    left: 0;
                    width: 100%;
                    height: 32px;
                    background: transparent;
                    z-index: 2147483647;
                    -webkit-app-region: drag; 
                    cursor: grab;
                    pointer-events: auto;
                }}
                #dj-drag-header:active {{
                    cursor: grabbing;
                }}
                #dj-close-btn {{
                    position: absolute;
                    top: 0;
                    right: 0;
                    width: 40px;
                    height: 32px;
                    background: transparent;
                    color: rgba(255,255,255,0.5);
                    border: none;
                    font-family: 'Google Sans Flex', 'Segoe UI', system-ui;
                    font-size: 16px;
                    cursor: pointer;
                    display: flex;
                    align-items: center;
                    justify-content: center;
                    transition: background 0.2s, color 0.2s;
                    -webkit-app-region: no-drag;
                }}
                #dj-close-btn:hover {{
                    background: rgba(255,0,0,0.5);
                    color: white;
                }}
                #dj-min-btn {{
                    position: absolute;
                    top: 0;
                    right: 40px;
                    width: 40px;
                    height: 32px;
                    background: transparent;
                    color: rgba(255,255,255,0.5);
                    border: none;
                    font-family: 'Google Sans Flex', 'Segoe UI', system-ui;
                    font-size: 16px;
                    cursor: pointer;
                    display: flex;
                    align-items: center;
                    justify-content: center;
                    transition: background 0.2s, color 0.2s;
                    -webkit-app-region: no-drag;
                }}
                #dj-min-btn:hover {{
                    background: rgba(255,255,255,0.1);
                    color: white;
                }}
                /* Light theme: keep white text with dark shadow for visibility */
                [data-theme='light'] #dj-close-btn,
                [data-theme='light'] #dj-min-btn {{
                    color: rgba(255,255,255,0.9);
                    text-shadow: 0 1px 3px rgba(0,0,0,0.5), 0 0 6px rgba(0,0,0,0.3);
                }}
            `;
            document.head.appendChild(style);

            const header = document.createElement('div');
            header.id = 'dj-drag-header';
            
            const minBtn = document.createElement('button');
            minBtn.id = 'dj-min-btn';
            minBtn.innerHTML = '—';
            minBtn.onclick = (e) => {{
                e.stopPropagation(); 
                if (window.ipc) window.ipc.postMessage('minimize_window');
            }};
            header.appendChild(minBtn);

            const closeBtn = document.createElement('button');
            closeBtn.id = 'dj-close-btn';
            closeBtn.innerHTML = '✕';
            closeBtn.onclick = (e) => {{
                e.stopPropagation(); 
                window.postMessage({{ type: 'pm-dj-stop-audio' }}, '*');
                if (window.ipc) window.ipc.postMessage('close_window');
            }};
            header.appendChild(closeBtn);

            // --- Volume Slider Removed (moved to PromptDjMidi.ts) ---

            const updateTheme = (theme) => {{
                if (theme === 'light') {{
                    document.documentElement.setAttribute('data-theme', 'light');
                }} else {{
                    document.documentElement.setAttribute('data-theme', 'dark');
                }}
            }};

            window.addEventListener('message', (e) => {{
                if (e.data && e.data.type === 'pm-dj-set-theme') {{
                    updateTheme(e.data.theme);
                }}
            }});
            
            // Hover Logic (Removed Vol Container part)

            document.body.appendChild(header);

            setTimeout(() => {{
                window.postMessage({{ type: 'pm-dj-set-api-key', apiKey: '{}', lang: '{}' }}, '*');
                window.postMessage({{ type: 'pm-dj-set-theme', theme: '{}' }}, '*');
                window.postMessage({{ type: 'pm-dj-set-font', font: 'google-sans-flex' }}, '*');
            }}, 250);
        }});

        "#,
        font_css, api_key, lang, theme_str
    );

    let hwnd_ipc = hwnd;

    PDJ_WEB_CONTEXT.with(|ctx| {
        if ctx.borrow().is_none() {
            let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("common"));
            *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
        }
    });

    // Brief delay to ensure window is fully initialized before creating WebView
    std::thread::sleep(std::time::Duration::from_millis(100));

    let webview_result = {
        // LOCK SCOPE: Serialized build to prevent resource contention
        let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();
        crate::log_info!("[PromptDJ] Acquired init lock. Building...");

        let build_res = PDJ_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let mut builder = WebViewBuilder::new_with_web_context(ctx_ref.as_mut().unwrap())
                .with_custom_protocol("promptdj".to_string(), move |_id, request| {
                    let path = request.uri().path();
                    let (content, mime) = if path == "/" || path == "/index.html" {
                        (Cow::Borrowed(INDEX_HTML), "text/html")
                    } else if path.ends_with("index.js") {
                        (Cow::Borrowed(ASSET_INDEX_JS), "application/javascript")
                    } else if path.ends_with("index.css") {
                        (Cow::Borrowed(ASSET_INDEX_CSS), "text/css")
                    } else if path.ends_with("cubic.js") {
                        (Cow::Borrowed(ASSET_CUBIC_JS), "application/javascript")
                    } else if path.ends_with("morph-fixed.js") {
                        (Cow::Borrowed(ASSET_MORPH_JS), "application/javascript")
                    } else if path.ends_with("roundedPolygon.js") {
                        (Cow::Borrowed(ASSET_ROUNDED_JS), "application/javascript")
                    } else if path.ends_with("utils.js") {
                        (Cow::Borrowed(ASSET_UTILS_JS), "application/javascript")
                    } else {
                        return wnd_http_response(
                            404,
                            "text/plain",
                            Cow::Borrowed(b"Not Found".as_slice()),
                        );
                    };
                    wnd_http_response(200, mime, content)
                })
                .with_initialization_script(&init_script)
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    let body = msg.body().as_str();
                    if body == "drag_window" {
                        let _ = ReleaseCapture();
                        unsafe {
                            let _ = SendMessageW(
                                hwnd_ipc,
                                WM_NCLBUTTONDOWN,
                                Some(WPARAM(HTCAPTION as usize)),
                                Some(LPARAM(0)),
                            );
                        }
                    } else if body == "minimize_window" {
                        unsafe {
                            let _ = ShowWindow(hwnd_ipc, SW_MINIMIZE);
                        }
                    } else if body == "close_window" {
                        unsafe {
                            let _ = ShowWindow(hwnd_ipc, SW_HIDE);
                        }
                    } else if body.starts_with("set_volume:") {
                        if let Ok(val) = body.trim_start_matches("set_volume:").parse::<f32>() {
                            unsafe {
                                let _ = set_app_volume(val);
                            }
                        }
                    }
                })
                .with_url("promptdj://localhost/index.html");

            builder = crate::overlay::html_components::font_manager::configure_webview(builder);
            builder.build_as_child(&wrapper)
        });
        crate::log_info!(
            "[PromptDJ] Build finished. Status: {}",
            if build_res.is_ok() { "OK" } else { "ERR" }
        );
        build_res
    };

    let webview = match webview_result {
        Ok(wv) => wv,
        Err(e) => {
            eprintln!("Failed to create PromptDJ WebView: {:?}", e);
            // Clean up and exit gracefully
            let _ = DestroyWindow(hwnd);
            PDJ_HWND = SendHwnd::default();
            return;
        }
    };
    let webview_arc = Arc::new(webview);

    // Initial Resize
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    let _ = webview_arc.set_bounds(Rect {
        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
            (r.right - r.left) as u32,
            (r.bottom - r.top) as u32,
        )),
    });

    PDJ_WEBVIEW.with(|wv| {
        *wv.borrow_mut() = Some(webview_arc);
    });

    // Mark as warmed up and ready
    IS_WARMED_UP = true;

    // Spawn thread to cache child PIDs for volume control
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(2));
        update_child_pids();
    });

    // 3. Message Loop
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        let _ = DispatchMessageW(&msg);
    }

    PDJ_WEBVIEW.with(|wv| {
        *wv.borrow_mut() = None;
    });
    PDJ_HWND = SendHwnd::default();
    IS_WARMED_UP = false;
    IS_INITIALIZING = false;
}
