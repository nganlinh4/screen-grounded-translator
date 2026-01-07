// use crate::win_types::SendHwnd; // Removed
use crate::APP;
use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering},
    Arc, Mutex, Once,
};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::System::Com::{CoInitialize, CoUninitialize};
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebView, WebViewBuilder};

// --- GLOBAL SIGNALS (Preserving existing logic usage) ---
lazy_static::lazy_static! {
    pub static ref AUDIO_STOP_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_PAUSE_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_ABORT_SIGNAL: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    pub static ref AUDIO_WARMUP_COMPLETE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    /// Signal for Gemini Live initialization phase (WebSocket setup)
    pub static ref AUDIO_INITIALIZING: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    static ref VISUALIZATION_BUFFER: Mutex<[f32; 40]> = Mutex::new([0.0; 40]);
}

static LAST_SHOW_TIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub static CURRENT_RMS: AtomicU32 = AtomicU32::new(0);

pub fn update_audio_viz(rms: f32) {
    let bits = rms.to_bits();
    CURRENT_RMS.store(bits, Ordering::Relaxed);
}

// --- STATE MANAGEMENT ---
// 0=Not Created, 1=Hidden/Warmup, 2=Visible/Recording
static RECORDING_STATE: AtomicI32 = AtomicI32::new(0);
static RECORDING_HWND_VAL: AtomicIsize = AtomicIsize::new(0);
static REGISTER_RECORDING_CLASS: Once = Once::new();
static LAST_THEME_IS_DARK: AtomicBool = AtomicBool::new(true);
static CURRENT_RECORDING_HIDDEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    static RECORDING_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    static RECORDING_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

// --- ADAPTIVE UI SIZE ---
fn get_ui_dimensions() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    // Width scales inversely with aspect ratio for consistent UI appearance
    // At 16:9 (1.78:1): 450px width
    // At 21:9 (2.37:1): 375px width (narrower on ultrawide)
    let aspect_ratio = screen_w as f64 / screen_h as f64;
    let base_aspect = 16.0 / 9.0; // 1.778
    let width = (450.0 - (aspect_ratio - base_aspect) * 127.0).clamp(350.0, 500.0) as i32;

    // Height stays constant at 70px
    let height = 70;

    (width, height)
}

const WM_APP_SHOW: u32 = WM_USER + 20;
const WM_APP_HIDE: u32 = WM_USER + 21;
const WM_APP_REAL_SHOW: u32 = WM_USER + 22;
const WM_APP_UPDATE_STATE: u32 = WM_USER + 23;

// --- PUBLIC API ---

pub fn is_recording_overlay_active() -> bool {
    RECORDING_STATE.load(Ordering::SeqCst) == 2
}

pub fn stop_recording_and_submit() {
    // Check if we are already active
    if is_recording_overlay_active() {
        let was_stopped = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);

        // If already stopped (processing) or aborted, hitting this again should FORCE CLOSE
        if was_stopped {
            AUDIO_ABORT_SIGNAL.store(true, Ordering::SeqCst);
            let hwnd_val = RECORDING_HWND_VAL.load(Ordering::SeqCst);
            if hwnd_val != 0 {
                let hwnd = HWND(hwnd_val as *mut _);
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                }
            }
        } else {
            // First time: Just stop and let it process
            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
            // Force update UI to "Processing"
            let hwnd_val = RECORDING_HWND_VAL.load(Ordering::SeqCst);
            if hwnd_val != 0 {
                let hwnd = HWND(hwnd_val as *mut _);
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_APP_UPDATE_STATE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
}

pub fn warmup_recording_overlay() {
    // Transition 0 -> 1
    if RECORDING_STATE
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        std::thread::spawn(|| {
            internal_create_recording_window();
        });
    }
}

pub fn show_recording_overlay(preset_idx: usize) {
    // Check current state
    let current = RECORDING_STATE.load(Ordering::SeqCst);

    // If state is 0, warmup hasn't started - trigger it and show notification
    // If state is 0 (not started) or 1 (stuck warming up), trigger recovery and auto-show
    if current == 0 || (current == 1 && RECORDING_HWND_VAL.load(Ordering::SeqCst) == 0) {
        // Reset state if stuck
        if current == 1 {
            RECORDING_STATE.store(0, Ordering::SeqCst);
        }

        // Start warmup
        warmup_recording_overlay();

        // Show loading notification
        let ui_lang = APP.lock().unwrap().config.ui_language.clone();
        let locale = crate::gui::locale::LocaleText::get(&ui_lang);
        crate::overlay::auto_copy_badge::show_notification(locale.recording_loading);

        // Spawn a thread to wait for warmup completion and then trigger show
        std::thread::spawn(move || {
            // Poll for up to 5 seconds
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if RECORDING_HWND_VAL.load(Ordering::SeqCst) != 0 {
                    // Ready! Trigger show
                    unsafe {
                        let hwnd = HWND(RECORDING_HWND_VAL.load(Ordering::SeqCst) as *mut _);
                        let _ =
                            PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(preset_idx), LPARAM(0));
                    }
                    return;
                }
            }
        });

        return;
    }

    // Wait for HWND to be valid (state is 1 or 2)
    let hwnd_val = RECORDING_HWND_VAL.load(Ordering::SeqCst);

    if hwnd_val != 0 {
        // Reset Signals
        AUDIO_STOP_SIGNAL.store(false, Ordering::SeqCst);
        AUDIO_PAUSE_SIGNAL.store(false, Ordering::SeqCst);
        AUDIO_ABORT_SIGNAL.store(false, Ordering::SeqCst);
        AUDIO_WARMUP_COMPLETE.store(false, Ordering::SeqCst);
        CURRENT_RMS.store(0, Ordering::Relaxed);

        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_val as *mut _)),
                WM_APP_SHOW,
                WPARAM(preset_idx),
                LPARAM(0),
            );
        }
    } else {
        // HWND not ready yet, reset state and try again
        RECORDING_STATE.store(0, Ordering::SeqCst);
        warmup_recording_overlay();

        let ui_lang = APP.lock().unwrap().config.ui_language.clone();
        let locale = crate::gui::locale::LocaleText::get(&ui_lang);
        crate::overlay::auto_copy_badge::show_notification(locale.recording_loading);
    }
}

// --- INTERNAL IMPLEMENTATION ---

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

fn internal_create_recording_window() {
    unsafe {
        let _ = CoInitialize(None); // Required for WebView
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_Recording_Persistent");

        REGISTER_RECORDING_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(recording_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            RegisterClassW(&wc);
        });

        // Get adaptive UI dimensions
        let (ui_width, ui_height) = get_ui_dimensions();

        // Create window OFF-SCREEN initially (-4000, -4000)
        // WS_POPUP | WS_VISIBLE (so WebView renders) but off-screen.
        // Using Layered window for transparency
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("SGT Recording Web"),
            WS_POPUP | WS_VISIBLE,
            -4000,
            -4000,
            ui_width,
            ui_height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        RECORDING_HWND_VAL.store(hwnd.0 as isize, Ordering::SeqCst);

        // Windows 11 Rounded Corners - Disable native rounding to hide native border/shadow
        // We rely on CSS for rounded corners + transparency
        let corner_pref = 1u32; // DWMWCP_DONOTROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::addr_of!(corner_pref) as *const _,
            std::mem::size_of_val(&corner_pref) as u32,
        );

        // Glass Frame Extension (critical for per-pixel alpha with WebView)
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // --- WEBVIEW CREATION ---
        let wrapper = HwndWrapper(hwnd);
        let html = generate_html();

        RECORDING_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        let ipc_hwnd_val = hwnd.0 as usize;
        let webview_res = RECORDING_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let mut builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                WebViewBuilder::new_with_web_context(web_ctx)
            } else {
                WebViewBuilder::new()
            };

            builder = crate::overlay::html_components::font_manager::configure_webview(builder);

            let (ui_width, ui_height) = get_ui_dimensions();
            builder
                .with_bounds(Rect {
                    position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        ui_width as u32,
                        ui_height as u32,
                    )),
                })
                .with_transparent(true)
                .with_background_color((0, 0, 0, 0)) // Fully transparent background
                .with_html(&html)
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    let hwnd = HWND(ipc_hwnd_val as *mut std::ffi::c_void);
                    let body = msg.body().as_str();
                    match body {
                        "pause_toggle" => {
                            let paused = AUDIO_PAUSE_SIGNAL.load(Ordering::SeqCst);
                            AUDIO_PAUSE_SIGNAL.store(!paused, Ordering::SeqCst);
                        }
                        "cancel" | "close" => {
                            AUDIO_ABORT_SIGNAL.store(true, Ordering::SeqCst);
                            AUDIO_STOP_SIGNAL.store(true, Ordering::SeqCst);
                            let _ = PostMessageW(Some(hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                        }
                        "ready" => {
                            // Handshake: WebView is ready (from resetState), so now we can REAL_SHOW
                            // Kill fallback timer 99
                            let _ = KillTimer(Some(hwnd), 99);
                            // Add a tiny delay to ensure paint catch-up
                            if !CURRENT_RECORDING_HIDDEN.load(Ordering::SeqCst) {
                                let _ = SetTimer(Some(hwnd), 2, 20, None);
                            }
                        }
                        "drag_window" => {
                            let _ = ReleaseCapture();
                            let _ = PostMessageW(
                                Some(hwnd),
                                WM_NCLBUTTONDOWN,
                                WPARAM(2 as usize), // HTCAPTION = 2
                                LPARAM(0 as isize),
                            );
                        }
                        _ => {}
                    }
                })
                .build(&wrapper)
        });

        if let Ok(wv) = webview_res {
            RECORDING_WEBVIEW.with(|cell| *cell.borrow_mut() = Some(wv));

            // Setup Global Key Hook for ESC (This needs to be persistent or installed/uninstalled on show/hide)
            // Better to install once and check `is_recording_overlay_active()` inside hook.
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(recording_hook_proc),
                Some(GetModuleHandleW(None).unwrap().into()),
                0,
            );

            // Message Loop
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }

            if let Ok(h) = hook {
                let _ = UnhookWindowsHookEx(h);
            }
        }

        // Cleanup on FULL EXIT
        RECORDING_WEBVIEW.with(|cell| *cell.borrow_mut() = None);
        RECORDING_STATE.store(0, Ordering::SeqCst);

        let _ = CoUninitialize();
    }
}

fn start_audio_thread(hwnd: HWND, preset_idx: usize) {
    let (preset, last_active_window) = {
        let app = APP.lock().unwrap();
        (
            app.config.presets[preset_idx].clone(),
            app.last_active_window, // Keep as SendHwnd for safety across threads
        )
    };
    let hwnd_val = hwnd.0 as usize;

    // Check audio streaming modes
    let (use_gemini_live_stream, use_parakeet_stream) = {
        let mut gemini = false;
        let mut parakeet = false;

        for block in &preset.blocks {
            if block.block_type == "audio" {
                if let Some(config) = crate::model_config::get_model_by_id(&block.model) {
                    if config.provider == "gemini-live" {
                        gemini = true;
                    }
                    if config.provider == "parakeet" {
                        parakeet = true;
                    }
                }
            }
        }
        (gemini, parakeet)
    };

    std::thread::spawn(move || {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        let target = last_active_window.map(|h| h.0);

        if use_gemini_live_stream {
            // Use real-time streaming for Gemini Live
            crate::api::record_and_stream_gemini_live(
                preset,
                AUDIO_STOP_SIGNAL.clone(),
                AUDIO_PAUSE_SIGNAL.clone(),
                AUDIO_ABORT_SIGNAL.clone(),
                hwnd,
                target,
            );
        } else if use_parakeet_stream {
            // Use real-time streaming for Parakeet (Local)
            crate::api::audio::record_and_stream_parakeet(
                preset,
                AUDIO_STOP_SIGNAL.clone(),
                AUDIO_PAUSE_SIGNAL.clone(),
                AUDIO_ABORT_SIGNAL.clone(),
                hwnd,
                target,
            );
        } else {
            // Use standard record-then-transcribe flow
            crate::api::record_audio_and_transcribe(
                preset,
                AUDIO_STOP_SIGNAL.clone(),
                AUDIO_PAUSE_SIGNAL.clone(),
                AUDIO_ABORT_SIGNAL.clone(),
                hwnd,
            );
        }
    });
}

unsafe extern "system" fn recording_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW => {
            // 1. Prepare Content (while still off-screen)
            let preset_idx = wparam.0;

            // Reset JS state
            RECORDING_WEBVIEW.with(|cell| {
                if let Some(wv) = cell.borrow().as_ref() {
                    let _ = wv.evaluate_script("resetState();");
                }
            });

            // 2. Start Audio Logic
            start_audio_thread(hwnd, preset_idx);

            // 3. Mark state as Active (Visible)
            RECORDING_STATE.store(2, Ordering::SeqCst);

            // 4. Check if we should hide the UI
            let is_hidden = {
                let app = APP.lock().unwrap();
                if preset_idx < app.config.presets.len() {
                    app.config.presets[preset_idx].hide_recording_ui
                } else {
                    false
                }
            };
            CURRENT_RECORDING_HIDDEN.store(is_hidden, Ordering::SeqCst);

            // 5. Fallback Timer (99) - If IPC ready signal doesn't come in 500ms, show anyway
            // If hidden, we don't set the show timers.
            if !is_hidden {
                SetTimer(Some(hwnd), 99, 500, None);
            }

            // 5. REMOVED Timer 2 here. We now confirm via IPC "ready" signal to trigger the show.
            // This ensures we never show a blank window on first load.

            // Record Show Time to prevent race with old threads closing
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            LAST_SHOW_TIME.store(now, Ordering::SeqCst);

            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == 2 {
                // REAL SHOW TIMER (from IPC "ready")
                let _ = KillTimer(Some(hwnd), 2);
                let _ = PostMessageW(Some(hwnd), WM_APP_REAL_SHOW, WPARAM(0), LPARAM(0));
            } else if wparam.0 == 99 {
                // FALLBACK TIMER (IPC timed out)
                let _ = KillTimer(Some(hwnd), 99);
                println!("Warning: Recording overlay IPC timed out, forcing show");
                let _ = PostMessageW(Some(hwnd), WM_APP_REAL_SHOW, WPARAM(0), LPARAM(0));
            } else if wparam.0 == 1 {
                // VIZ UPDATE TIMER
                let is_processing = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
                let is_paused = AUDIO_PAUSE_SIGNAL.load(Ordering::SeqCst);
                let is_initializing = AUDIO_INITIALIZING.load(Ordering::SeqCst);
                let warming_up = !AUDIO_WARMUP_COMPLETE.load(Ordering::SeqCst);

                let rms_bits = CURRENT_RMS.load(Ordering::Relaxed);
                let rms = f32::from_bits(rms_bits);

                let state_str = if is_processing {
                    "processing"
                } else if is_paused {
                    "paused"
                } else if is_initializing {
                    "initializing"
                } else if warming_up {
                    "warmup"
                } else {
                    "recording"
                };

                let script = format!("updateState('{}', {});", state_str, rms);

                RECORDING_WEBVIEW.with(|cell| {
                    if let Some(wv) = cell.borrow().as_ref() {
                        let _ = wv.evaluate_script(&script);
                    }
                });

                // Check for theme changes
                if let Ok(app) = APP.try_lock() {
                    let current_is_dark = match app.config.theme_mode {
                        crate::config::ThemeMode::Dark => true,
                        crate::config::ThemeMode::Light => false,
                        crate::config::ThemeMode::System => {
                            crate::gui::utils::is_system_in_dark_mode()
                        }
                    };
                    let last_dark = LAST_THEME_IS_DARK.load(Ordering::SeqCst);

                    if current_is_dark != last_dark {
                        LAST_THEME_IS_DARK.store(current_is_dark, Ordering::SeqCst);

                        // Generate new theme colors
                        let (
                            container_bg,
                            container_border,
                            text_color,
                            subtext_color,
                            btn_bg,
                            btn_hover_bg,
                            btn_color,
                            text_shadow,
                        ) = if current_is_dark {
                            (
                                "rgba(18, 18, 18, 0.85)",
                                "rgba(255, 255, 255, 0.1)",
                                "white",
                                "rgba(255, 255, 255, 0.7)",
                                "rgba(255, 255, 255, 0.05)",
                                "rgba(255, 255, 255, 0.15)",
                                "rgba(255, 255, 255, 0.8)",
                                "0 1px 2px rgba(0, 0, 0, 0.3)",
                            )
                        } else {
                            (
                                "rgba(255, 255, 255, 0.92)",
                                "rgba(0, 0, 0, 0.1)",
                                "#222222",
                                "rgba(0, 0, 0, 0.6)",
                                "rgba(0, 0, 0, 0.05)",
                                "rgba(0, 0, 0, 0.1)",
                                "rgba(0, 0, 0, 0.7)",
                                "0 1px 2px rgba(255, 255, 255, 0.3)",
                            )
                        };

                        let theme_script = format!(
                            "if(window.updateTheme) window.updateTheme({}, '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}');",
                            current_is_dark, container_bg, container_border, text_color, subtext_color, btn_bg, btn_hover_bg, btn_color, text_shadow
                        );

                        RECORDING_WEBVIEW.with(|cell| {
                            if let Some(wv) = cell.borrow().as_ref() {
                                let _ = wv.evaluate_script(&theme_script);
                            }
                        });
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_REAL_SHOW => {
            if CURRENT_RECORDING_HIDDEN.load(Ordering::SeqCst) {
                return LRESULT(0);
            }
            // Move to Center Screen
            let (ui_width, ui_height) = get_ui_dimensions();
            let screen_x = GetSystemMetrics(SM_CXSCREEN);
            let screen_y = GetSystemMetrics(SM_CYSCREEN);
            let center_x = (screen_x - ui_width) / 2;
            let center_y = (screen_y - ui_height) / 2 + 100;

            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                center_x,
                center_y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            // Set Foreground/Focus
            // let _ = SetForegroundWindow(hwnd);
            // let _ = SetFocus(Some(hwnd));

            // Start Visualization Updates NOW that we are visible and ready
            // Only needing one timer start here
            let _ = SetTimer(Some(hwnd), 1, 16, None);

            // Trigger Fade In - window is now in position
            RECORDING_WEBVIEW.with(|cell| {
                if let Some(wv) = cell.borrow().as_ref() {
                    let _ = wv.evaluate_script(
                        "setTimeout(() => document.body.classList.add('visible'), 50);",
                    );
                }
            });

            LRESULT(0)
        }

        WM_APP_HIDE => {
            // Stop logic
            let _ = KillTimer(Some(hwnd), 1);
            let _ = KillTimer(Some(hwnd), 2);
            let _ = KillTimer(Some(hwnd), 99);

            // Reset opacity immediately so it's ready for next time
            RECORDING_WEBVIEW.with(|cell| {
                if let Some(wv) = cell.borrow().as_ref() {
                    // Use hideState() which DOES NOT trigger 'ready' signal
                    // This prevents the recursion where hide -> reset -> ready -> show -> hide
                    let _ = wv.evaluate_script("hideState();");
                }
            });

            // Move Off-screen
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                -4000,
                -4000,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );

            RECORDING_STATE.store(1, Ordering::SeqCst); // Back to Warmup/Hidden

            LRESULT(0)
        }

        WM_APP_UPDATE_STATE => {
            // Just force an immediate update cycle if needed (e.g. for processing state)
            // Timer handles this mostly, but this can be used for instant response
            LRESULT(0)
        }

        WM_CLOSE => {
            // "Close" means Hide in this persistent model
            // LOGIC FIX: Check if this is a 'stale' close from a previous thread
            let is_stop = AUDIO_STOP_SIGNAL.load(Ordering::SeqCst);
            let is_abort = AUDIO_ABORT_SIGNAL.load(Ordering::SeqCst);

            if is_stop || is_abort {
                // User requested stop/abort, so this close is valid
                let _ = PostMessageW(Some(hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
            } else {
                // Natural close (error or finish?)
                // Check if we JUST started. If so, it's likely the old thread dying.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let last = LAST_SHOW_TIME.load(Ordering::SeqCst);
                if now > last && (now - last) < 2000 {
                    // Ignore Close during first 2 seconds if not explicitly stopped
                    // This prevents race condition where previous aborted thread sends WM_CLOSE late
                } else {
                    let _ = PostMessageW(Some(hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }

        WM_USER_FULL_CLOSE => {
            let _ = DestroyWindow(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn recording_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize {
            if kbd.vkCode == VK_ESCAPE.0 as u32 {
                if is_recording_overlay_active() {
                    stop_recording_and_submit();
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

const WM_USER_FULL_CLOSE: u32 = WM_USER + 99;

// --- HTML GENERATOR ---
fn generate_html() -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();
    let icon_pause = crate::overlay::html_components::icons::get_icon_svg("pause");
    let icon_play = crate::overlay::html_components::icons::get_icon_svg("play_arrow");
    let icon_close = crate::overlay::html_components::icons::get_icon_svg("close");
    let (text_rec, text_proc, text_wait, text_init, subtext, text_paused, is_dark) = {
        let app = APP.lock().unwrap();
        let lang = app.config.ui_language.as_str();
        let locale = crate::gui::locale::LocaleText::get(lang);
        let is_dark = match app.config.theme_mode {
            crate::config::ThemeMode::Dark => true,
            crate::config::ThemeMode::Light => false,
            crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        };
        // Store initial theme state
        LAST_THEME_IS_DARK.store(is_dark, Ordering::SeqCst);
        (
            match lang {
                "vi" => "Đang ghi âm...",
                "ko" => "녹음 중...",
                _ => "Recording...",
            },
            match lang {
                "vi" => "Đang xử lý...",
                "ko" => "처리 중...",
                _ => "Processing...",
            },
            match lang {
                "vi" => "Chuẩn bị...",
                "ko" => "준비 중...",
                _ => "Starting...",
            },
            match lang {
                "vi" => "Đang kết nối...",
                "ko" => "연결 중...",
                _ => "Connecting...",
            },
            locale.recording_subtext,
            locale.recording_paused,
            is_dark,
        )
    };

    // Theme-specific colors
    let (
        container_bg,
        container_border,
        text_color,
        subtext_color,
        btn_bg,
        btn_hover_bg,
        btn_color,
        text_shadow,
    ) = if is_dark {
        // Dark mode
        (
            "rgba(18, 18, 18, 0.85)",
            "rgba(255, 255, 255, 0.1)",
            "white",
            "rgba(255, 255, 255, 0.7)",
            "rgba(255, 255, 255, 0.05)",
            "rgba(255, 255, 255, 0.15)",
            "rgba(255, 255, 255, 0.8)",
            "0 1px 2px rgba(0, 0, 0, 0.3)",
        )
    } else {
        // Light mode
        (
            "rgba(255, 255, 255, 0.92)",
            "rgba(0, 0, 0, 0.1)",
            "#222222",
            "rgba(0, 0, 0, 0.6)",
            "rgba(0, 0, 0, 0.05)",
            "rgba(0, 0, 0, 0.1)",
            "rgba(0, 0, 0, 0.7)",
            "0 1px 2px rgba(255, 255, 255, 0.3)",
        )
    };

    format!(
        r#"
<!DOCTYPE html>
<html>
<head>
<style>
    {font_css}
    
    * {{ box-sizing: border-box; user-select: none; }}
    
    body {{
        margin: 0;
        padding: 0;
        width: 100vw;
        height: 100vh;
        overflow: hidden;
        background: transparent;
        display: flex;
        justify-content: center;
        align-items: center;
        opacity: 0;
        transition: opacity 0.15s ease-out; 
    }}
    
    body.visible {{
        opacity: 1;
    }}

    .container {{
        width: {width}px;
        height: {height}px;
        background: {container_bg};
        backdrop-filter: blur(20px);
        -webkit-backdrop-filter: blur(20px);
        border: 1px solid {container_border};
        border-radius: 50px;
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        padding: 0 3px;
        gap: 6px;
        position: relative;
        color: {text_color};
        font-family: 'Google Sans Flex', sans-serif;
    }}

    .text-group {{
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        justify-content: center;
        flex-grow: 1;
        min-width: 0;
        margin-left: 5px;
    }}

    .status-text {{
        font-size: 15px;
        font-weight: 700;
        margin-bottom: 2px;
        text-shadow: {text_shadow};
        font-stretch: expanded;
        white-space: nowrap;
    }}
    
    .sub-text {{
        font-size: 10px;
        color: {subtext_color};
        margin-bottom: 0;
        white-space: nowrap;
        font-family: 'Google Sans Flex', sans-serif;
        font-variation-settings: 'opsz' 14;
    }}

    /* Volume Canvas Styling */
    #volume-canvas {{
        height: 30px;
        width: 100px;
        margin-right: 5px;
    }}

    .btn {{
        position: relative;
        width: 34px; 
        height: 34px;
        border-radius: 50%;
        background: {btn_bg};
        display: flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;
        pointer-events: auto;
        transition: background 0.2s, transform 0.1s;
        color: {btn_color};
        flex-shrink: 0;
        margin: 0 2px;
    }}
    
    .btn:hover {{
        background: {btn_hover_bg};
    }}
    .btn:active {{
        transform: scale(0.95);
    }}

    .btn svg {{
        width: 24px;
        height: 24px;
        fill: currentColor;
        display: block;
    }}
    
    .btn-close svg {{
        width: 36px;
        height: 36px;
    }}
    
    #icon-pause, #icon-play {{
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
    }}
    
    .hidden {{ display: none !important; }}

</style>
</head>
<body>
    <div class="container">
        <!-- 1. Play/Pause -->
        <div class="btn btn-pause" onclick="togglePause()" id="btn-pause">
             <div id="icon-pause">{icon_pause}</div>
             <div id="icon-play" class="hidden">{icon_play}</div>
        </div>

        <!-- 2. Text -->
        <div class="text-group">
            <div class="status-text" id="status">{tx_rec}</div>
            <div class="sub-text">{tx_sub}</div>
        </div>
        
        <!-- 3. Waveform -->
        <!-- 3. Waveform (Canvas) -->
        <div style="display: flex; align-items: center;">
            <canvas id="volume-canvas" width="200" height="60" style="width: 100px; height: 30px;"></canvas>
        </div>

        <!-- 4. Close -->
        <div class="btn btn-close" onclick="closeApp()">
            {icon_close}
        </div>
    </div>

    <script>
        // const ipc = window.__TAURI__.ipc; // Removed - not using Tauri here, just Wry

        
        // I18n constants
        const TEXT_REC = "{tx_rec}";
        const TEXT_PROC = "{tx_proc}";
        const TEXT_WAIT = "{tx_wait}";
        const TEXT_INIT = "{tx_init}";
        const TEXT_PAUSED = "{tx_paused}";

        const statusEl = document.getElementById('status');
        const pauseBtn = document.getElementById('btn-pause');
        const iconPause = document.getElementById('icon-pause');
        const iconPlay = document.getElementById('icon-play');
        
        let currentState = "warmup"; 
        
        // --- CANVAS WAVEFORM LOGIC ---
        const volumeCanvas = document.getElementById('volume-canvas');
        const volumeCtx = volumeCanvas ? volumeCanvas.getContext('2d') : null;
        
        const BAR_WIDTH = 8; 
        const BAR_GAP = 6;
        const BAR_SPACING = BAR_WIDTH + BAR_GAP;
        const VISIBLE_BARS = 20; 
        
        const barHeights = new Array(VISIBLE_BARS + 2).fill(6);
        let latestRMS = 0;
        let scrollProgress = 0;
        let lastTime = 0;
        let animationFrame = null;
        
        // Theme state (passed from Rust)
        let isDark = {is_dark};
        
        // Color Schemes for Dark Mode
        const COLORS_DARK = {{
            recording:    ['#00a8e0', '#00c8ff', '#40e0ff'], // Light Cyan
            processing:   ['#00FF00', '#32CD32', '#98FB98'], // Green (unused - rainbow)
            warmup:       ['#FFD700', '#FFA500', '#FFDEAD'], // Gold/Orange
            initializing: ['#9F7AEA', '#805AD5', '#B794F4'], // Purple/Violet
            paused:       ['#888888', '#AAAAAA', '#CCCCCC']  // Grey
        }};
        
        // Color Schemes for Light Mode (darker, more saturated)
        const COLORS_LIGHT = {{
            recording:    ['#0066cc', '#0088dd', '#00aaee'], // Deep Blue
            processing:   ['#00AA00', '#008800', '#006600'], // Dark Green (unused - rainbow)
            warmup:       ['#cc6600', '#dd8800', '#ee9900'], // Dark Orange
            initializing: ['#6B46C1', '#553C9A', '#805AD5'], // Deep Purple
            paused:       ['#666666', '#888888', '#aaaaaa']  // Dark Grey
        }};
        
        let COLORS = isDark ? COLORS_DARK : COLORS_LIGHT;
        let currentColors = COLORS.warmup;

        function updateState(state, rms) {{
            currentState = state;
            latestRMS = rms; 
            
            if (state === 'processing') {{
                 statusEl.innerText = TEXT_PROC;
                 currentColors = COLORS.processing;
                 // Don't reset bars - let them transition smoothly from recording
                 // New DNA-pattern bars will be added as old ones scroll off
                 pauseBtn.style.visibility = 'hidden';
                 pauseBtn.style.pointerEvents = 'none';
            }} else if (state === 'paused') {{
                 statusEl.innerText = TEXT_PAUSED;
                 currentColors = COLORS.paused;
                 // Reset bars to minimal when paused
                 for (let i = 0; i < barHeights.length; i++) barHeights[i] = 6;
                 pauseBtn.style.visibility = 'visible';
                 pauseBtn.style.pointerEvents = 'auto';
                 iconPause.classList.add('hidden');
                 iconPlay.classList.remove('hidden');
            }} else if (state === 'initializing') {{
                 statusEl.innerText = TEXT_INIT;
                 currentColors = COLORS.initializing;
                 // Pulsing bars during initialization
                 for (let i = 0; i < barHeights.length; i++) barHeights[i] = 6;
                 // Hide pause button during initialization
                 pauseBtn.style.visibility = 'hidden';
                 pauseBtn.style.pointerEvents = 'none';
            }} else if (state === 'warmup') {{
                 statusEl.innerText = TEXT_WAIT;
                 currentColors = COLORS.warmup;
                 // Reset bars to minimal when entering warmup to clear lingering full bars
                 for (let i = 0; i < barHeights.length; i++) barHeights[i] = 6;
                 // Hide pause button during warmup
                 pauseBtn.style.visibility = 'hidden';
                 pauseBtn.style.pointerEvents = 'none';
            }} else {{
                 statusEl.innerText = TEXT_REC;
                 currentColors = COLORS.recording;
                 pauseBtn.style.visibility = 'visible';
                 pauseBtn.style.pointerEvents = 'auto';
                 iconPause.classList.remove('hidden');
                 iconPlay.classList.add('hidden');
            }}
        }}

        function drawWaveform(timestamp) {{
            if (!volumeCtx) return;
            
            const dt = lastTime ? (timestamp - lastTime) / 1000 : 0.016;
            lastTime = timestamp;
            
            // Speed: faster for processing to create urgency effect
            const speed = currentState === 'processing' ? 0.06 : 0.15;
            scrollProgress += dt / speed;
            
            // When in processing, apply decay to existing bars for smooth transition
            if (currentState === 'processing') {{
                const decayFactor = 0.95; // Shrink old bars by 5% each frame
                const minHeight = 15;
                for (let i = 0; i < barHeights.length; i++) {{
                    if (barHeights[i] > minHeight) {{
                        barHeights[i] = Math.max(minHeight, barHeights[i] * decayFactor);
                    }}
                }}
            }}
            
            while (scrollProgress >= 1) {{
                scrollProgress -= 1;
                barHeights.shift();
                
                const h = volumeCanvas.height;
                let displayRMS = latestRMS;
                if (currentState === 'processing') {{
                    // DNA-like sine wave pattern for processing
                    displayRMS = 0.12 + 0.2 * Math.abs(Math.sin(timestamp / 120));
                }} else if (currentState === 'initializing') {{
                    // Gentle pulsing wave for initialization
                    displayRMS = 0.08 + 0.12 * Math.abs(Math.sin(timestamp / 300));
                }} else if (currentState === 'paused') {{
                    displayRMS = 0.02; // Tiny dots
                }} else if (currentState === 'warmup') {{
                    displayRMS = 0.02; // Minimal - tiny orange dots
                }}
                
                let v = Math.max(6, Math.min(h - 4, displayRMS * 250 + 6));
                barHeights.push(v);
            }}
            
            const w = volumeCanvas.width;
            const h = volumeCanvas.height;
            volumeCtx.clearRect(0, 0, w, h);
            
            const pixelOffset = scrollProgress * BAR_SPACING;
            
            // For processing: draw each bar with its own rainbow color AND DNA wave height
            // For others: use a single gradient
            if (currentState === 'processing') {{
                const baseHue = (timestamp / 20) % 360; // Slower cycling base
                const wavePhase = timestamp / 200; // Animation phase for wave movement
                
                for (let i = 0; i < barHeights.length; i++) {{
                    // DNA wave: each bar height based on position + time for traveling wave
                    const waveValue = Math.sin((i * 0.4) + wavePhase);
                    const pillHeight = 12 + 35 * Math.abs(waveValue); // Range: 12 to 47
                    
                    const x = i * BAR_SPACING - pixelOffset;
                    const y = (h - pillHeight) / 2;
                    
                    if (x > -BAR_WIDTH && x < w) {{
                        // Each bar gets a different hue
                        const barHue = (baseHue + i * 18) % 360;
                        volumeCtx.fillStyle = `hsl(${{barHue}}, 100%, 55%)`;
                        volumeCtx.beginPath();
                        if (volumeCtx.roundRect) {{
                            volumeCtx.roundRect(x, y, BAR_WIDTH, pillHeight, BAR_WIDTH / 2);
                        }} else {{
                            volumeCtx.rect(x, y, BAR_WIDTH, pillHeight);
                        }}
                        volumeCtx.fill();
                    }}
                }}
            }} else {{
                // Normal gradient for other states
                const grad = volumeCtx.createLinearGradient(0, h, 0, 0);
                grad.addColorStop(0, currentColors[0]);
                grad.addColorStop(0.5, currentColors[1]);
                grad.addColorStop(1, currentColors[2]);
                volumeCtx.fillStyle = grad;
                
                for (let i = 0; i < barHeights.length; i++) {{
                    const pillHeight = barHeights[i];
                    const x = i * BAR_SPACING - pixelOffset;
                    const y = (h - pillHeight) / 2;
                    
                    if (x > -BAR_WIDTH && x < w) {{
                        volumeCtx.beginPath();
                        if (volumeCtx.roundRect) {{
                            volumeCtx.roundRect(x, y, BAR_WIDTH, pillHeight, BAR_WIDTH / 2);
                        }} else {{
                            volumeCtx.rect(x, y, BAR_WIDTH, pillHeight);
                        }}
                        volumeCtx.fill();
                    }}
                }}
            }}
            
            animationFrame = requestAnimationFrame(drawWaveform);
        }}

        if (!animationFrame) {{
            animationFrame = requestAnimationFrame(drawWaveform);
        }}

        function togglePause() {{
            window.ipc.postMessage('pause_toggle');
        }}
        
        function closeApp() {{
            window.ipc.postMessage('cancel');
        }}
        
        function resetState() {{
            hideState();
            setTimeout(() => {{
                 window.ipc.postMessage('ready');
            }}, 10);
        }}

        const container = document.querySelector('.container');
        container.addEventListener('mousedown', (e) => {{
            if (e.target.closest('.btn')) return;
            window.ipc.postMessage('drag_window');
        }});

        function hideState() {{
            document.body.classList.remove('visible');
        }}
        
        // Dynamic theme update function (called from Rust)
        window.updateTheme = function(newIsDark, containerBg, containerBorder, textColor, subtextColor, btnBg, btnHoverBg, btnColor, textShadow) {{
            isDark = newIsDark;
            COLORS = isDark ? COLORS_DARK : COLORS_LIGHT;
            
            // Update CSS
            const container = document.querySelector('.container');
            container.style.background = containerBg;
            container.style.borderColor = containerBorder;
            container.style.color = textColor;
            
            const subtext = document.querySelector('.sub-text');
            if (subtext) subtext.style.color = subtextColor;
            
            const statusText = document.querySelector('.status-text');
            if (statusText) statusText.style.textShadow = textShadow;
            
            document.querySelectorAll('.btn').forEach(btn => {{
                btn.style.background = btnBg;
                btn.style.color = btnColor;
            }});
            
            // Update current colors based on current state
            if (currentState === 'recording') currentColors = COLORS.recording;
            else if (currentState === 'paused') currentColors = COLORS.paused;
            else if (currentState === 'warmup') currentColors = COLORS.warmup;
            else if (currentState === 'initializing') currentColors = COLORS.initializing;
            else if (currentState === 'processing') currentColors = COLORS.processing;
        }};
    </script>
</body>
</html>
    "#,
        width = get_ui_dimensions().0 - 20,
        height = get_ui_dimensions().1 - 20,
        font_css = font_css,
        tx_rec = text_rec,
        tx_proc = text_proc,
        tx_wait = text_wait,
        tx_init = text_init,
        tx_sub = subtext,
        tx_paused = text_paused,
        icon_pause = icon_pause,
        icon_play = icon_play,
        icon_close = icon_close,
        container_bg = container_bg,
        container_border = container_border,
        text_color = text_color,
        subtext_color = subtext_color,
        btn_bg = btn_bg,
        btn_hover_bg = btn_hover_bg,
        btn_color = btn_color,
        text_shadow = text_shadow,
        is_dark = if is_dark { "true" } else { "false" }
    )
}
