#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod api;
mod gui;
mod overlay;
mod icon_gen;
mod model_config;

use std::sync::{Arc, Mutex};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::System::Threading::*;
use windows::core::*;
use lazy_static::lazy_static;
use image::ImageBuffer;
use config::{Config, load_config};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem}};

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
}

lazy_static! {
    pub static ref APP: Arc<Mutex<AppState>> = Arc::new(Mutex::new({
        let config = load_config();
        AppState {
            config,
            original_screenshot: None,
            hotkeys_updated: false,
            registered_hotkey_ids: Vec::new(),
        }
    }));
}

fn main() -> eframe::Result<()> {
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

    unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); }

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
        .with_inner_size([600.0, 500.0]) // Increased size for new UI
        .with_resizable(true);
    
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
        "Screen Grounded Translator",
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
            // This assumes < 1000 presets and < 1000 hotkeys per preset.
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

fn run_hotkey_listener() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("HotkeyListenerClass");
        
        let wc = WNDCLASSW {
            lpfnWndProc: Some(hotkey_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);
        
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Listener"),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            None, None, instance, None
        );

        register_all_hotkeys(hwnd);
        
        // Set timer to check updates
        SetTimer(hwnd, 999, 500, None);

        let mut msg = MSG::default();
        loop {
            if GetMessageW(&mut msg, None, 0, 0).into() {
                match msg.message {
                    WM_TIMER => {
                        let app = APP.lock().unwrap();
                        if app.hotkeys_updated {
                            drop(app); // Release lock before unregistering
                            unregister_all_hotkeys(hwnd);
                            register_all_hotkeys(hwnd);
                            
                            let mut app = APP.lock().unwrap();
                            app.hotkeys_updated = false;
                        }
                    }
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
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
                // Decode ID to get preset index
                // ID = (p_idx * 1000) + h_idx + 1
                let preset_idx = ((id - 1) / 1000) as usize;

                if overlay::is_selection_overlay_active_and_dismiss() {
                    return LRESULT(0);
                }
                
                match capture_full_screen() {
                    Ok(img) => {
                        {
                            let mut app = APP.lock().unwrap();
                            app.original_screenshot = Some(img);
                        }
                        std::thread::spawn(move || {
                           overlay::show_selection_overlay(preset_idx); 
                        });
                    },
                    Err(e) => println!("Capture Error: {}", e),
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

        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbitmap = CreateCompatibleBitmap(hdc_screen, width, height);
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
