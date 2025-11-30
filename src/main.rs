#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod api;
mod gui;
mod overlay;
mod icon_gen;
mod model_config;
mod updater;

use std::sync::{Arc, Mutex};
use std::panic;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Threading::*;
use windows::core::*;
use lazy_static::lazy_static;
use image::ImageBuffer;
use config::{Config, load_config};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem}};
use std::collections::HashMap;

// Global event for inter-process restore signaling (manual-reset event)
lazy_static! {
    pub static ref RESTORE_EVENT: Option<windows::Win32::Foundation::HANDLE> = unsafe {
        CreateEventW(None, true, false, w!("ScreenGroundedTranslatorRestoreEvent")).ok()
    };
}

pub struct AppState {
    pub config: Config,
    pub original_screenshot: Option<ImageBuffer<image::Rgba<u8>, Vec<u8>>>,
    pub hotkeys_updated: bool,
    pub registered_hotkey_ids: Vec<i32>, // Track IDs of currently registered hotkeys
    // New: Track API usage limits (Key: Model Full Name, Value: "Remaining / Total")
    pub model_usage_stats: HashMap<String, String>, 
}

lazy_static! {
    pub static ref APP: Arc<Mutex<AppState>> = Arc::new(Mutex::new({
        let config = load_config();
        AppState {
            config,
            original_screenshot: None,
            hotkeys_updated: false,
            registered_hotkey_ids: Vec::new(),
            model_usage_stats: HashMap::new(),
        }
    }));
}

fn main() -> eframe::Result<()> {
    // --- CRASH HANDLER START ---
    panic::set_hook(Box::new(|panic_info| {
        // 1. Format the error message
        let location = if let Some(location) = panic_info.location() {
            format!("File: {}\nLine: {}", location.file(), location.line())
        } else {
            "Unknown location".to_string()
        };

        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        let error_msg = format!("CRASH DETECTED!\n\nError: {}\n\nLocation:\n{}", payload, location);

        // Show a Windows Message Box so the user knows it crashed
        let wide_msg: Vec<u16> = error_msg.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_title: Vec<u16> = "SGT Crash Report".encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            MessageBoxW(
                None,
                PCWSTR(wide_msg.as_ptr()),
                PCWSTR(wide_title.as_ptr()),
                MB_ICONERROR | MB_OK
            );
        }
    }));
    // --- CRASH HANDLER END ---
    
    // Ensure the named event exists (for first instance, for second instance to signal)
    let _ = RESTORE_EVENT.as_ref();
    
    // Keep the handle alive for the duration of the program
    let _single_instance_mutex = unsafe {
        let instance = CreateMutexW(None, true, w!("ScreenGroundedTranslatorSingleInstanceMutex"));
        if let Ok(handle) = instance {
            if GetLastError() == ERROR_ALREADY_EXISTS {
                // Another instance is running - signal it to restore
                if let Some(event) = RESTORE_EVENT.as_ref() {
                    let _ = SetEvent(*event);
                }
                return Ok(());
            }
            Some(handle)
        } else {
            None
        }
    };

    std::thread::spawn(|| {
        run_hotkey_listener();
    });

    let tray_menu = Menu::new();
    let settings_i = MenuItem::with_id("1002", "Settings", true, None);
    let quit_i = MenuItem::with_id("1001", "Quit", true, None);
    let _ = tray_menu.append(&settings_i);
    let _ = tray_menu.append(&quit_i);

    let icon = icon_gen::generate_icon();
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu.clone()))
        .with_tooltip("Screen Grounded Translator (nganlinh4)")
        .with_icon(icon)
        .build()
        .unwrap();

    let mut viewport_builder = eframe::egui::ViewportBuilder::default()
        .with_inner_size([635.0, 500.0]) 
        .with_resizable(true)
        .with_visible(false) // Start invisible
        .with_transparent(false) 
        .with_decorations(true); // FIX: Start WITH decorations, opaque window
    
    let app_icon_bytes = include_bytes!("../assets/app-icon-small.png");
    if let Ok(img) = image::load_from_memory(app_icon_bytes) {
        let img_rgba = img.to_rgba8();
        let (width, height) = img_rgba.dimensions();
        let icon_data = eframe::egui::IconData {
            rgba: img_rgba.to_vec(),
            width,
            height,
        };
        viewport_builder = viewport_builder.with_icon(std::sync::Arc::new(icon_data));
    }
    
    let options = eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    };
    
    let initial_config = APP.lock().unwrap().config.clone();
    
    eframe::run_native(
        "Screen Grounded Translator (SGT by nganlinh4)",
        options,
        Box::new(move |cc| {
            gui::configure_fonts(&cc.egui_ctx);
            Box::new(gui::SettingsApp::new(initial_config, APP.clone(), tray_icon, tray_menu, cc.egui_ctx.clone()))
        }),
    )
}

fn register_all_hotkeys(hwnd: HWND) {
    let mut app = APP.lock().unwrap();
    let presets = &app.config.presets;
    
    let mut registered_ids = Vec::new();
    for (p_idx, preset) in presets.iter().enumerate() {
        for (h_idx, hotkey) in preset.hotkeys.iter().enumerate() {
            // ID encoding: 1000 * preset_idx + hotkey_idx + 1
            let id = (p_idx as i32 * 1000) + (h_idx as i32) + 1;
            unsafe {
                RegisterHotKey(hwnd, id, HOT_KEY_MODIFIERS(hotkey.modifiers), hotkey.code);
            }
            registered_ids.push(id);
        }
    }
    app.registered_hotkey_ids = registered_ids;
}

fn unregister_all_hotkeys(hwnd: HWND) {
    let app = APP.lock().unwrap();
    for &id in &app.registered_hotkey_ids {
        unsafe { UnregisterHotKey(hwnd, id); }
    }
}

const WM_RELOAD_HOTKEYS: u32 = WM_USER + 101;

fn run_hotkey_listener() {
    unsafe {
        // Error handling: GetModuleHandleW should not fail, but handle it
        let instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => {
                eprintln!("Error: Failed to get module handle for hotkey listener");
                return;
            }
        };
        
        let class_name = w!("HotkeyListenerClass");
        
        let wc = WNDCLASSW {
            lpfnWndProc: Some(hotkey_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        
        // RegisterClassW can fail if class already exists, which is okay
        let _ = RegisterClassW(&wc);
        
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Listener"),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            None, None, instance, None
        );
        
        // Error handling: hwnd is invalid if creation failed
        if hwnd.0 == 0 {
            eprintln!("Error: Failed to create hotkey listener window");
            return;
        }

        register_all_hotkeys(hwnd);

        let mut msg = MSG::default();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).into() {
                if msg.message == WM_RELOAD_HOTKEYS {
                    unregister_all_hotkeys(hwnd);
                    register_all_hotkeys(hwnd);
                    
                    if let Ok(mut app) = APP.lock() {
                         app.hotkeys_updated = false;
                    }
                } else {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

unsafe extern "system" fn hotkey_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            if id > 0 {
                let preset_idx = ((id - 1) / 1000) as usize;
                
                let preset_type = {
                    if let Ok(app) = APP.lock() {
                        if preset_idx < app.config.presets.len() {
                            app.config.presets[preset_idx].preset_type.clone()
                        } else { "image".to_string() }
                    } else {
                        eprintln!("Error: APP mutex poisoned on hotkey trigger.");
                        return LRESULT(0);
                    }
                };

                if preset_type == "audio" {
                    if overlay::is_recording_overlay_active() {
                        overlay::stop_recording_and_submit();
                    } else {
                        std::thread::spawn(move || {
                            overlay::show_recording_overlay(preset_idx);
                        });
                    }
                } else {
                    if overlay::is_selection_overlay_active_and_dismiss() {
                        return LRESULT(0);
                    }
                    
                    let app_clone = APP.clone();
                    let p_idx = preset_idx;

                    std::thread::spawn(move || {
                        match capture_full_screen() {
                            Ok(img) => {
                                if let Ok(mut app) = app_clone.lock() {
                                    app.original_screenshot = Some(img);
                                } else {
                                    return;
                                }
                                overlay::show_selection_overlay(p_idx);
                            },
                            Err(e) => {
                                eprintln!("Capture Error: {}", e);
                            }
                        }
                    });
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn capture_full_screen() -> anyhow::Result<ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        
        // Validate dimensions
        if width <= 0 || height <= 0 {
            return Err(anyhow::anyhow!("GDI Error: Invalid screen dimensions ({} x {})", width, height));
        }

        let hdc_screen = GetDC(None);
        if hdc_screen.0 == 0 {
            return Err(anyhow::anyhow!("GDI Error: Failed to get screen device context"));
        }
        
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.0 == 0 {
            ReleaseDC(None, hdc_screen);
            return Err(anyhow::anyhow!("GDI Error: Failed to create compatible device context"));
        }
        
        let hbitmap = CreateCompatibleBitmap(hdc_screen, width, height);
        
        if hbitmap.0 == 0 {
             DeleteDC(hdc_mem);
             ReleaseDC(None, hdc_screen);
             return Err(anyhow::anyhow!("GDI Error: Failed to create compatible bitmap."));
        }
        
        SelectObject(hdc_mem, hbitmap);

        BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, x, y, SRCCOPY).ok()?;

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buffer: Vec<u8> = vec![0; (width * height * 4) as usize];
        GetDIBits(hdc_mem, hbitmap, 0, height as u32, Some(buffer.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);

        for chunk in buffer.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            chunk[3] = 255;
        }

        DeleteObject(hbitmap);
        DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        let img = ImageBuffer::from_raw(width as u32, height as u32, buffer)
            .ok_or_else(|| anyhow::anyhow!("Buffer creation failed"))?;
        
        Ok(img)
    }
}