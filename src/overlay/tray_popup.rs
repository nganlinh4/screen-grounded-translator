// Tray Popup - Custom non-blocking popup window for tray icon menu
// Replaces native Windows tray context menu to avoid blocking the main UI thread

use crate::APP;
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicIsize, Ordering},
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
use wry::{Rect, WebContext, WebView, WebViewBuilder};

static REGISTER_POPUP_CLASS: Once = Once::new();
static POPUP_HWND: AtomicIsize = AtomicIsize::new(0);
static IGNORE_FOCUS_LOSS_UNTIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// Warmup flag - tracks if the window has been created and is ready for instant display
static IS_WARMED_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static IS_WARMING_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WARMUP_START_TIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// Custom window messages
const WM_APP_SHOW: u32 = WM_APP + 1;


thread_local! {
    static POPUP_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    // Shared WebContext for this thread using common data directory
    static POPUP_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

const BASE_POPUP_WIDTH: i32 = 220;
const BASE_POPUP_HEIGHT: i32 = 152; // Base height at 100% scaling (96 DPI) - includes stop TTS row

/// Get DPI-scaled dimension
fn get_scaled_dimension(base: i32) -> i32 {
    let dpi = unsafe {
        windows::Win32::UI::HiDpi::GetDpiForSystem()
    };
    // Scale: 96 DPI = 100%, 120 DPI = 125%, 144 DPI = 150%, etc.
    // Using 93 instead of 96 provides a small buffer (~3%) to ensure content fits comfortably
    (base * dpi as i32) / 93
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

/// Show the tray popup at cursor position
pub fn show_tray_popup() {
    unsafe {
        // Check if warmed up and window exists
        if !IS_WARMED_UP.load(Ordering::SeqCst) {
            // Not ready yet - trigger warmup and show notification
            warmup_tray_popup();
            
            let ui_lang = APP.lock().unwrap().config.ui_language.clone();
            let locale = crate::gui::locale::LocaleText::get(&ui_lang);
            crate::overlay::auto_copy_badge::show_notification(locale.tray_popup_loading);
            
            // Spawn thread to wait for warmup completion and auto-show
            std::thread::spawn(move || {
                // Poll for 5 seconds (50 * 100ms)
                for _ in 0..50 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    // Check if ready
                    let ready = IS_WARMED_UP.load(Ordering::SeqCst) && POPUP_HWND.load(Ordering::SeqCst) != 0;
                    if ready {
                        show_tray_popup();
                        return;
                    }
                }
            });
            return;
        }
        
        let hwnd_val = POPUP_HWND.load(Ordering::SeqCst);
        if hwnd_val == 0 {
            // Should be warmed up but handle missing? Retry warmup
            IS_WARMED_UP.store(false, Ordering::SeqCst);
            warmup_tray_popup();
            return;
        }
        
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        
        // Check if window still valid logic...
        if !IsWindow(Some(hwnd)).as_bool() {
            // Window destroyed
            IS_WARMED_UP.store(false, Ordering::SeqCst);
            POPUP_HWND.store(0, Ordering::SeqCst);
            warmup_tray_popup();
            return;
        }
        
        // Check if already visible
        if IsWindowVisible(hwnd).as_bool() {
            hide_tray_popup();
            return;
        }
        
        // Post message to show
        let _ = PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0));
    }
}

/// Hide the tray popup (preserves window for reuse)
pub fn hide_tray_popup() {
    let hwnd_val = POPUP_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            // Just hide - don't destroy. Preserves WebView state for instant redisplay.
            let _ = KillTimer(Some(hwnd), 888);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

/// Warmup the tray popup - creates hidden window with WebView for instant display later
pub fn warmup_tray_popup() {
    // Check if dead stuck (timestamp check)
    unsafe {
        let start_time = WARMUP_START_TIME.load(Ordering::SeqCst);
        let now = windows::Win32::System::SystemInformation::GetTickCount64();
        if start_time > 0 && (now - start_time) > 10000 {
            // Stuck for > 10s - force reset
            IS_WARMED_UP.store(false, Ordering::SeqCst);
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            POPUP_HWND.store(0, Ordering::SeqCst);
        }
    }

    // Only allow one warmup thread at a time
    if IS_WARMING_UP
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    // Update timestamp
    unsafe {
        WARMUP_START_TIME.store(windows::Win32::System::SystemInformation::GetTickCount64(), Ordering::SeqCst);
    }
    
    std::thread::spawn(|| {
        create_popup_window();
    });
}

/// Check if the tray popup is currently visible
/// Used by warmup logic to defer WebView2 initialization until popup closes
pub fn is_popup_open() -> bool {
    let hwnd_val = POPUP_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe { IsWindowVisible(hwnd).as_bool() }
    } else {
        false
    }
}

fn generate_popup_html() -> String {
    use crate::config::ThemeMode;
    
    let (settings_text, bubble_text, stop_tts_text, quit_text, bubble_checked, is_dark_mode) = if let Ok(app) = APP.lock() {
        let lang = &app.config.ui_language;
        let settings = match lang.as_str() {
            "vi" => "Cài đặt",
            "ko" => "설정",
            _ => "Settings",
        };
        let bubble = match lang.as_str() {
            "vi" => "Hiện bong bóng",
            "ko" => "즐겨찾기 버블",
            _ => "Favorite Bubble",
        };
        let stop_tts = match lang.as_str() {
            "vi" => "Dừng đọc",
            "ko" => "재생 중인 모든 음성 중지",
            _ => "Stop All Playing TTS",
        };
        let quit = match lang.as_str() {
            "vi" => "Thoát",
            "ko" => "종료",
            _ => "Quit",
        };
        let checked = app.config.show_favorite_bubble;
        
        // Theme detection
        let is_dark = match app.config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        };
        
        (settings, bubble, stop_tts, quit, checked, is_dark)
    } else {
        ("Settings", "Favorite Bubble", "Stop All TTS", "Quit", false, true)
    };

    // Check if TTS has pending audio
    let has_tts_pending = crate::api::tts::TTS_MANAGER.has_pending_audio();

    // Define Colors based on theme
    let (bg_color, text_color, hover_color, border_color, separator_color) = if is_dark_mode {
        ("#2c2c2c", "#ffffff", "#3c3c3c", "#454545", "rgba(255,255,255,0.08)")
    } else {
        ("#f9f9f9", "#1a1a1a", "#eaeaea", "#dcdcdc", "rgba(0,0,0,0.06)")
    };

    let check_mark = if bubble_checked {
        r#"<svg class="check-icon" viewBox="0 0 16 16" fill="currentColor"><path d="M13.86 3.66a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.6 9.03a.75.75 0 1 1 1.06-1.06l2.42 2.42 6.72-6.72a.75.75 0 0 1 1.06 0z"/></svg>"#
    } else {
        ""
    };
    
    let active_class = if bubble_checked {
        "active"
    } else {
        ""
    };

    let stop_tts_disabled_class = if has_tts_pending { "" } else { "disabled" };

    // Get font CSS to preload fonts into WebView2 cache (tray popup warms up first)
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{font_css}
:root {{
    --bg-color: {bg};
    --text-color: {text};
    --hover-bg: {hover};
    --border-color: {border};
    --separator-color: {separator};
}}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--bg-color);
    font-family: 'Google Sans Flex', 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif;
    font-variation-settings: 'ROND' 100;
    user-select: none;
    color: var(--text-color);
    border: 1px solid var(--border-color);
    border-radius: 8px;
}}

.container {{
    display: flex;
    flex-direction: column;
    padding: 4px;
}}

.menu-item {{
    display: flex;
    align-items: center;
    padding: 6px 10px;
    border-radius: 4px;
    cursor: default;
    font-size: 13px;
    margin-bottom: 2px;
    background: transparent;
    transition: background 0.1s ease;
    height: 32px;
}}

.menu-item:hover {{
    background: var(--hover-bg);
}}

.icon {{
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    margin-right: 12px;
    opacity: 0.8;
}}

.label {{
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-bottom: 1px; /* Visual alignment */
}}

.check {{
    width: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-left: 8px;
}}

.separator {{
    height: 1px;
    background: var(--separator-color);
    margin: 4px 10px;
}}

svg {{
    width: 16px;
    height: 16px;
}}


.bubble-item .label {{
    transition: font-variation-settings 0.4s cubic-bezier(0.33, 1, 0.68, 1);
    font-variation-settings: 'wght' 400, 'wdth' 100, 'ROND' 100;
}}
.bubble-item.active .label {{
    font-variation-settings: 'wght' 700, 'wdth' 110, 'ROND' 100;
    color: var(--text-color);
}}

.menu-item.disabled {{
    opacity: 0.4;
    pointer-events: none;
}}
</style>
</head>
<body>
<div class="container">
    <div class="menu-item" onclick="action('settings')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.09a2 2 0 0 1-1-1.74v-.47a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.39a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"></path>
                <circle cx="12" cy="12" r="3"></circle>
            </svg>
        </div>
        <div class="label">{settings}</div>
        <div class="check"></div>
    </div>
    
    <div class="menu-item bubble-item {active_class}" data-state="{active_class}" onclick="action('bubble')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
        </div>
        <div class="label">{bubble}</div>
        <div class="check" id="bubble-check-container">{check}</div>
    </div>
    
    <div class="menu-item {stop_tts_disabled}" id="stop-tts-item" onclick="action('stop_tts')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5L6 9H2v6h4l5 4V5z"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/></svg>
        </div>
        <div class="label">{stop_tts}</div>
        <div class="check"></div>
    </div>
    
    <div class="separator"></div>
    
    <div class="menu-item" onclick="action('quit')">
        <div class="icon">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
        </div>
        <div class="label">{quit}</div>
        <div class="check"></div>
    </div>
</div>
<script>
window.ignoreBlur = false;
function action(cmd) {{
    if (cmd === 'bubble') {{
        window.ignoreBlur = true;
        setTimeout(function() {{ window.ignoreBlur = false; }}, 1200);
        const el = document.querySelector('.bubble-item');
        if (el) {{
            if (el.classList.contains('active')) {{
                el.classList.remove('active');
            }} else {{
                el.classList.add('active');
            }}
        }}
    }}
    window.ipc.postMessage(cmd);
}}

// Update popup state without reloading (preserves font cache)
window.updatePopupState = function(config) {{
    // Update CSS variables for theme
    document.documentElement.style.setProperty('--bg-color', config.bgColor);
    document.documentElement.style.setProperty('--text-color', config.textColor);
    document.documentElement.style.setProperty('--hover-bg', config.hoverColor);
    document.documentElement.style.setProperty('--border-color', config.borderColor);
    document.documentElement.style.setProperty('--separator-color', config.separatorColor);
    
    // Update bubble active state
    const bubbleItem = document.querySelector('.bubble-item');
    if (bubbleItem) {{
        if (config.bubbleActive) {{
            bubbleItem.classList.add('active');
            document.getElementById('bubble-check-container').innerHTML = '<svg class="check-icon" viewBox="0 0 16 16" fill="currentColor"><path d="M13.86 3.66a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.6 9.03a.75.75 0 1 1 1.06-1.06l2.42 2.42 6.72-6.72a.75.75 0 0 1 1.06 0z"/></svg>';
        }} else {{
            bubbleItem.classList.remove('active');
            document.getElementById('bubble-check-container').innerHTML = '';
        }}
    }}
    
    // Update stop TTS disabled state
    const stopTtsItem = document.getElementById('stop-tts-item');
    if (stopTtsItem) {{
        if (config.ttsDisabled) {{
            stopTtsItem.classList.add('disabled');
        }} else {{
            stopTtsItem.classList.remove('disabled');
        }}
    }}
}};

window.addEventListener('blur', function() {{
    if (window.ignoreBlur) return;
    window.ipc.postMessage('close');
}});
</script>
</body>
</html>"#,
        bg = bg_color,
        text = text_color,
        hover = hover_color,
        border = border_color,
        separator = separator_color,
        settings = settings_text,
        bubble = bubble_text,
        stop_tts = stop_tts_text,
        stop_tts_disabled = stop_tts_disabled_class,
        quit = quit_text,
        check = check_mark
    )
}

/// Generate JavaScript to update popup state without reloading HTML
fn generate_popup_update_script() -> String {
    use crate::config::ThemeMode;
    
    let (bubble_checked, is_dark_mode) = if let Ok(app) = APP.lock() {
        let is_dark = match app.config.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        };
        (app.config.show_favorite_bubble, is_dark)
    } else {
        (false, true)
    };

    let has_tts_pending = crate::api::tts::TTS_MANAGER.has_pending_audio();

    let (bg_color, text_color, hover_color, border_color, separator_color) = if is_dark_mode {
        ("#2c2c2c", "#ffffff", "#3c3c3c", "#454545", "rgba(255,255,255,0.08)")
    } else {
        ("#f9f9f9", "#1a1a1a", "#eaeaea", "#dcdcdc", "rgba(0,0,0,0.06)")
    };

    format!(
        r#"window.updatePopupState({{ 
            bgColor: '{}', 
            textColor: '{}', 
            hoverColor: '{}', 
            borderColor: '{}', 
            separatorColor: '{}',
            bubbleActive: {},
            ttsDisabled: {}
        }});"#,
        bg_color,
        text_color,
        hover_color,
        border_color,
        separator_color,
        bubble_checked,
        !has_tts_pending
    )
}

// Cleanup guard removed - window persists for entire app lifetime

/// Creates the popup window and runs its message loop forever.
/// This is called once during warmup - the window is kept alive hidden for reuse.
fn create_popup_window() {
    unsafe {
        // Initialize COM for the thread (Critical for WebView2/Wry)
        let coinit = windows::Win32::System::Com::CoInitialize(None);
        crate::log_info!("[TrayPopup] Loop Start - CoInit: {:?}", coinit);
        
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTTrayPopup");

        REGISTER_POPUP_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(popup_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });
        crate::log_info!("[TrayPopup] Class Registered");

        // Get DPI-scaled dimensions
        let popup_height = get_scaled_dimension(BASE_POPUP_HEIGHT);
        let popup_width = get_scaled_dimension(BASE_POPUP_WIDTH);

        // Create hidden off-screen (will be repositioned when shown)
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("TrayPopup"),
            WS_POPUP,
            -3000,
            -3000,
            popup_width,
            popup_height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        crate::log_info!("[TrayPopup] Window created with HWND: {:?}", hwnd);

        if hwnd.is_invalid() {
            return;
        }

        POPUP_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // Make transparent initially (invisible)
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

        // Round corners
        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::addr_of!(corner_pref) as *const _,
            std::mem::size_of_val(&corner_pref) as u32,
        );

        // Create WebView using shared context for RAM efficiency
        let wrapper = HwndWrapper(hwnd);
        let html = generate_popup_html();

        // Initialize shared WebContext if needed (uses same data dir as other modules)
        POPUP_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                // Use separate data dir to avoid process-sharing hangs on low-quota systems
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("tray"));
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });
        
        crate::log_info!("[TrayPopup] Starting WebView initialization...");

        let mut final_webview: Option<WebView> = None;
        
        // Stagger startup to avoid collision
        std::thread::sleep(std::time::Duration::from_millis(250));

        for attempt in 1..=3 {
            let res = {
                // LOCK SCOPE: Only one WebView builds at a time to prevent "Not enough quota"
                let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();
                crate::log_info!("[TrayPopup] (Attempt {}) Acquired init lock. Building...", attempt);
                
                let build_res = POPUP_WEB_CONTEXT.with(|ctx| {
                    let mut ctx_ref = ctx.borrow_mut();
                    let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                        WebViewBuilder::new_with_web_context(web_ctx)
                    } else {
                        WebViewBuilder::new()
                    };
                    let builder = crate::overlay::html_components::font_manager::configure_webview(builder);
                    
                    // Store HTML in font server and get URL for same-origin font loading
                    let page_url = crate::overlay::html_components::font_manager::store_html_page(html.clone())
                        .unwrap_or_else(|| format!("data:text/html,{}", urlencoding::encode(&html)));
                    
                    builder
                        .with_bounds(Rect {
                            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                                popup_width as u32,
                                popup_height as u32,
                            )),
                        })
                        .with_transparent(true)
                        .with_url(&page_url)
                        .with_ipc_handler(move |msg: wry::http::Request<String>| {
                            let body = msg.body();
                            match body.as_str() {
                                "settings" => {
                                    // Hide popup and restore main window
                                    hide_tray_popup();
                                    crate::gui::signal_restore_window();
                                }
                                "bubble" => {
                                    // Toggle bubble state
                                    let new_state = if let Ok(mut app) = APP.lock() {
                                        app.config.show_favorite_bubble = !app.config.show_favorite_bubble;
                                        let enabled = app.config.show_favorite_bubble;
                                        crate::config::save_config(&app.config);

                                        if enabled {
                                            crate::overlay::favorite_bubble::show_favorite_bubble();
                                            std::thread::spawn(|| {
                                                std::thread::sleep(std::time::Duration::from_millis(150));
                                                crate::overlay::favorite_bubble::trigger_blink_animation();
                                            });
                                        } else {
                                            crate::overlay::favorite_bubble::hide_favorite_bubble();
                                        }
                                        enabled
                                    } else {
                                        false
                                    };

                                    // Update checkmark in popup via JavaScript (keep popup open)
                                    POPUP_WEBVIEW.with(|cell| {
                                        if let Some(webview) = cell.borrow().as_ref() {
                                            let js = format!(
                                                "document.getElementById('bubble-check-container').innerHTML = '{}';",
                                                if new_state { 
                                                    r#"<svg class="check-icon" viewBox="0 0 16 16" fill="currentColor"><path d="M13.86 3.66a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.6 9.03a.75.75 0 1 1 1.06-1.06l2.42 2.42 6.72-6.72a.75.75 0 0 1 1.06 0z"/></svg>"#
                                                } else { "" }
                                            );
                                            let _ = webview.evaluate_script(&js);
                                        }
                                    });
                                }
                                "stop_tts" => {
                                    // Stop all TTS playback and clear queues
                                    crate::api::tts::TTS_MANAGER.stop();
                                    // Hide popup after action
                                    hide_tray_popup();
                                }
                                "quit" => {
                                    // Hide popup first, then exit
                                    hide_tray_popup();
                                    std::thread::spawn(|| {
                                        std::thread::sleep(std::time::Duration::from_millis(50));
                                        std::process::exit(0);
                                    });
                                }
                                "close" => {
                                    hide_tray_popup();
                                }
                                _ => {}
                            }
                        })
                        .build(&wrapper)
                });
                build_res
            };

            crate::log_info!("[TrayPopup] (Attempt {}) Release lock. Result: {}", attempt, if res.is_ok() { "OK" } else { "ERR" });
    
            match res {
                Ok(wv) => {
                    final_webview = Some(wv);
                    break;
                }
                Err(e) => {
                    crate::log_info!("[TrayPopup] WebView init attempt {} failed: {:?}", attempt, e);
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                }
            }
        }

        if let Some(wv) = final_webview {
            crate::log_info!("[TrayPopup] WebView initialization SUCCESSFUL");
            POPUP_WEBVIEW.with(|cell| {
                *cell.borrow_mut() = Some(wv);
            });

            // Mark as warmed up - ready for instant display
            IS_WARMED_UP.store(true, Ordering::SeqCst);
            IS_WARMING_UP.store(false, Ordering::SeqCst); // Done warming up
            WARMUP_START_TIME.store(0, Ordering::SeqCst);

            // Message loop runs forever to keep window alive
            let mut msg = MSG::default();
            unsafe {
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        } else {
            crate::log_info!("[TrayPopup] FAILED to initialize WebView after 3 attempts.");
        }

        // Clean up on thread exit
        IS_WARMED_UP.store(false, Ordering::SeqCst);
        IS_WARMING_UP.store(false, Ordering::SeqCst);
        POPUP_HWND.store(0, Ordering::SeqCst);
        WARMUP_START_TIME.store(0, Ordering::SeqCst);
        POPUP_WEBVIEW.with(|cell| {
            *cell.borrow_mut() = None;
        });
        
        unsafe { let _ = windows::Win32::System::Com::CoUninitialize(); }
    }
}

unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW => {
            // Reposition window to cursor and show
            let popup_height = get_scaled_dimension(BASE_POPUP_HEIGHT);
            let popup_width = get_scaled_dimension(BASE_POPUP_WIDTH);
            
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);

            let popup_x = (pt.x - popup_width / 2).max(0).min(screen_w - popup_width);
            let popup_y = (pt.y - popup_height - 10).max(0).min(screen_h - popup_height);
            
            // Update state via JavaScript (preserves font cache - no reload flash)
            POPUP_WEBVIEW.with(|cell| {
                if let Some(webview) = cell.borrow().as_ref() {
                    let update_script = generate_popup_update_script();
                    let _ = webview.evaluate_script(&update_script);
                }
            });
            
            // Resize WebView to current DPI
            POPUP_WEBVIEW.with(|cell| {
                if let Some(webview) = cell.borrow().as_ref() {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            popup_width as u32,
                            popup_height as u32,
                        )),
                    });
                }
            });
            
            // Reposition and resize window
            let _ = SetWindowPos(hwnd, None, popup_x, popup_y, popup_width, popup_height, SWP_NOZORDER);
            
            // Make fully visible (undo the warmup transparency)
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            
            // Show and focus
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            
            // Start focus-polling timer
            let _ = SetTimer(Some(hwnd), 888, 100, None);
            
            LRESULT(0)
        }
        
        WM_ACTIVATE => {
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == 888 {
                // Focus polling: check if we're still the active window
                let fg = GetForegroundWindow();
                let root = GetAncestor(fg, GA_ROOT);
                
                // If focus is on this popup or its children (WebView2), stay open
                if fg == hwnd || root == hwnd {
                    return LRESULT(0);
                }
                
                // Focus is elsewhere - check grace period
                let now = windows::Win32::System::SystemInformation::GetTickCount64();
                if now > IGNORE_FOCUS_LOSS_UNTIL.load(Ordering::SeqCst) {
                    let _ = KillTimer(Some(hwnd), 888);
                    hide_tray_popup();
                }
            }
            LRESULT(0)
        }

        WM_CLOSE => {
            // Just hide - don't destroy. Preserves WebView for instant redisplay.
            let _ = KillTimer(Some(hwnd), 888);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
