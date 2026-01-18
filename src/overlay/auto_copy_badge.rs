use crate::APP;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, Once};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitialize, CoUninitialize};
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebView, WebViewBuilder};

static REGISTER_BADGE_CLASS: Once = Once::new();

// Thread-safe handle using atomic (like preset_wheel)
static BADGE_HWND: AtomicIsize = AtomicIsize::new(0);
static IS_WARMING_UP: AtomicBool = AtomicBool::new(false);
static IS_WARMED_UP: AtomicBool = AtomicBool::new(false);

// Messages
const WM_APP_PROCESS_QUEUE: u32 = WM_USER + 201;

/// Notification themes
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotificationType {
    Success, // Green - auto copied
    Info,    // Yellow - loading/warming up
    Update,  // Blue - update available (longer duration)
    Error,   // Red - error (e.g., no writable area for auto-paste)
}

#[derive(Clone, Debug)]
pub struct PendingNotification {
    pub title: String,
    pub snippet: String,
    pub n_type: NotificationType,
}

lazy_static::lazy_static! {
    static ref PENDING_QUEUE: Mutex<VecDeque<PendingNotification>> = Mutex::new(VecDeque::new());
}

thread_local! {
    static BADGE_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    static BADGE_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

// Dimensions
const BADGE_WIDTH: i32 = 1200; // Super wide
const BADGE_HEIGHT: i32 = 400; // Taller for stacking

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

fn enqueue_notification(title: String, snippet: String, n_type: NotificationType) {
    crate::log_info!("[Badge] Enqueuing: '{}' ({:?})", title, n_type);
    {
        let mut q = PENDING_QUEUE.lock().unwrap();
        q.push_back(PendingNotification {
            title,
            snippet,
            n_type,
        });
    }
    ensure_window_and_post(WM_APP_PROCESS_QUEUE);
}

pub fn show_auto_copy_badge_text(text: &str) {
    let app = APP.lock().unwrap();
    let ui_lang = app.config.ui_language.clone();
    let locale = crate::gui::locale::LocaleText::get(&ui_lang);
    let title = locale.auto_copied_badge.to_string();
    drop(app);

    let clean_text = text.replace('\n', " ").replace('\r', "");
    let snippet = format!("\"{}\"", clean_text);

    enqueue_notification(title, snippet, NotificationType::Success);
}

pub fn show_auto_copy_badge_image() {
    let app = APP.lock().unwrap();
    let ui_lang = app.config.ui_language.clone();
    let locale = crate::gui::locale::LocaleText::get(&ui_lang);
    let title = locale.auto_copied_badge.to_string();
    let snippet = locale.auto_copied_image_badge.to_string();
    drop(app);

    enqueue_notification(title, snippet, NotificationType::Success);
}

/// Show a loading/info notification with just a title (yellow theme)
pub fn show_notification(title: &str) {
    enqueue_notification(title.to_string(), String::new(), NotificationType::Info);
}

/// Show an update available notification (blue theme, longer duration)
pub fn show_update_notification(title: &str) {
    enqueue_notification(title.to_string(), String::new(), NotificationType::Update);
}

/// Show an error notification (red theme)
pub fn show_error_notification(title: &str) {
    enqueue_notification(title.to_string(), String::new(), NotificationType::Error);
}

fn ensure_window_and_post(msg: u32) {
    // Check if already warmed up
    if !IS_WARMED_UP.load(Ordering::SeqCst) {
        crate::log_info!("[Badge] Not warmed up, triggering warmup...");
        // Trigger warmup if not started yet
        warmup();

        // Poll for ready state (up to 20 seconds to allow for serialized startup)
        for i in 0..400 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if IS_WARMED_UP.load(Ordering::SeqCst) {
                crate::log_info!("[Badge] Warmup completed after {} ms", i * 50);
                break;
            }
        }

        // If still not ready, give up this notification
        if !IS_WARMED_UP.load(Ordering::SeqCst) {
            crate::log_info!("[Badge] Warmup TIMED OUT! Notification dropped.");
            return;
        }
    }

    let hwnd_val = BADGE_HWND.load(Ordering::SeqCst);
    let hwnd = HWND(hwnd_val as *mut _);
    if hwnd_val != 0 && !hwnd.is_invalid() {
        unsafe {
            let res = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
            println!("[Badge] PostMessage Result: {:?}", res);
        }
    } else {
        println!("[Badge] Invalid HWND: {:?}", hwnd);
    }
}

pub fn warmup() {
    // Prevent multiple warmup threads from spawning (like preset_wheel)
    if IS_WARMING_UP
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        internal_create_window_loop();
    });
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
        /* Defaults just in case */
        --this-bg: #1A3D2A;
        --this-border: #4ADE80;
        --this-text-prio: #ffffff;
        --this-text-sec: rgba(255, 255, 255, 0.9);
        --this-accent: #4ADE80;
        --this-bloom: rgba(74, 222, 128, 0.6);
        --this-shadow: rgba(0, 0, 0, 0.5);
    }}
    
    * {{ margin: 0; padding: 0; box-sizing: border-box; }}
    
    body {{
        overflow: hidden;
        background: transparent;
        font-family: 'Google Sans Flex', 'Segoe UI', sans-serif;
        display: flex;
        flex-direction: column;
        justify-content: flex-end; /* Align bottom */
        align-items: center;
        height: 100vh;
        user-select: none;
        cursor: default;
        padding-bottom: 20px;
    }}
    
    #notifications {{
        display: flex;
        flex-direction: column;
        width: 100%;
        align-items: center;
        gap: 10px;
    }}

    .badge {{
        min-width: 180px;
        max-width: 90%;
        width: auto;
        
        background: var(--this-bg);
        border: 2.5px solid var(--this-border);
        border-radius: 12px;
        
        box-shadow: 0 0 12px var(--this-bloom), 
                    0 4px 15px var(--this-shadow);
                    
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        
        display: flex;
        flex-direction: column;
        justify-content: center;
        align-items: center;
        
        padding: 4px 18px;
        position: relative;
        
        opacity: 0;
        transform: translateY(20px) scale(0.92);
        transition: all 0.4s cubic-bezier(0.2, 0.8, 0.2, 1);
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
    
    .title-row {{ margin-bottom: 0px; }}
    
    .title {{
        font-size: 15px;
        font-weight: 700;
        color: var(--this-text-prio);
        display: flex;
        align-items: center;
        gap: 8px;
        letter-spacing: 1.2px; 
        text-transform: uppercase;
        font-variation-settings: 'wght' 700, 'wdth' 115, 'ROND' 100;
    }}
    
    .check {{
        color: var(--this-accent);
        font-weight: 800;
        font-size: 18px;
        display: flex;
        align-items: center;
        justify-content: center;
        animation: pop 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275) forwards;
        animation-delay: 0.1s;
        opacity: 0;
        transform: scale(0);
        filter: drop-shadow(0 0 5px var(--this-accent));
    }}
    
    @keyframes pop {{
        from {{ opacity: 0; transform: scale(0); }}
        to {{ opacity: 1; transform: scale(1); }}
    }}
    
    .snippet {{
        font-size: 13px;
        font-weight: 500;
        color: var(--this-text-sec);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 100%;
        text-align: center;
        padding-top: 1px;
        font-family: 'Google Sans Flex', 'Segoe UI', sans-serif;
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
    <div id="notifications"></div>
    <script>
        // JS Error Handler for Debugging
        window.onerror = function(msg, source, line, col, error) {{
            window.ipc.postMessage('error: ' + msg + ' @ ' + line);
        }};

        // Theme definitions (Copied from original)
        const themes = {{
            success: {{
                dark: {{
                    bg: 'rgba(10, 24, 18, 0.95)',
                    border: '#4ADE80',
                    textPrio: '#ffffff',
                    textSec: 'rgba(255, 255, 255, 0.9)',
                    accent: '#4ADE80',
                    bloom: 'rgba(74, 222, 128, 0.5)',
                    shadow: 'rgba(0, 0, 0, 0.6)'
                }},
                light: {{
                    bg: 'rgba(255, 255, 255, 0.95)',
                    border: '#16a34a',
                    textPrio: '#1a1a1a',
                    textSec: '#333333',
                    accent: '#16a34a',
                    bloom: 'rgba(22, 163, 74, 0.3)',
                    shadow: 'rgba(0, 0, 0, 0.2)'
                }},
                duration: 1000
            }},
            info: {{
                dark: {{
                    bg: 'rgba(30, 25, 10, 0.95)',
                    border: '#FACC15',
                    textPrio: '#ffffff',
                    textSec: 'rgba(255, 255, 255, 0.9)',
                    accent: '#FACC15',
                    bloom: 'rgba(250, 204, 21, 0.5)',
                    shadow: 'rgba(0, 0, 0, 0.6)'
                }},
                light: {{
                    bg: 'rgba(255, 251, 235, 0.95)',
                    border: '#CA8A04',
                    textPrio: '#1a1a1a',
                    textSec: '#333333',
                    accent: '#CA8A04',
                    bloom: 'rgba(202, 138, 4, 0.3)',
                    shadow: 'rgba(0, 0, 0, 0.2)'
                }},
                duration: 1500
            }},
            update: {{
                dark: {{
                    bg: 'rgba(10, 18, 30, 0.95)',
                    border: '#60A5FA',
                    textPrio: '#ffffff',
                    textSec: 'rgba(255, 255, 255, 0.9)',
                    accent: '#60A5FA',
                    bloom: 'rgba(96, 165, 250, 0.5)',
                    shadow: 'rgba(0, 0, 0, 0.6)'
                }},
                light: {{
                    bg: 'rgba(239, 246, 255, 0.95)',
                    border: '#2563EB',
                    textPrio: '#1a1a1a',
                    textSec: '#333333',
                    accent: '#2563EB',
                    bloom: 'rgba(37, 99, 235, 0.3)',
                    shadow: 'rgba(0, 0, 0, 0.2)'
                }},
                duration: 5000
            }},
            error: {{
                dark: {{
                    bg: 'rgba(30, 10, 10, 0.95)',
                    border: '#F87171',
                    textPrio: '#ffffff',
                    textSec: 'rgba(255, 255, 255, 0.9)',
                    accent: '#F87171',
                    bloom: 'rgba(248, 113, 113, 0.5)',
                    shadow: 'rgba(0, 0, 0, 0.6)'
                }},
                light: {{
                    bg: 'rgba(254, 242, 242, 0.95)',
                    border: '#DC2626',
                    textPrio: '#1a1a1a',
                    textSec: '#333333',
                    accent: '#DC2626',
                    bloom: 'rgba(220, 38, 38, 0.3)',
                    shadow: 'rgba(0, 0, 0, 0.2)'
                }},
                duration: 2500
            }}
        }};
        
        let isDarkMode = false;
        
        window.setTheme = (isDark) => {{
            isDarkMode = isDark;
        }};
        
        function getColors(type, isDark) {{
            const t = themes[type] || themes.success;
            return isDark ? t.dark : t.light;
        }}
        
        window.addNotification = (title, snippet, type) => {{
            const container = document.getElementById('notifications');
            const colors = getColors(type, isDarkMode);
            const duration = (themes[type] || themes.success).duration;
            
            const badge = document.createElement('div');
            badge.className = 'badge';
            
            // Set styles
            const s = badge.style;
            s.setProperty('--this-bg', colors.bg);
            s.setProperty('--this-border', colors.border);
            s.setProperty('--this-text-prio', colors.textPrio);
            s.setProperty('--this-text-sec', colors.textSec);
            s.setProperty('--this-accent', colors.accent);
            s.setProperty('--this-bloom', colors.bloom);
            s.setProperty('--this-shadow', colors.shadow);
            
            const hasSnippet = (snippet && snippet.length > 0);
            const checkDisplay = hasSnippet ? 'flex' : 'none';
            const snippetDisplay = hasSnippet ? 'flex' : 'none';
            
            badge.innerHTML = `
                <div class="row title-row">
                    <div class="title">
                        <span class="check" style="display: ${{checkDisplay}}">
                            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="4.5" stroke-linecap="round" stroke-linejoin="round">
                                <polyline points="20 6 9 17 4 12"></polyline>
                            </svg>
                        </span>
                        <span>${{title}}</span>
                    </div>
                </div>
                <div class="row snippet-container" style="display: ${{snippetDisplay}}">
                    <div class="snippet">${{snippet}}</div>
                </div>
            `;
            
            container.appendChild(badge);
            
            // Animate In
            // Double raf to ensure transition
            requestAnimationFrame(() => {{
                requestAnimationFrame(() => {{
                   badge.classList.add('visible'); 
                }});
            }});
            
            // Remove logic
            setTimeout(() => {{
                badge.classList.remove('visible');
                setTimeout(() => {{
                    if (badge.parentNode) badge.parentNode.removeChild(badge);
                    if (container.children.length === 0) {{
                        window.ipc.postMessage('finished');
                    }}
                }}, 400);
            }}, duration);
        }};
    </script>
</body>
</html>"#
    )
}

fn internal_create_window_loop() {
    unsafe {
        // Initialize COM for the thread (Critical for WebView2/Wry)
        let coinit = CoInitialize(None);
        crate::log_info!("[Badge] Internal Loop Start - CoInit: {:?}", coinit);

        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGT_AutoCopyBadgeWebView");

        REGISTER_BADGE_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(badge_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            wc.hbrBackground = HBRUSH(std::ptr::null_mut());
            let _ = RegisterClassW(&wc);
        });
        crate::log_info!("[Badge] Class Registered");

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
        crate::log_info!("[Badge] Window created with HWND: {:?}", hwnd);

        if hwnd.is_invalid() {
            crate::log_info!("[Badge] Window creation failed, HWND is invalid.");
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            BADGE_HWND.store(0, Ordering::SeqCst);
            let _ = CoUninitialize();
            return;
        }

        // Don't store HWND yet - wait until WebView is ready
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        let wrapper = HwndWrapper(hwnd);

        // Initialize shared WebContext if needed (uses same data dir as other modules)
        BADGE_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                // Consolidate all minor overlays to 'common' to share one browser process and keep RAM at ~80MB
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("common"));
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });
        crate::log_info!("[Badge] Starting WebView initialization...");

        // Stagger start to avoid global WebView2 init lock contention
        std::thread::sleep(std::time::Duration::from_millis(50));

        let webview = {
            // LOCK SCOPE: Only one WebView builds at a time to prevent "Not enough quota"
            let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();
            crate::log_info!("[Badge] Acquired init lock. Building...");

            let build_res = BADGE_WEB_CONTEXT.with(|ctx| {
                let mut ctx_ref = ctx.borrow_mut();
                let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                    WebViewBuilder::new_with_web_context(web_ctx)
                } else {
                    WebViewBuilder::new()
                };

                let builder =
                    crate::overlay::html_components::font_manager::configure_webview(builder);

                // Store HTML in font server and get URL for same-origin font loading
                let badge_html = get_badge_html();
                let page_url = crate::overlay::html_components::font_manager::store_html_page(
                    badge_html.clone(),
                )
                .unwrap_or_else(|| format!("data:text/html,{}", urlencoding::encode(&badge_html)));

                builder
                    .with_transparent(true)
                    .with_bounds(Rect {
                        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                            0, 0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            BADGE_WIDTH as u32,
                            BADGE_HEIGHT as u32,
                        )),
                    })
                    .with_url(&page_url)
                    .with_ipc_handler(move |msg: wry::http::Request<String>| {
                        let body = msg.body();
                        if body == "finished" {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        } else if body.starts_with("error:") {
                            crate::log_info!("[BadgeJS] {}", body);
                        }
                    })
                    .build(&wrapper)
            });

            crate::log_info!(
                "[Badge] Build phase finished. Releasing lock. Status: {}",
                if build_res.is_ok() { "OK" } else { "ERR" }
            );
            build_res
        };

        if let Ok(wv) = webview {
            crate::log_info!("[Badge] WebView initialization SUCCESSFUL");
            BADGE_WEBVIEW.with(|cell| {
                *cell.borrow_mut() = Some(wv);
            });

            // Now that WebView is ready, publicize the HWND and mark as ready
            BADGE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            IS_WARMED_UP.store(true, Ordering::SeqCst);
        } else {
            // Initialization failed - cleanup and exit
            let _ = DestroyWindow(hwnd);
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            BADGE_HWND.store(0, Ordering::SeqCst);
            let _ = CoUninitialize();
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup on exit - reset all state so warmup can be retriggered
        BADGE_WEBVIEW.with(|cell| {
            *cell.borrow_mut() = None;
        });
        BADGE_HWND.store(0, Ordering::SeqCst);
        IS_WARMING_UP.store(false, Ordering::SeqCst);
        IS_WARMED_UP.store(false, Ordering::SeqCst);
        let _ = CoUninitialize();
    }
}

unsafe extern "system" fn badge_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_PROCESS_QUEUE => {
            let app = APP.lock().unwrap();
            let is_dark = match app.config.theme_mode {
                crate::config::ThemeMode::Dark => true,
                crate::config::ThemeMode::Light => false,
                crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
            };
            drop(app);

            // Update badge position (if screen changed?)
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

            // Fetch generic queue items
            let mut items = Vec::new();
            {
                let mut q = PENDING_QUEUE.lock().unwrap();
                while let Some(item) = q.pop_front() {
                    items.push(item);
                }
            }

            if !items.is_empty() {
                BADGE_WEBVIEW.with(|wv| {
                    if let Some(webview) = wv.borrow().as_ref() {
                        // Update Theme
                        let theme_script = format!("window.setTheme({});", is_dark);
                        let _ = webview.evaluate_script(&theme_script);

                        // Add Notifications logic
                        for item in items {
                            let type_str = match item.n_type {
                                NotificationType::Success => "success",
                                NotificationType::Info => "info",
                                NotificationType::Update => "update",
                                NotificationType::Error => "error",
                            };

                            let safe_title = item
                                .title
                                .replace('\\', "\\\\")
                                .replace('"', "\\\"")
                                .replace('\'', "\\'");

                            let safe_snippet = item
                                .snippet
                                .replace('\\', "\\\\")
                                .replace('"', "\\\"")
                                .replace('\'', "\\'")
                                .replace('\n', " ");

                            let script = format!(
                                "window.addNotification('{}', '{}', '{}');",
                                safe_title, safe_snippet, type_str
                            );
                            let _ = webview.evaluate_script(&script);
                        }
                    }
                });
            }

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
