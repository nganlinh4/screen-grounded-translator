//! App selection popup for per-app audio capture

use super::state::*;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::*;

lazy_static::lazy_static! {
    /// Cache for app icons (PID -> base64 PNG)
    static ref ICON_CACHE: Mutex<HashMap<u32, Option<String>>> = Mutex::new(HashMap::new());
}

/// Get the executable path for a given process ID
fn get_process_exe_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;

        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );

        let _ = windows::Win32::Foundation::CloseHandle(handle);

        if result.is_ok() && size > 0 {
            Some(String::from_utf16_lossy(&buffer[..size as usize]))
        } else {
            None
        }
    }
}

/// Extract icon from an executable and convert to base64 PNG
fn extract_icon_as_base64(exe_path: &str) -> Option<String> {
    unsafe {
        // Convert path to wide string
        let wide_path: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();

        // Extract large icon (32x32)
        let mut large_icon = HICON::default();
        let count = ExtractIconExW(
            windows::core::PCWSTR(wide_path.as_ptr()),
            0,
            Some(&mut large_icon),
            None,
            1,
        );

        if count == 0 || large_icon.is_invalid() {
            return None;
        }

        // Get icon info to access the bitmap
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(large_icon, &mut icon_info).is_err() {
            let _ = DestroyIcon(large_icon);
            return None;
        }

        // Get bitmap info
        let mut bmp = BITMAP::default();
        if GetObjectW(
            icon_info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bmp as *mut _ as *mut std::ffi::c_void),
        ) == 0
        {
            let _ = DeleteObject(icon_info.hbmMask.into());
            let _ = DeleteObject(icon_info.hbmColor.into());
            let _ = DestroyIcon(large_icon);
            return None;
        }

        let width = bmp.bmWidth as u32;
        let height = bmp.bmHeight as u32;

        // Create DC and get bitmap bits
        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));

        // Setup BITMAPINFO for 32-bit BGRA
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // Top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        // Allocate buffer for pixel data
        let mut pixels = vec![0u8; (width * height * 4) as usize];

        // Get the bits from the color bitmap
        let lines = GetDIBits(
            hdc_mem,
            icon_info.hbmColor,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
            &bmi as *const _ as *mut _,
            DIB_RGB_COLORS,
        );

        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
        let _ = DeleteObject(icon_info.hbmMask.into());
        let _ = DeleteObject(icon_info.hbmColor.into());
        let _ = DestroyIcon(large_icon);

        if lines == 0 {
            return None;
        }

        // Convert BGRA to RGBA and check for alpha
        let mut has_alpha = false;
        for i in (0..pixels.len()).step_by(4) {
            // Swap B and R (BGRA -> RGBA)
            pixels.swap(i, i + 2);
            if pixels[i + 3] != 0 {
                has_alpha = true;
            }
        }

        // If no alpha channel, set all alpha to 255
        if !has_alpha {
            for i in (3..pixels.len()).step_by(4) {
                pixels[i] = 255;
            }
        }

        // Encode as PNG using image crate
        let rgba_image = match image::RgbaImage::from_raw(width, height, pixels) {
            Some(img) => img,
            None => return None,
        };

        let mut png_data: Vec<u8> = Vec::new();
        if rgba_image
            .write_to(
                &mut std::io::Cursor::new(&mut png_data),
                image::ImageFormat::Png,
            )
            .is_err()
        {
            return None;
        }

        // Encode to base64
        use base64::Engine;
        Some(base64::engine::general_purpose::STANDARD.encode(&png_data))
    }
}

/// Get icon for a process, using cache
fn get_app_icon(pid: u32) -> Option<String> {
    // Check cache first
    {
        let cache = ICON_CACHE.lock().ok()?;
        if let Some(cached) = cache.get(&pid) {
            return cached.clone();
        }
    }

    // Extract icon
    let icon = get_process_exe_path(pid).and_then(|path| extract_icon_as_base64(&path));

    // Cache result
    if let Ok(mut cache) = ICON_CACHE.lock() {
        cache.insert(pid, icon.clone());
    }

    icon
}
/// Enumerate visible windows with titles for app selection
/// Returns a list of (PID, Window Title) for apps that likely emit audio
pub fn enumerate_audio_apps() -> Vec<(u32, String)> {
    let mut apps: Vec<(u32, String)> = Vec::new();
    let mut seen_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    unsafe {
        // Callback to collect window info
        let mut callback_data = (&mut apps, &mut seen_pids);

        extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> windows_core::BOOL {
            unsafe {
                // Skip invisible windows
                if !IsWindowVisible(hwnd).as_bool() {
                    return windows_core::BOOL(1);
                }

                // Get window title
                let mut title_buf = [0u16; 256];
                let len = GetWindowTextW(hwnd, &mut title_buf);
                if len == 0 {
                    return windows_core::BOOL(1);
                }

                let title = String::from_utf16_lossy(&title_buf[..len as usize]);

                // Skip empty/system windows
                if title.is_empty() || title == "Program Manager" || title == "Settings" {
                    return windows_core::BOOL(1);
                }

                // Get process ID
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));

                if pid == 0 {
                    return windows_core::BOOL(1);
                }

                // Get callback data from lparam
                let data = &mut *(lparam.0
                    as *mut (&mut Vec<(u32, String)>, &mut std::collections::HashSet<u32>));
                let (apps, seen_pids) = data;

                // Skip if we've already seen this PID (one entry per app)
                if seen_pids.contains(&pid) {
                    return windows_core::BOOL(1);
                }
                seen_pids.insert(pid);

                // Skip our own process
                let our_pid = std::process::id();
                if pid == our_pid {
                    return windows_core::BOOL(1);
                }

                apps.push((pid, title));

                windows_core::BOOL(1)
            }
        }

        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut callback_data as *mut _ as isize),
        );
    }

    // Sort by title for better UX
    apps.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

    apps
}

/// Show a popup window for selecting which app to capture audio from
/// This is called when TTS is enabled in device mode
pub fn show_app_selection_popup() {
    use crate::gui::locale::LocaleText;
    use crate::APP;
    use std::sync::atomic::Ordering;
    use windows::core::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    // Get locale text
    let locale_text = {
        let app = APP.lock().unwrap();
        let lang = app.config.ui_language.clone();
        LocaleText::get(&lang)
    };

    // Get apps list
    let apps = enumerate_audio_apps();
    if apps.is_empty() {
        eprintln!("No audio apps found for selection");
        return;
    }

    // Build HTML for app list
    let app_items: Vec<String> = apps
        .iter()
        .map(|(pid, name)| {
            let escaped_name = name
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            // Truncate by characters, not bytes (for Unicode safety)
            let short_name = if escaped_name.chars().count() > 50 {
                let truncated: String = escaped_name.chars().take(47).collect();
                format!("{}...", truncated)
            } else {
                escaped_name.clone()
            };

            // Get real app icon or use fallback
            let icon_html = if let Some(base64_icon) = get_app_icon(*pid) {
                format!(
                    r#"<img class="app-icon-img" src="data:image/png;base64,{}" alt=""/>"#,
                    base64_icon
                )
            } else {
                r#"<span class="material-symbols-rounded app-icon-fallback">music_note</span>"#
                    .to_string()
            };

            format!(
                r#"<div class="app-item" data-pid="{}" onclick="selectApp({}, '{}')">
                    <div class="app-icon">{}</div>
                    <div class="app-info">
                        <span class="app-name" title="{}">{}</span>
                        <span class="app-pid">PID: {}</span>
                    </div>
                </div>"#,
                pid,
                pid,
                escaped_name.replace('\'', "\\'"),
                icon_html,
                escaped_name,
                short_name,
                pid
            )
        })
        .collect();

    let html = format!(
        r##"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@24,400,1,0&display=swap" />
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:opsz,slnt,wdth,wght,ROND@6..144,-10..0,25..151,100..1000,100&display=swap" />
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
            background: rgba(20, 20, 30, 0.98);
            color: #fff;
            padding: 20px;
            height: 100vh;
            overflow: hidden;
        }}
        /* Loading overlay - covers content until fonts load, then fades out */
        #loading-overlay {{
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgb(20, 20, 30);
            z-index: 9999;
            pointer-events: none;
            display: flex;
            justify-content: center;
            align-items: center;
            animation: fadeOut 0.4s ease-out 0.9s forwards;
        }}
        .loading-svg {{
            width: 72px;
            height: 72px;
            filter: drop-shadow(0 0 12px #00c8ff90);
            animation: breathe 2.5s ease-in-out infinite;
        }}
        @keyframes breathe {{
            0%, 100% {{ 
                transform: scale(1); 
                opacity: 0.85;
                filter: drop-shadow(0 0 8px #00c8ff60);
            }}
            50% {{ 
                transform: scale(1.08); 
                opacity: 1;
                filter: drop-shadow(0 0 20px #00c8ff);
            }}
        }}
        @keyframes fadeOut {{
            from {{ opacity: 1; }}
            to {{ opacity: 0; }}
        }}
        .material-symbols-rounded {{
            font-variation-settings: 'FILL' 1, 'wght' 400, 'GRAD' 0, 'opsz' 24;
        }}
        h1 {{
            font-size: 18px;
            font-weight: 500;
            margin-bottom: 8px;
            color: #fff;
            display: flex;
            align-items: center;
            gap: 8px;
        }}
        h1 .material-symbols-rounded {{
            font-size: 22px;
            color: #00c8ff;
        }}
        .hint {{
            font-size: 12px;
            color: #888;
            margin-bottom: 16px;
        }}
        .app-list {{
            display: flex;
            flex-direction: column;
            gap: 8px;
            max-height: calc(100vh - 100px);
            overflow-y: auto;
        }}
        .app-item {{
            display: flex;
            align-items: center;
            gap: 12px;
            padding: 12px 16px;
            background: rgba(255, 255, 255, 0.05);
            border-radius: 8px;
            cursor: pointer;
            transition: all 0.15s ease;
            border: 1px solid transparent;
        }}
        .app-item:hover {{
            background: rgba(255, 255, 255, 0.1);
            border-color: rgba(100, 180, 255, 0.5);
        }}
        .app-icon {{
            width: 40px;
            height: 40px;
            display: flex;
            align-items: center;
            justify-content: center;
            background: rgba(100, 180, 255, 0.15);
            border-radius: 8px;
            flex-shrink: 0;
            overflow: hidden;
        }}
        .app-icon-img {{
            width: 32px;
            height: 32px;
            object-fit: contain;
            image-rendering: auto;
        }}
        .app-icon-fallback {{
            font-size: 24px;
            color: #00c8ff;
        }}
        .app-info {{
            flex: 1;
            min-width: 0;
        }}
        .app-name {{
            display: block;
            font-size: 14px;
            font-weight: 500;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        .app-pid {{
            font-size: 11px;
            color: #888;
        }}
        .app-list::-webkit-scrollbar {{
            width: 6px;
        }}
        .app-list::-webkit-scrollbar-track {{
            background: transparent;
        }}
        .app-list::-webkit-scrollbar-thumb {{
            background: rgba(255, 255, 255, 0.2);
            border-radius: 3px;
        }}
    </style>
</head>
<body>
    <div id="loading-overlay">
        <svg class="loading-svg" viewBox="0 0 24 24" fill="none" stroke="#00c8ff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"></path>
        </svg>
    </div>
    <h1><span class="material-symbols-rounded">headphones</span> {app_title}</h1>
    <p class="hint">{app_hint}</p>
    <div class="app-list">
        {app_list}
    </div>
    <script>
        function selectApp(pid, name) {{
            window.ipc.postMessage('selectApp:' + pid + ':' + name);
        }}
    </script>
</body>
</html>"##,
        app_title = locale_text.app_select_title,
        app_hint = locale_text.app_select_hint,
        app_list = app_items.join("\n")
    );

    // Create popup window
    std::thread::spawn(move || {
        unsafe {
            use windows::Win32::Graphics::Dwm::{
                DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            };
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, WS_CLIPCHILDREN};

            // Register window class
            let class_name = w!("AppSelectPopup");
            let h_instance = GetModuleHandleW(None).unwrap_or_default();

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(app_select_wndproc),
                hInstance: h_instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassExW(&wc);

            // Center the window on screen
            let screen_width = GetSystemMetrics(SM_CXSCREEN);
            let screen_height = GetSystemMetrics(SM_CYSCREEN);
            let win_width = 400;
            let win_height = 500;
            let x = (screen_width - win_width) / 2;
            let y = (screen_height - win_height) / 2;

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name,
                w!("Select App"),
                WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN,
                x,
                y,
                win_width,
                win_height,
                None,
                None,
                Some(h_instance.into()),
                None,
            )
            .unwrap();

            // Store handle for external closing
            APP_SELECTION_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

            // Apply rounded corners
            let preference = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );

            // Create WebView2
            let html_clone = html.clone();
            let hwnd_val = hwnd.0 as isize;

            let result = wry::WebViewBuilder::new()
                .with_bounds(wry::Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        win_width as u32,
                        win_height as u32,
                    )),
                })
                .with_html(&html_clone)
                .with_transparent(true)
                .with_ipc_handler(move |req| {
                    let body = req.body();
                    if body.starts_with("selectApp:") {
                        let rest = &body[10..];
                        if let Some((pid_str, name)) = rest.split_once(':') {
                            if let Ok(pid) = pid_str.parse::<u32>() {
                                // Store selected app
                                SELECTED_APP_PID.store(pid, Ordering::SeqCst);
                                if let Ok(mut app_name) = SELECTED_APP_NAME.lock() {
                                    *app_name = name.to_string();
                                }

                                // Set audio source to trigger restart (must set this for restart to work!)
                                if let Ok(mut new_source) = NEW_AUDIO_SOURCE.lock() {
                                    *new_source = "device".to_string();
                                }
                                AUDIO_SOURCE_CHANGE.store(true, Ordering::SeqCst);

                                let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
                                // Close native popup
                                let _ = ShowWindow(hwnd, SW_HIDE);
                                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));

                                // Close TTS Modal in translation window (if exists)
                                if !std::ptr::addr_of!(TRANSLATION_HWND).read().is_invalid() {
                                    let _ = PostMessageW(
                                        Some(TRANSLATION_HWND),
                                        WM_CLOSE_TTS_MODAL,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                }
                            }
                        }
                    }
                })
                .build_as_child(&HwndWrapper(hwnd));

            if result.is_err() {
                eprintln!("Failed to create WebView for app selection");
                let _ = DestroyWindow(hwnd);
                return;
            }

            // Keep WebView alive
            let _webview = result.unwrap();

            // Message loop
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

pub unsafe extern "system" fn app_select_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::*;

    match msg {
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            APP_SELECTION_HWND.store(0, std::sync::atomic::Ordering::SeqCst);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_SIZE => {
            // Resize child (WebView) to match parent
            let width = (lparam.0 & 0xFFFF) as i32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as i32;
            if let Ok(child) = GetWindow(hwnd, GW_CHILD) {
                if child.0 != std::ptr::null_mut() {
                    let _ = MoveWindow(child, 0, 0, width, height, true);
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
