use crate::APP;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Arc, Mutex, Once,
};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Memory::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// Shared wrapper for WebView parent
use crate::overlay::realtime_webview::state::HwndWrapper;

// --- SHARED STATE ---
struct TextSelectionState {
    preset_idx: usize,
    is_selecting: bool,
    is_processing: bool,
    hook_handle: HHOOK,
    webview: Option<wry::WebView>,
}
unsafe impl Send for TextSelectionState {}

static SELECTION_STATE: Mutex<TextSelectionState> = Mutex::new(TextSelectionState {
    preset_idx: 0,
    is_selecting: false,
    is_processing: false,
    hook_handle: HHOOK(std::ptr::null_mut()),
    webview: None,
});

static REGISTER_TAG_CLASS: Once = Once::new();

lazy_static::lazy_static! {
    pub static ref TAG_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref INITIAL_TEXT_GLOBAL: Mutex<String> = Mutex::new(String::from("Select text..."));
}

thread_local! {
    static SELECTION_WEB_CONTEXT: std::cell::RefCell<Option<wry::WebContext>> = std::cell::RefCell::new(None);
}

// Warmup / Persistence Globals
static TAG_HWND: AtomicIsize = AtomicIsize::new(0);
static IS_WARMING_UP: AtomicBool = AtomicBool::new(false);
static IS_WARMED_UP: AtomicBool = AtomicBool::new(false);

// CONTINUOUS MODE HOTKEY TRACKING
static mut TRIGGER_VK_CODE: u32 = 0;
static mut TRIGGER_MODIFIERS: u32 = 0;
static IS_HOTKEY_HELD: AtomicBool = AtomicBool::new(false);
static CONTINUOUS_ACTIVATED_THIS_SESSION: AtomicBool = AtomicBool::new(false);
static HOLD_DETECTED_THIS_SESSION: AtomicBool = AtomicBool::new(false);

// DEDUPLICATION: Track last processed text to prevent reprocessing same content
static LAST_PROCESSED_HASH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// DEDUPLICATION: Timestamp of last instant process to debounce rapid calls
static LAST_INSTANT_PROCESS_TIME: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
// DRAG DETECTION: Mouse start position when selection begins
static MOUSE_START_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MOUSE_START_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

// Messages
const WM_APP_SHOW: u32 = WM_USER + 200;
const WM_APP_HIDE: u32 = WM_USER + 201;

// --- PUBLIC API ---

pub fn is_active() -> bool {
    let hwnd_val = TAG_HWND.load(Ordering::SeqCst);
    if hwnd_val == 0 {
        return false;
    }
    unsafe { IsWindowVisible(HWND(hwnd_val as *mut std::ffi::c_void)).as_bool() }
}

pub fn is_processing() -> bool {
    let state = SELECTION_STATE.lock().unwrap();
    state.is_processing
}

struct ProcessingGuard;

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        let mut state = SELECTION_STATE.lock().unwrap();
        state.is_processing = false;
    }
}

/// Try to process already-selected text instantly.
pub fn try_instant_process(preset_idx: usize) -> bool {
    // TIME-BASED DEBOUNCE: If we processed via instant process recently, skip
    // This prevents multiple processes when holding hotkey on preselected text
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let last_time = LAST_INSTANT_PROCESS_TIME.load(Ordering::SeqCst);
    if last_time > 0 && now - last_time < 2000 {
        return false; // Processed within last 2 seconds, skip
    }

    // Set processing flag early to block other threads
    // The guard will reset it to false when this function returns
    let _guard = {
        let mut state = SELECTION_STATE.lock().unwrap();
        if state.is_processing {
            return false;
        }
        state.is_processing = true;
        ProcessingGuard
    };

    // Update timestamp now that we're committed to processing
    LAST_INSTANT_PROCESS_TIME.store(now, Ordering::SeqCst);

    unsafe {
        // Step 1: Save clipboard
        let original_clipboard = get_clipboard_text();

        // Step 2: Clear & Copy
        if OpenClipboard(Some(HWND::default())).is_ok() {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
        std::thread::sleep(std::time::Duration::from_millis(30));

        let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
            let input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                        wScan: 0,
                    },
                },
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        };

        send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0));
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(0x43, KEYBD_EVENT_FLAGS(0)); // 'C'
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(0x43, KEYEVENTF_KEYUP);
        std::thread::sleep(std::time::Duration::from_millis(15));
        send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);

        // Step 3: Wait & Check
        let mut clipboard_text = String::new();
        for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            clipboard_text = get_clipboard_text();
            if !clipboard_text.is_empty() {
                break;
            }
        }

        if clipboard_text.trim().is_empty() {
            if !original_clipboard.is_empty() {
                crate::overlay::utils::copy_to_clipboard(&original_clipboard, HWND::default());
            }
            return false;
        }

        // HIDE BADGE BEFORE PROCESSING (Critical for Master Wheel appearance)
        cancel_selection();

        // CONTINUOUS MODE SUPPORT for instant process
        // Check if user is holding the hotkey for continuous mode
        let mut final_preset_idx = preset_idx;
        if !crate::overlay::continuous_mode::is_active() {
            // Check if hotkey is being held
            let held = crate::overlay::continuous_mode::was_triggered_recently(1500);
            if held {
                let mut hotkey_name = crate::overlay::continuous_mode::get_hotkey_name();
                if hotkey_name.is_empty() {
                    hotkey_name = "Hotkey".to_string();
                }
                let preset_name = {
                    if let Ok(app) = APP.lock() {
                        app.config
                            .presets
                            .get(preset_idx)
                            .map(|p| p.id.clone())
                            .unwrap_or_default()
                    } else {
                        "Preset".to_string()
                    }
                };

                // Disable continuous mode for Master Preset
                if preset_name != "preset_text_select_master" {
                    crate::overlay::continuous_mode::activate(preset_idx, hotkey_name.clone());
                    crate::overlay::continuous_mode::show_activation_notification(
                        &preset_name,
                        &hotkey_name,
                    );
                }
            }
        }

        // Continuous mode retrigger (immediately, before processing)
        if crate::overlay::continuous_mode::is_active() {
            let current_idx = crate::overlay::continuous_mode::get_preset_idx();
            if current_idx == preset_idx {
                final_preset_idx = current_idx;
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    if crate::overlay::continuous_mode::is_active() {
                        let _ = show_text_selection_tag(current_idx);
                    }
                });
            }
        }

        process_selected_text(final_preset_idx, clipboard_text);
        true
    }
}

pub fn cancel_selection() {
    TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
    let hwnd_val = TAG_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        unsafe {
            // Just hide it, don't destroy
            let _ = PostMessageW(
                Some(HWND(hwnd_val as *mut std::ffi::c_void)),
                WM_APP_HIDE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn warmup() {
    if IS_WARMED_UP.load(Ordering::SeqCst) {
        return;
    }
    if IS_WARMING_UP
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    std::thread::spawn(|| {
        internal_create_tag_thread();
    });
}

// Positioning constants
const OFFSET_X: i32 = -20;
const OFFSET_Y: i32 = -90;

pub fn show_text_selection_tag(preset_idx: usize) {
    // 1. Ensure Warmed Up / Recover
    if !IS_WARMED_UP.load(Ordering::SeqCst) {
        warmup();
        // Wait up to 5s for recovery
        for _ in 0..500 {
            use windows::Win32::UI::WindowsAndMessaging::*;
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            if IS_WARMED_UP.load(Ordering::SeqCst) {
                break;
            }
        }
        if !IS_WARMED_UP.load(Ordering::SeqCst) {
            return;
        }
    }

    // 2. Prepare State
    {
        let mut state = SELECTION_STATE.lock().unwrap();
        state.preset_idx = preset_idx;
        state.is_selecting = false;
        state.is_processing = false;
        TAG_ABORT_SIGNAL.store(false, Ordering::SeqCst);

        // Initialize Hotkey Tracking
        // Only reset session flags if NOT already in continuous mode
        // (to prevent multiple notifications on hotkey repeats)
        if !crate::overlay::continuous_mode::is_active() {
            CONTINUOUS_ACTIVATED_THIS_SESSION.store(false, Ordering::SeqCst);
            HOLD_DETECTED_THIS_SESSION.store(false, Ordering::SeqCst);
        }
        if let Some((mods, vk)) = crate::overlay::continuous_mode::get_current_hotkey_info() {
            unsafe {
                TRIGGER_MODIFIERS = mods;
                TRIGGER_VK_CODE = vk;

                // Actually check if it's physically held
                use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
                if !crate::overlay::continuous_mode::is_active() {
                    let is_physically_held = (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0;
                    IS_HOTKEY_HELD.store(is_physically_held, Ordering::SeqCst);
                }
            }
        } else {
            IS_HOTKEY_HELD.store(false, Ordering::SeqCst);
        }
    }

    // 3. Signal Show (Pre-position to prevent jump/lag)
    let hwnd_val = TAG_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        unsafe {
            let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);

            // Decouple delay: Move window immediately to cursor BEFORE showing
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let target_x = pt.x + OFFSET_X;
            let target_y = pt.y + OFFSET_Y;

            let _ = MoveWindow(hwnd, target_x, target_y, 200, 120, false);

            let _ = PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}

// helper to reset state UI
fn reset_ui_state(initial_text: &str) {
    let state = SELECTION_STATE.lock().unwrap();
    if let Some(wv) = state.webview.as_ref() {
        let reset_js = format!("updateState(false, '{}')", initial_text);
        let _ = wv.evaluate_script(&reset_js);
    }
}

unsafe extern "system" fn tag_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match msg {
        WM_APP_SHOW => {
            // Cancel any pending Hide timer to prevent it from hiding us later
            let _ = KillTimer(Some(hwnd), 1);

            // Trigger Fade In Script
            {
                let state = SELECTION_STATE.lock().unwrap();
                if let Some(wv) = state.webview.as_ref() {
                    let _ = wv.evaluate_script("playEntry();");
                }
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            LRESULT(0)
        }
        WM_APP_HIDE => {
            // Trigger Fade Out Script & Delay Hide
            {
                let state = SELECTION_STATE.lock().unwrap();
                if let Some(wv) = state.webview.as_ref() {
                    let _ = wv.evaluate_script("playExit();");
                }
            }
            // 150ms delay for animation
            SetTimer(Some(hwnd), 1, 150, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == 1 {
                let _ = KillTimer(Some(hwnd), 1);
                // Reset text state internally when truly hidden
                {
                    let initial_text = INITIAL_TEXT_GLOBAL.lock().unwrap();
                    reset_ui_state(&initial_text);
                }
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = KillTimer(Some(hwnd), 1);
            let initial_text = INITIAL_TEXT_GLOBAL.lock().unwrap();
            reset_ui_state(&initial_text);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }));
    match result {
        Ok(lresult) => lresult,
        Err(_) => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn internal_create_tag_thread() {
    unsafe {
        use windows::Win32::System::Com::*;
        let coinit = CoInitialize(None);

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextTag_Web_Persistent");

        REGISTER_TAG_CLASS.call_once(|| {
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(tag_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            let _ = RegisterClassExW(&wc);
        });

        // Create Layered Transparent Window
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT Tag"),
            WS_POPUP,
            -1000,
            -1000,
            200,
            120, // Increased height for glow
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if hwnd.is_invalid() {
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            return;
        }

        // Initialize WebView with dynamic theme support
        let (initial_is_dark, lang) = {
            let app = APP.lock().unwrap();
            (
                app.config.theme_mode == crate::config::ThemeMode::Dark
                    || (app.config.theme_mode == crate::config::ThemeMode::System
                        && crate::gui::utils::is_system_in_dark_mode()),
                app.config.ui_language.clone(),
            )
        };

        let initial_text = match lang.as_str() {
            "vi" => "Bôi đen văn bản...",
            "ko" => "텍스트 선택...",
            _ => "Select text...",
        };
        *INITIAL_TEXT_GLOBAL.lock().unwrap() = initial_text.to_string();
        // Use new get_html with CSS variables and updateTheme function
        let html_content = get_html(initial_text);

        // Consolidate all minor overlays to 'common' to share one browser process and keep RAM at ~80MB
        let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("common"));

        // Initialize shared WebContext if needed
        SELECTION_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                *ctx.borrow_mut() = Some(wry::WebContext::new(Some(shared_data_dir)));
            }
        });

        // Store HTML in font server and get URL for same-origin font loading
        let page_url =
            crate::overlay::html_components::font_manager::store_html_page(html_content.clone())
                .unwrap_or_else(|| {
                    format!("data:text/html,{}", urlencoding::encode(&html_content))
                });

        let mut final_webview: Option<wry::WebView> = None;

        // Small initial delay to avoid collision with other warming-up modules (TextInput/Badge)
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Retry loop for stability (similar to text_input)
        for attempt in 1..=3 {
            let res = {
                // LOCK SCOPE: Only one WebView builds at a time to prevent "Not enough quota"
                let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();

                let build_res = SELECTION_WEB_CONTEXT.with(|ctx| {
                    let mut ctx_ref = ctx.borrow_mut();
                    let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                        wry::WebViewBuilder::new_with_web_context(web_ctx)
                    } else {
                        wry::WebViewBuilder::new()
                    };

                    builder
                        .with_bounds(wry::Rect {
                            position: wry::dpi::Position::Physical(
                                wry::dpi::PhysicalPosition::new(0, 0),
                            ),
                            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(200, 120)),
                        })
                        .with_url(&page_url)
                        .with_transparent(true)
                        .build_as_child(&HwndWrapper(hwnd))
                });

                build_res
            };

            match res {
                Ok(wv) => {
                    final_webview = Some(wv);
                    break;
                }
                Err(e) => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }

        if let Some(webview) = final_webview {
            // Set initial theme
            let init_script = format!("updateTheme({});", initial_is_dark);
            let _ = webview.evaluate_script(&init_script);
            SELECTION_STATE.lock().unwrap().webview = Some(webview);
        } else {
            let _ = DestroyWindow(hwnd);
            IS_WARMING_UP.store(false, Ordering::SeqCst);
            let _ = CoUninitialize();
            return;
        }

        TAG_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        IS_WARMED_UP.store(true, Ordering::SeqCst);
        IS_WARMING_UP.store(false, Ordering::SeqCst);

        let mut msg = MSG::default();
        let mut visible = false;

        // Theme tracking
        let mut current_is_dark = initial_is_dark;
        let mut last_sent_is_selecting = false;

        loop {
            // Check Quit
            if msg.message == WM_QUIT {
                break;
            }

            if visible {
                // Active Loop (Animation/Update) - Poll messages
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        visible = false;
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if msg.message == WM_QUIT {
                    break;
                }
            } else {
                // Inactive Loop - Block until message (e.g., WM_APP_SHOW)
                if GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                } else {
                    break;
                }
            }

            // Check Visibility State (updated by WndProc)
            let is_actually_visible = IsWindowVisible(hwnd).as_bool();

            // On Transition
            if is_actually_visible != visible {
                visible = is_actually_visible;
                // Hook Management
                let mut state = SELECTION_STATE.lock().unwrap();
                if visible {
                    // Install Hook
                    if state.hook_handle.is_invalid() {
                        let hook = SetWindowsHookExW(
                            WH_KEYBOARD_LL,
                            Some(keyboard_hook_proc),
                            Some(GetModuleHandleW(None).unwrap().into()),
                            0,
                        );
                        if let Ok(h) = hook {
                            state.hook_handle = h;
                        }
                    }

                    // CRITICAL: Re-check physical key state AFTER hook is installed.
                    // This catches the race condition where user released key between
                    // the initial GetAsyncKeyState check and hook installation.
                    if TRIGGER_VK_CODE != 0 {
                        let is_still_held =
                            (GetAsyncKeyState(TRIGGER_VK_CODE as i32) as u16 & 0x8000) != 0;
                        if !is_still_held {
                            IS_HOTKEY_HELD.store(false, Ordering::SeqCst);
                        }
                    }

                    // Reset Logic
                    last_sent_is_selecting = false;

                    // Sync Theme (Realtime check on show)
                    let new_is_dark = crate::overlay::is_dark_mode();
                    if new_is_dark != current_is_dark {
                        current_is_dark = new_is_dark;
                        if let Some(wv) = state.webview.as_ref() {
                            let _ =
                                wv.evaluate_script(&format!("updateTheme({});", current_is_dark));
                        }
                    }

                    // Reset State in JS
                    if let Some(wv) = state.webview.as_ref() {
                        let reset_js = format!("updateState(false, '{}')", initial_text);
                        let _ = wv.evaluate_script(&reset_js);
                    }
                } else {
                    // Uninstall Hook ONLY if continuous mode is NOT active.
                    // If continuous mode is active, we keep the hook to catch the exit command (ESC or Hotkey)
                    // even while the tag is temporarily hidden/processing.
                    if !crate::overlay::continuous_mode::is_active()
                        && !state.hook_handle.is_invalid()
                    {
                        let _ = UnhookWindowsHookEx(state.hook_handle);
                        state.hook_handle = HHOOK::default();
                    }
                }
            }

            if visible {
                // 1. Check Abort
                if TAG_ABORT_SIGNAL.load(Ordering::SeqCst) {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    continue;
                }

                // 1.5 Real-time Theme Sync (Check every frame while visible)
                let new_is_dark = crate::overlay::is_dark_mode();
                if new_is_dark != current_is_dark {
                    current_is_dark = new_is_dark;
                    if let Some(wv) = SELECTION_STATE.lock().unwrap().webview.as_ref() {
                        let _ = wv.evaluate_script(&format!("updateTheme({});", current_is_dark));
                    }
                }

                // 2. Logic & Movement
                // 2. Logic & Movement
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let target_x = pt.x + OFFSET_X;
                let target_y = pt.y + OFFSET_Y;

                // Use MoveWindow for Webview host
                let _ = MoveWindow(hwnd, target_x, target_y, 200, 120, false);

                // EARLY CONTINUOUS MODE TRIGGER
                let cm_active = crate::overlay::continuous_mode::is_active();
                let session_activated = CONTINUOUS_ACTIVATED_THIS_SESSION.load(Ordering::SeqCst);

                if !cm_active && !session_activated {
                    // Latch the hold detection early via heartbeats
                    let heartbeat = crate::overlay::continuous_mode::was_triggered_recently(2000);
                    if heartbeat {
                        HOLD_DETECTED_THIS_SESSION.store(true, Ordering::SeqCst);
                    }

                    if HOLD_DETECTED_THIS_SESSION.load(Ordering::SeqCst) {
                        let mut hotkey_name = crate::overlay::continuous_mode::get_hotkey_name();
                        if hotkey_name.is_empty() {
                            hotkey_name = "Hotkey".to_string();
                        }

                        let p_idx = SELECTION_STATE.lock().unwrap().preset_idx;
                        let p_name = {
                            if let Ok(app) = APP.lock() {
                                app.config
                                    .presets
                                    .get(p_idx)
                                    .map(|p| p.id.clone())
                                    .unwrap_or_default()
                            } else {
                                "Preset".to_string()
                            }
                        };

                        // Disable continuous mode for Master Preset
                        if p_name != "preset_text_select_master" {
                            crate::overlay::continuous_mode::activate(p_idx, hotkey_name.clone());
                            crate::overlay::continuous_mode::show_activation_notification(
                                &p_name,
                                &hotkey_name,
                            );
                            CONTINUOUS_ACTIVATED_THIS_SESSION.store(true, Ordering::SeqCst);
                        }
                    }
                }

                let lbutton_down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0;

                let mut should_spawn_thread = false;
                let mut preset_idx_for_thread = 0;

                // Scope for State Lock
                let update_js = {
                    let mut state = SELECTION_STATE.lock().unwrap();

                    if !state.is_selecting && lbutton_down {
                        // Check if mouse is over our own window to avoid triggering selection on UI interaction
                        let mut pt = POINT::default();
                        let _ = GetCursorPos(&mut pt);
                        let hwnd_under_mouse = WindowFromPoint(pt);
                        let mut pid: u32 = 0;
                        unsafe { GetWindowThreadProcessId(hwnd_under_mouse, Some(&mut pid)) };
                        let our_pid = std::process::id();

                        if pid != our_pid {
                            state.is_selecting = true;
                            // Record mouse start position for drag detection
                            MOUSE_START_X.store(pt.x, Ordering::SeqCst);
                            MOUSE_START_Y.store(pt.y, Ordering::SeqCst);
                        }
                    } else if state.is_selecting && !lbutton_down && !state.is_processing {
                        // DRAG DETECTION: Only process if mouse moved significantly
                        let mut pt = POINT::default();
                        let _ = GetCursorPos(&mut pt);
                        let start_x = MOUSE_START_X.load(Ordering::SeqCst);
                        let start_y = MOUSE_START_Y.load(Ordering::SeqCst);
                        let dx = (pt.x - start_x).abs();
                        let dy = (pt.y - start_y).abs();
                        let distance = dx + dy; // Manhattan distance

                        if distance >= 10 {
                            // Real drag/selection detected
                            state.is_processing = true;
                            should_spawn_thread = true;
                            preset_idx_for_thread = state.preset_idx;
                        } else {
                            // Just a click, not a selection - reset state
                            state.is_selecting = false;
                        }
                    }

                    if state.is_selecting != last_sent_is_selecting {
                        last_sent_is_selecting = state.is_selecting;
                        let new_text = if state.is_selecting {
                            match lang.as_str() {
                                "vi" => "Thả chuột để xử lý",
                                "ko" => "처리를 위해 마우스를 놓으세요",
                                _ => "Release to process",
                            }
                        } else {
                            initial_text
                        };
                        Some(format!(
                            "updateState({}, '{}')",
                            state.is_selecting, new_text
                        ))
                    } else {
                        None
                    }
                };

                // Update WebView outside lock
                if let Some(js) = update_js {
                    if let Some(webview) = SELECTION_STATE.lock().unwrap().webview.as_ref() {
                        let _ = webview.evaluate_script(&js);
                    }
                }

                // Spawn Worker Thread
                if should_spawn_thread {
                    let hwnd_val = hwnd.0 as usize;
                    std::thread::spawn(move || {
                        let hwnd_copy = HWND(hwnd_val as *mut std::ffi::c_void);

                        if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));

                        // Clear Clipboard
                        if OpenClipboard(Some(HWND::default())).is_ok() {
                            let _ = EmptyClipboard();
                            let _ = CloseClipboard();
                        }

                        let send_input_event = |vk: u16, flags: KEYBD_EVENT_FLAGS| {
                            let input = INPUT {
                                r#type: INPUT_KEYBOARD,
                                Anonymous: INPUT_0 {
                                    ki: KEYBDINPUT {
                                        wVk: VIRTUAL_KEY(vk),
                                        dwFlags: flags,
                                        time: 0,
                                        dwExtraInfo: 0,
                                        wScan: 0,
                                    },
                                },
                            };
                            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                        };

                        // Ctrl + C chain
                        send_input_event(VK_CONTROL.0, KEYBD_EVENT_FLAGS(0));
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        send_input_event(0x43, KEYBD_EVENT_FLAGS(0));
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        send_input_event(0x43, KEYEVENTF_KEYUP);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        send_input_event(VK_CONTROL.0, KEYEVENTF_KEYUP);

                        let mut clipboard_text = String::new();
                        for _ in 0..10 {
                            if TAG_ABORT_SIGNAL.load(Ordering::Relaxed) {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(25));
                            clipboard_text = get_clipboard_text();
                            if !clipboard_text.is_empty() {
                                break;
                            }
                        }

                        if !clipboard_text.trim().is_empty()
                            && !TAG_ABORT_SIGNAL.load(Ordering::Relaxed)
                        {
                            // HIDE FIRST
                            let _ =
                                PostMessageW(Some(hwnd_copy), WM_APP_HIDE, WPARAM(0), LPARAM(0));

                            let mut p_idx = preset_idx_for_thread;

                            // CHECK FOR CONTINUOUS MODE ACTIVATION
                            let cm_active_before = crate::overlay::continuous_mode::is_active();
                            let session_flag =
                                CONTINUOUS_ACTIVATED_THIS_SESSION.load(Ordering::SeqCst);

                            if !cm_active_before && !session_flag {
                                let mut held = if unsafe { TRIGGER_MODIFIERS == 0 } {
                                    IS_HOTKEY_HELD.load(Ordering::SeqCst)
                                } else {
                                    crate::overlay::continuous_mode::are_modifiers_still_held()
                                };

                                if !held {
                                    held = crate::overlay::continuous_mode::was_triggered_recently(
                                        1500,
                                    );
                                }

                                if held {
                                    let mut hotkey_name =
                                        crate::overlay::continuous_mode::get_hotkey_name();
                                    if hotkey_name.is_empty() {
                                        hotkey_name = "Hotkey".to_string();
                                    }

                                    let preset_name = {
                                        if let Ok(app) = APP.lock() {
                                            app.config
                                                .presets
                                                .get(p_idx)
                                                .map(|p| p.id.clone())
                                                .unwrap_or_default()
                                        } else {
                                            "Preset".to_string()
                                        }
                                    };

                                    let current_active_idx =
                                        crate::overlay::continuous_mode::get_preset_idx();
                                    if current_active_idx != p_idx {
                                        p_idx = current_active_idx;
                                    }
                                    crate::overlay::continuous_mode::activate(
                                        p_idx,
                                        hotkey_name.clone(),
                                    );
                                    crate::overlay::continuous_mode::show_activation_notification(
                                        &preset_name,
                                        &hotkey_name,
                                    );
                                    CONTINUOUS_ACTIVATED_THIS_SESSION.store(true, Ordering::SeqCst);
                                }
                            }

                            // CONTINUOUS MODE RETRIGGER - Immediately after hide, BEFORE processing
                            // This ensures the tag reappears right at mouse release, not after process completes
                            let cm_active = crate::overlay::continuous_mode::is_active();
                            let cm_idx = crate::overlay::continuous_mode::get_preset_idx();
                            if cm_active && cm_idx == p_idx {
                                let retrigger_idx = p_idx;
                                std::thread::spawn(move || {
                                    // Small delay to let the hide animation complete
                                    std::thread::sleep(std::time::Duration::from_millis(150));
                                    if crate::overlay::continuous_mode::is_active() {
                                        let _ = super::show_text_selection_tag(retrigger_idx);
                                    }
                                });
                            }

                            process_selected_text(p_idx, clipboard_text);
                        } else {
                            // Reset state if failed or empty
                            let mut state = SELECTION_STATE.lock().unwrap();
                            state.is_selecting = false;
                            state.is_processing = false;
                        }
                    });
                }

                // 60FPS Cap for polling drag state
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        }

        // Cleanup
        {
            let mut state = SELECTION_STATE.lock().unwrap();
            state.webview = None;
            if !state.hook_handle.is_invalid() {
                let _ = UnhookWindowsHookEx(state.hook_handle);
                state.hook_handle = HHOOK::default();
            }
        }
    }
}

// Reuse helper functions like get_clipboard_text, process_selected_text
unsafe fn get_clipboard_text() -> String {
    let mut result = String::new();
    if OpenClipboard(Some(HWND::default())).is_ok() {
        if let Ok(h_data) = GetClipboardData(13u32) {
            let h_global: HGLOBAL = std::mem::transmute(h_data);
            let ptr = GlobalLock(h_global);
            if !ptr.is_null() {
                let size = GlobalSize(h_global);
                let wide_slice = std::slice::from_raw_parts(ptr as *const u16, size / 2);
                if let Some(end) = wide_slice.iter().position(|&c| c == 0) {
                    result = String::from_utf16_lossy(&wide_slice[..end]);
                }
            }
            let _ = GlobalUnlock(h_global);
        }
        let _ = CloseClipboard();
    }
    result
}

fn process_selected_text(preset_idx: usize, clipboard_text: String) {
    unsafe {
        let (is_master, _original_mode) = {
            let app = APP.lock().unwrap();
            let p = &app.config.presets[preset_idx];
            (p.is_master, p.text_input_mode.clone())
        };

        let final_preset_idx = if is_master {
            let mut cursor_pos = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            let selected =
                crate::overlay::preset_wheel::show_preset_wheel("text", Some("select"), cursor_pos);
            if let Some(idx) = selected {
                idx
            } else {
                return;
            }
        } else {
            preset_idx
        };

        let (config, mut preset, screen_w, screen_h) = {
            let mut app = APP.lock().unwrap();
            app.config.active_preset_idx = final_preset_idx;
            (
                app.config.clone(),
                app.config.presets[final_preset_idx].clone(),
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        };

        preset.text_input_mode = "select".to_string();

        let center_rect = RECT {
            left: (screen_w - 700) / 2,
            top: (screen_h - 300) / 2,
            right: (screen_w + 700) / 2,
            bottom: (screen_h + 300) / 2,
        };
        let localized_name =
            crate::gui::settings_ui::get_localized_preset_name(&preset.id, &config.ui_language);
        let cancel_hotkey = preset
            .hotkeys
            .first()
            .map(|h| h.name.clone())
            .unwrap_or_default();

        crate::overlay::process::start_text_processing(
            clipboard_text,
            center_rect,
            config,
            preset,
            localized_name,
            cancel_hotkey,
        );
        // NOTE: Continuous retrigger is now handled at mouse release, not here
    }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kbd_struct = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize {
            if kbd_struct.vkCode == VK_ESCAPE.0 as u32 {
                crate::overlay::continuous_mode::deactivate();
                TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
                return LRESULT(1);
            }
            if kbd_struct.vkCode == TRIGGER_VK_CODE {
                if !IS_HOTKEY_HELD.load(Ordering::SeqCst) {
                    crate::overlay::continuous_mode::deactivate();
                    TAG_ABORT_SIGNAL.store(true, Ordering::SeqCst);
                    return LRESULT(1);
                }
            }
        } else if wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize {
            if kbd_struct.vkCode == TRIGGER_VK_CODE {
                IS_HOTKEY_HELD.store(false, Ordering::SeqCst);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

// --- HTML CONTENT ---
fn get_html(initial_text: &str) -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        {font_css}
        :root {{
            --bg-color: rgba(255, 255, 255, 0.95);
            --text-color: #202124;
            /* Aurora Gradient - Idle (Blue-Violet-Cyan) */
            --g1: #0033cc;
            --g2: #00ddff;
            --g3: #8844ff;
            /* Aurora Gradient - Active (Red-Gold-Purple DRAMATIC) */
            --a1: #ff0055;
            --a2: #ffdd00;
            --a3: #aa00ff;
            --wave-color: #1a73e8;
        }}
        [data-theme="dark"] {{
            --bg-color: rgba(26, 26, 26, 0.95);
            --text-color: #ffffff;
            /* Aurora Gradient - Idle (Neon Synthwave) */
            --g1: #2bd9fe;
            --g2: #aa22ff;
            --g3: #00fe9b;
            /* Aurora Gradient - Active (Hyper Energy) */
            --a1: #ff00cc;
            --a2: #ccff00;
            --a3: #ff2200;
            --wave-color: #8ab4f8;
        }}

        * {{
            margin: 0;
            padding: 0;
            user-select: none;
            cursor: default;
        }}
        
        body {{
            background: transparent;
            overflow: hidden;
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100vh;
            width: 100vw;
            font-family: 'Google Sans Flex Rounded', 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
            font-weight: 500;
        }}
        
        /* Clip the glow to the container shape to prevent "inside out" giant square */
        .badge-container {{
            position: relative;
            padding: 2px; /* Border thickness */
            border-radius: 999px; /* Pill shape */
            background: var(--bg-color); /* Opaque track */
            overflow: hidden; /* CRITICAL FIX: Clips the spinning gradient */
            opacity: 0; /* Default invisible */
            transform: translateY(10px);
            /* Remove default animation, handled by classes */
            box-shadow: 0 4px 12px rgba(0,0,0,0.25);
            transition: box-shadow 0.2s, transform 0.2s;
        }}

        .badge-container.entering {{
            animation: fadeIn 0.15s cubic-bezier(0.2, 0, 0, 1) forwards;
        }}
        
        .badge-container.exiting {{
            animation: fadeOut 0.15s cubic-bezier(0.2, 0, 0, 1) forwards;
        }}

        .badge-glow {{
            position: absolute;
            top: -50%;
            left: -50%;
            width: 200%;
            height: 200%;
            background: conic-gradient(
                from 0deg, 
                var(--c1), 
                var(--c2), 
                var(--c3), 
                var(--c2), 
                var(--c1)
            );
            animation: spin 4s linear infinite; /* Slower, smoother flow */
            opacity: 1;
            z-index: 0;
            filter: blur(2px); /* Soften the gradient blends */
        }}

        .badge-inner {{
            position: relative;
            background: var(--bg-color); /* Covers the center */
            color: var(--text-color);
            padding: 3px 10px;
            border-radius: 999px; /* Match parent */
            font-size: 12px;
            white-space: nowrap;
            z-index: 1; /* Sit above glow */
            display: flex;
            align-items: center;
            gap: 8px;
            font-stretch: condensed;
            letter-spacing: -0.2px;
            box-shadow: 0 0 4px 1px var(--bg-color); /* Soft edge blending */
        }}

        @keyframes fadeIn {{
            to {{ opacity: 1; transform: translateY(0); }}
        }}

        @keyframes spin {{
            from {{ transform: rotate(0deg); }}
            to {{ transform: rotate(360deg); }}
        }}

        @keyframes waveColor {{
            0% {{
                color: var(--a1);
                font-variation-settings: 'GRAD' 0, 'wght' 500, 'ROND' 100;
                transform: translateY(0px) scale(1);
            }}
            33% {{
                color: var(--a2);
                font-variation-settings: 'GRAD' 200, 'wght' 900, 'ROND' 100;
                transform: translateY(-2px) scale(1.1);
            }}
            66% {{
                color: var(--a3);
                font-variation-settings: 'GRAD' 200, 'wght' 900, 'ROND' 100;
                transform: translateY(-1px) scale(1.1);
            }}
            100% {{
                color: var(--a1);
                font-variation-settings: 'GRAD' 0, 'wght' 500, 'ROND' 100;
                transform: translateY(0px) scale(1);
            }}
        }}

        @keyframes idleWave {{
            0% {{
                color: var(--g1);
                font-variation-settings: 'GRAD' 0, 'wght' 400, 'ROND' 100;
            }}
            50% {{
                color: var(--g2);
                font-variation-settings: 'GRAD' 50, 'wght' 600, 'ROND' 100;
            }}
            100% {{
                color: var(--g1);
                font-variation-settings: 'GRAD' 0, 'wght' 400, 'ROND' 100;
            }}
        }}
        
        @keyframes fadeOut {{
            from {{ opacity: 1; transform: translateY(0); }}
            to {{ opacity: 0; transform: translateY(-10px); }}
        }}

        /* State: Selecting (Active) */
        body.selecting .badge-glow {{
            --c1: var(--a1);
            --c2: var(--a2);
            --c3: var(--a3);
            animation: spin 0.8s linear infinite; /* Faster spin for urgency */
        }}
        
        body.selecting .badge-container {{
            transform: scale(1.05);
            /* Soft orange outer glow */
            box-shadow: 0 0 15px rgba(255, 94, 0, 0.4), 0 4px 12px rgba(0,0,0,0.3);
        }}
        
        /* State: Idle */
        body:not(.selecting) .badge-glow {{
            --c1: var(--g1);
            --c2: var(--g2);
            --c3: var(--g3);
        }}

    </style>
</head>
<body>
    <div class="badge-container">
        <div class="badge-glow"></div>
        <div class="badge-inner">
            <span id="text">{text}</span>
        </div>
    </div>

    <script>
        function playEntry() {{
            const el = document.querySelector('.badge-container');
            if(el) {{
                el.classList.remove('exiting');
                el.classList.add('entering');
            }}
        }}

        function playExit() {{
            const el = document.querySelector('.badge-container');
            if(el) {{
                el.classList.remove('entering');
                el.classList.add('exiting');
            }}
        }}
        
        function updateState(isSelecting, newText) {{
            if (isSelecting) {{
                document.body.classList.add('selecting');
            }} else {{
                document.body.classList.remove('selecting');
            }}
            
            const title = document.getElementById('text');
            if (isSelecting) {{
                // Apply DRAMATIC, SPEEDY, LOOPING Wave Animation
                const chars = newText.split('');
                title.innerHTML = chars.map((char, i) => 
                    `<span style="
                        display: inline-block;
                        animation: waveColor 0.6s linear infinite;
                        animation-delay: ${{i * 0.05}}s;
                    ">${{char === ' ' ? '&nbsp;' : char}}</span>`
                ).join('');
            }} else {{
                // Idle State: Gentle Blue Wave
                const chars = newText.split('');
                title.innerHTML = chars.map((char, i) => 
                    `<span style="
                        display: inline-block;
                        animation: idleWave 3s ease-in-out infinite;
                        animation-delay: ${{i * 0.1}}s;
                    ">${{char === ' ' ? '&nbsp;' : char}}</span>`
                ).join('');
            }}
        }}

        function updateTheme(isDark) {{
            if (isDark) {{
                document.documentElement.setAttribute('data-theme', 'dark');
            }} else {{
                document.documentElement.removeAttribute('data-theme');
            }}
        }}
    </script>
</body>
</html>"#,
        font_css = font_css,
        text = initial_text
    )
}
