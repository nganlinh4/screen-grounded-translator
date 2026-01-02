use crate::APP;
use std::cell::RefCell;
use std::sync::{Mutex, Once};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebView, WebViewBuilder};

use crate::win_types::SendHwnd;

static REGISTER_BADGE_CLASS: Once = Once::new();
static mut BADGE_HWND: SendHwnd = SendHwnd(HWND(std::ptr::null_mut()));

// Messages
const WM_APP_SHOW_TEXT: u32 = WM_USER + 201;
const WM_APP_SHOW_IMAGE: u32 = WM_USER + 202;
const WM_APP_SHOW_NOTIFICATION: u32 = WM_USER + 203;

lazy_static::lazy_static! {
    static ref PENDING_CONTENT: Mutex<String> = Mutex::new(String::new());
}

thread_local! {
    static BADGE_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    static BADGE_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

// Dimensions
const BADGE_WIDTH: i32 = 1200; // Super wide
const BADGE_HEIGHT: i32 = 80; // Taller for nicer padding/shadows

/// Wrapper for HWND to implement HasWindowHandle
struct HwndWrapper(HWND);
unsafe impl Send for HwndWrapper {}
unsafe impl Sync for HwndWrapper {}

impl raw_window_handle::HasWindowHandle for HwndWrapper {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let raw = raw_window_handle::Win32WindowHandle::new(
            std::num::NonZeroIsize::new(self.0 .0 as isize).expect("HWND cannot be null"),
        );
        let handle = raw_window_handle::RawWindowHandle::Win32(raw);
        unsafe { Ok(raw_window_handle::WindowHandle::borrow_raw(handle)) }
    }
}

pub fn show_auto_copy_badge_text(text: &str) {
    *PENDING_CONTENT.lock().unwrap() = text.to_string();
    ensure_window_and_post(WM_APP_SHOW_TEXT);
}

pub fn show_auto_copy_badge_image() {
    ensure_window_and_post(WM_APP_SHOW_IMAGE);
}

/// Show a notification with just a title (no snippet/auto-copy text)
pub fn show_notification(title: &str) {
    *PENDING_CONTENT.lock().unwrap() = title.to_string();
    ensure_window_and_post(WM_APP_SHOW_NOTIFICATION);
}

fn ensure_window_and_post(msg: u32) {
    unsafe {
        if std::ptr::addr_of!(BADGE_HWND).read().is_invalid() {
            warmup();
            // Wait longer for WebView to initialize
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        let hwnd = std::ptr::addr_of!(BADGE_HWND).read().0;
        if !hwnd.is_invalid() {
            let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn warmup() {
    unsafe {
        if std::ptr::addr_of!(BADGE_HWND).read().is_invalid() {
            std::thread::spawn(|| {
                internal_create_window_loop();
            });
        }
    }
}

fn get_badge_html() -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
    {font_css}
    :root {{
        --bg-color: #1A3D2A;
        --border-color: #4ADE80; /* Brighter initial border */
        --text-prio-color: #ffffff;
        --text-sec-color: rgba(255, 255, 255, 0.9);
        --accent-color: #4ADE80;
        --bloom-color: rgba(74, 222, 128, 0.6); /* Strong glow */
        --shadow-color: rgba(0, 0, 0, 0.5);
    }}
    
    * {{ margin: 0; padding: 0; box-sizing: border-box; }}
    
    body {{
        overflow: hidden;
        background: transparent;
        font-family: 'Google Sans Flex', 'Segoe UI', sans-serif;
        display: flex;
        justify-content: center;
        align-items: center;
        height: 100vh;
        user-select: none;
        cursor: default;
    }}
    
    .badge {{
        min-width: 180px;
        max-width: 90%;
        width: auto;
        
        /* Glass / Dynamic Styling */
        background: var(--bg-color);
        /* Super thick border as requested */
        border: 2.5px solid var(--border-color);
        border-radius: 12px;
        
        /* Blooming / Glow Effect */
        box-shadow: 0 0 20px var(--bloom-color), 
                    0 8px 30px var(--shadow-color);
                    
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        
        display: flex;
        flex-direction: column;
        justify-content: center;
        align-items: center;
        
        opacity: 0;
        transform: translateY(20px) scale(0.92);
        
        transition: opacity 0.3s cubic-bezier(0.2, 0.8, 0.2, 1), 
                    transform 0.4s cubic-bezier(0.34, 1.56, 0.64, 1),
                    background 0.3s ease,
                    border-color 0.3s ease,
                    box-shadow 0.3s ease;
                    
        padding: 4px 18px;
        position: relative;
    }}
    
    .badge.visible {{
        opacity: 1;
        transform: translateY(0) scale(1);
    }}
    
    .row {{
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        line-height: normal;
        position: relative;
    }}
    
    .title-row {{
        margin-bottom: 0px;
    }}
    
    .title {{
        font-size: 15px;
        font-weight: 700;
        color: var(--text-prio-color);
        display: flex;
        align-items: center;
        gap: 8px;
        /* More stretch */
        letter-spacing: 1.2px; 
        text-transform: uppercase;
        
        font-variation-settings: 'wght' 700, 'wdth' 115, 'ROND' 100;
    }}
    
    .check {{
        color: var(--accent-color);
        font-weight: 800;
        font-size: 18px;
        display: flex;
        align-items: center;
        justify-content: center;
        animation: pop 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275) forwards;
        animation-delay: 0.1s;
        opacity: 0;
        transform: scale(0);
        /* Glow for checkmark too */
        filter: drop-shadow(0 0 5px var(--accent-color));
    }}
    
    @keyframes pop {{
        from {{ opacity: 0; transform: scale(0); }}
        to {{ opacity: 1; transform: scale(1); }}
    }}
    
    .snippet {{
        font-size: 13px;
        font-weight: 500;
        color: var(--text-sec-color);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 100%;
        text-align: center;
        padding-top: 1px;
        
        font-family: 'Google Sans Flex', 'Segoe UI', sans-serif;
        /* Condensed width (wdth < 100), keep slightly rounded (ROND 50) */
        font-variation-settings: 'wght' 500, 'wdth' 85, 'ROND' 50;
        letter-spacing: -0.3px;
    }}
    
    .snippet-container {{
        width: 100%;
        display: flex;
        justify-content: center;
        overflow: hidden;
    }}
</style>
</head>
<body>
    <div id="badge" class="badge">
        <div class="row title-row">
            <div class="title">
                <span class="check">
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="4.5" stroke-linecap="round" stroke-linejoin="round">
                        <polyline points="20 6 9 17 4 12"></polyline>
                    </svg>
                </span>
                <span id="title-text">Auto Copy</span>
            </div>
        </div>
        <div class="row snippet-container">
            <div id="snippet" class="snippet"></div>
        </div>
    </div>
    <script>
        let hideTimer;
        
        window.setTheme = (isDark) => {{
            const root = document.documentElement;
            if (isDark) {{
                // Dark Mode: Deep Neon Green
                root.style.setProperty('--bg-color', 'rgba(10, 24, 18, 0.95)');
                root.style.setProperty('--border-color', '#4ADE80'); // Bright Green Border
                root.style.setProperty('--text-prio-color', '#ffffff');
                root.style.setProperty('--text-sec-color', 'rgba(255, 255, 255, 0.9)');
                root.style.setProperty('--accent-color', '#4ADE80');
                root.style.setProperty('--bloom-color', 'rgba(74, 222, 128, 0.5)'); // Strong bloom
                root.style.setProperty('--shadow-color', 'rgba(0, 0, 0, 0.6)');
            }} else {{
                // Light Mode: Bright Neon (White + Green Glow)
                root.style.setProperty('--bg-color', 'rgba(255, 255, 255, 0.95)');
                root.style.setProperty('--border-color', '#16a34a'); // Solid Green Border
                root.style.setProperty('--text-prio-color', '#1a1a1a');
                root.style.setProperty('--text-sec-color', '#333333');
                root.style.setProperty('--accent-color', '#16a34a'); 
                root.style.setProperty('--bloom-color', 'rgba(22, 163, 74, 0.3)'); // Subtler bloom
                root.style.setProperty('--shadow-color', 'rgba(0, 0, 0, 0.2)');
            }}
        }};

        window.show = (title, snippet) => {{
            document.getElementById('title-text').innerText = title;
            document.getElementById('snippet').innerText = snippet;
            const b = document.getElementById('badge');
            const check = document.querySelector('.check');
            const snippetContainer = document.querySelector('.snippet-container');
            
            // Hide checkmark and snippet for notifications (empty snippet)
            if (!snippet) {{
                check.style.display = 'none';
                snippetContainer.style.display = 'none';
            }} else {{
                check.style.display = 'flex';
                snippetContainer.style.display = 'flex';
            }}
            
            // Force reflow to restart animation
            b.classList.remove('visible');
            b.offsetHeight; // trigger reflow
            
            // Re-trigger check animation if visible
            if (snippet) {{
                check.style.animation = 'none';
                check.offsetHeight; /* trigger reflow */
                check.style.animation = null; 
            }}
            
            b.classList.add('visible');
            
            clearTimeout(hideTimer);
            hideTimer = setTimeout(() => {{
                b.classList.remove('visible');
                // Tell Rust to hide window after fade out
                setTimeout(() => window.ipc.postMessage('finished'), 400);
            }}, 1000); 
        }};
    </script>
</body>
</html>"#
    )
}

fn internal_create_window_loop() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_AutoCopyBadgeWebView");

        REGISTER_BADGE_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(badge_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(std::ptr::null_mut());
            let _ = RegisterClassW(&wc);
        });

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT AutoCopy Badge"),
            WS_POPUP,
            -4000,
            -4000,
            BADGE_WIDTH,
            BADGE_HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        BADGE_HWND = SendHwnd(hwnd);

        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        let wrapper = HwndWrapper(hwnd);

        BADGE_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        let webview = BADGE_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                WebViewBuilder::new_with_web_context(web_ctx)
            } else {
                WebViewBuilder::new()
            };

            let builder = crate::overlay::html_components::font_manager::configure_webview(builder);

            builder
                .with_transparent(true)
                .with_bounds(Rect {
                    position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        BADGE_WIDTH as u32,
                        BADGE_HEIGHT as u32,
                    )),
                })
                .with_html(&get_badge_html())
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    let body = msg.body();
                    if body == "finished" {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                })
                .build(&wrapper)
        });

        if let Ok(wv) = webview {
            BADGE_WEBVIEW.with(|cell| {
                *cell.borrow_mut() = Some(wv);
            });
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        BADGE_HWND = SendHwnd(HWND(std::ptr::null_mut()));
    }
}

unsafe extern "system" fn badge_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW_TEXT | WM_APP_SHOW_IMAGE => {
            let app = APP.lock().unwrap();
            let ui_lang = app.config.ui_language.clone();
            // Determin theme
            let is_dark = match app.config.theme_mode {
                crate::config::ThemeMode::Dark => true,
                crate::config::ThemeMode::Light => false,
                crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
            };

            let locale = crate::gui::locale::LocaleText::get(&ui_lang);
            let title = locale.auto_copied_badge;

            let snippet = if msg == WM_APP_SHOW_TEXT {
                let text = PENDING_CONTENT.lock().unwrap().clone();
                let clean_text = text.replace('\n', " ").replace('\r', "");
                format!("\"{}\"", clean_text)
            } else {
                locale.auto_copied_image_badge.to_string()
            };

            drop(app);

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let x = (screen_w - BADGE_WIDTH) / 2;
            let y = screen_h - BADGE_HEIGHT - 100;

            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                BADGE_WIDTH,
                BADGE_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            BADGE_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    // 1. Update theme
                    let theme_script = format!("window.setTheme({});", is_dark);
                    let _ = webview.evaluate_script(&theme_script);

                    // 2. Show content
                    let safe_title = title
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\'', "\\'");
                    let safe_snippet = snippet
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\'', "\\'");

                    let script = format!("window.show('{}', '{}');", safe_title, safe_snippet);
                    let _ = webview.evaluate_script(&script);
                }
            });

            LRESULT(0)
        }
        WM_APP_SHOW_NOTIFICATION => {
            let app = APP.lock().unwrap();
            let is_dark = match app.config.theme_mode {
                crate::config::ThemeMode::Dark => true,
                crate::config::ThemeMode::Light => false,
                crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
            };
            drop(app);

            let title = PENDING_CONTENT.lock().unwrap().clone();

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let x = (screen_w - BADGE_WIDTH) / 2;
            let y = screen_h - BADGE_HEIGHT - 100;

            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                BADGE_WIDTH,
                BADGE_HEIGHT,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            BADGE_WEBVIEW.with(|wv| {
                if let Some(webview) = wv.borrow().as_ref() {
                    let theme_script = format!("window.setTheme({});", is_dark);
                    let _ = webview.evaluate_script(&theme_script);

                    let safe_title = title
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\'', "\\'");

                    let script = format!("window.show('{}', '');", safe_title);
                    let _ = webview.evaluate_script(&script);
                }
            });

            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
