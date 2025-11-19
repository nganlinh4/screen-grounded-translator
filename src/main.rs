use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::io::{self, Write};
use std::mem::size_of;

use tokio::runtime::Runtime;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::UI::HiDpi::*; // Added for DPI awareness

use serde::{Deserialize, Serialize};
use base64::{Engine as _, engine::general_purpose};
use image::{ImageBuffer, Rgba, ImageFormat, GenericImageView};
use reqwest::Client;
use lazy_static::lazy_static;
use anyhow::Result;

// --- CONFIGURATION ---

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    api_key: String,
    language: String,
    hotkey: u32, // Virtual Key Code
}

// --- API STRUCTS ---

#[derive(Serialize, Deserialize, Debug)]
struct GroqResponse {
    translation: String,
}

// --- APPLICATION STATE ---

struct AppState {
    config: Config,
    client: Client,
    #[allow(dead_code)]
    runtime: Runtime,
    original_screenshot: Option<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    selection_rect: Option<RECT>,
}

lazy_static! {
    static ref APP: Arc<Mutex<AppState>> = Arc::new(Mutex::new(load_app_state()));
}

fn load_app_state() -> AppState {
    let config = load_config();
    AppState {
        config,
        client: Client::new(),
        runtime: Runtime::new().unwrap(),
        original_screenshot: None,
        selection_rect: None,
    }
}

// --- HELPER: STRING TO WIDE (UTF-16) ---
fn to_wstring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// --- CONFIG UTILS ---

fn load_config() -> Config {
    let config_dir = dirs::config_dir().unwrap_or_default().join("screen-grounded-translator");
    let _ = std::fs::create_dir_all(&config_dir);
    let config_file = config_dir.join("config.json");

    if config_file.exists() {
        let data = std::fs::read_to_string(config_file).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or(default_config())
    } else {
        default_config()
    }
}

fn default_config() -> Config {
    Config {
        api_key: "".to_string(),
        language: "Vietnamese".to_string(),
        hotkey: VK_OEM_3.0 as u32, // Tilde key
    }
}

fn save_config(config: &Config) {
    let config_dir = dirs::config_dir().unwrap_or_default().join("screen-grounded-translator");
    let config_file = config_dir.join("config.json");
    let data = serde_json::to_string_pretty(config).unwrap();
    let _ = std::fs::write(config_file, data);
}

fn prompt_user_config() {
    let mut config = load_config();
    println!("\n=== Setup ===");
    
    if config.api_key.is_empty() {
        print!("Enter Groq API Key: ");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        config.api_key = input.trim().to_string();
    }

    print!("Enter Target Language (Default: {}): ", config.language);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    if !input.trim().is_empty() {
        config.language = input.trim().to_string();
    }

    save_config(&config);
    println!("Configuration saved.\n");
}

// --- SCREEN CAPTURE UTILS ---

fn capture_full_screen() -> Result<(ImageBuffer<Rgba<u8>, Vec<u8>>, i32, i32)> {
    unsafe {
        // Capture Virtual Screen (All Monitors)
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
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buffer: Vec<u8> = vec![0; (width * height * 4) as usize];
        GetDIBits(
            hdc_mem,
            hbitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        for chunk in buffer.chunks_exact_mut(4) {
            chunk.swap(0, 2); // Swap B and R
            chunk[3] = 255;   // Ensure alpha is opaque
        }

        DeleteObject(hbitmap);
        DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        let img = ImageBuffer::from_raw(width as u32, height as u32, buffer)
            .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;

        Ok((img, x, y))
    }
}

// --- API LOGIC ---

async fn translate_region(
    api_key: String,
    target_lang: String,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> Result<String> {
    let mut png_data = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png_data), ImageFormat::Png)?;
    let b64_image = general_purpose::STANDARD.encode(&png_data);

    let client = Client::new();
    
    let prompt = format!(
        "Extract text from this image and translate it to {}. \
        You must output valid JSON containing ONLY the key 'translation'. \
        Example: {{ \"translation\": \"Xin chÃ o tháº¿ giá»›i\" }}",
        target_lang
    );

    let payload = serde_json::json!({
        "model": "meta-llama/llama-4-scout-17b-16e-instruct",
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                ]
            }
        ],
        "temperature": 0.1,
        "max_completion_tokens": 1024,
        "response_format": { "type": "json_object" }
    });

    let resp = client.post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await?;

    let status = resp.status();
    let text_resp = resp.text().await?;

    if !status.is_success() {
        return Err(anyhow::anyhow!("API Error ({}): {}", status, text_resp));
    }

    let json_resp: serde_json::Value = serde_json::from_str(&text_resp)
        .map_err(|e| anyhow::anyhow!("Failed to parse API JSON: {}. Body: {}", e, text_resp))?;
        
    let content_str = json_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in response: {}", text_resp))?;

    let groq_resp: GroqResponse = serde_json::from_str(content_str)
        .map_err(|e| anyhow::anyhow!("LLM returned invalid JSON content: {}. Content: {}", e, content_str))?;
    
    Ok(groq_resp.translation)
}

// --- WINDOW PROCEDURES ---

static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static mut SCREEN_OFFSET_X: i32 = 0;
static mut SCREEN_OFFSET_Y: i32 = 0;

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            IS_DRAGGING = true;
            GetCursorPos(std::ptr::addr_of_mut!(START_POS));
            CURR_POS = START_POS;
            SetCapture(hwnd);
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false;
                ReleaseCapture();
                ShowWindow(hwnd, SW_HIDE); 

                let rect = RECT {
                    left: START_POS.x.min(CURR_POS.x),
                    top: START_POS.y.min(CURR_POS.y),
                    right: START_POS.x.max(CURR_POS.x),
                    bottom: START_POS.y.max(CURR_POS.y),
                };

                if (rect.right - rect.left) > 10 && (rect.bottom - rect.top) > 10 {
                    let mut app = APP.lock().unwrap();
                    app.selection_rect = Some(rect);
                    
                    let app_clone = APP.clone();
                    std::thread::spawn(move || process_selection(app_clone, rect));
                } else {
                     PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            let mem_dc = CreateCompatibleDC(hdc);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            let mem_bitmap = CreateCompatibleBitmap(hdc, width, height);
            SelectObject(mem_dc, mem_bitmap);

            // Simulate Dimming
            let brush = CreateSolidBrush(COLORREF(0x00000000)); // Black
            let full_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(mem_dc, &full_rect, brush);
            DeleteObject(brush);

            if IS_DRAGGING {
                let rect = RECT {
                    left: (START_POS.x.min(CURR_POS.x)) - SCREEN_OFFSET_X,
                    top: (START_POS.y.min(CURR_POS.y)) - SCREEN_OFFSET_Y,
                    right: (START_POS.x.max(CURR_POS.x)) - SCREEN_OFFSET_X,
                    bottom: (START_POS.y.max(CURR_POS.y)) - SCREEN_OFFSET_Y,
                };
                
                let frame_brush = CreateSolidBrush(COLORREF(0x00FFFFFF)); // White
                FrameRect(mem_dc, &rect, frame_brush);
                DeleteObject(frame_brush);
            }

            BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY).ok().unwrap();
            
            DeleteObject(mem_bitmap);
            DeleteDC(mem_dc);
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let policy = DWM_SYSTEMBACKDROP_TYPE(2); // Mica
            let _ = DwmSetWindowAttribute(
                hwnd, 
                DWMWA_SYSTEMBACKDROP_TYPE, 
                &policy as *const _ as *const c_void, 
                size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32
            );
            LRESULT(0)
        }
        WM_RBUTTONUP | WM_LBUTTONUP => {
            DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);

            let brush = CreateSolidBrush(COLORREF(0x00101010)); 
            FillRect(hdc, &rect, brush);
            DeleteObject(brush);

            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FFFFFF));
            
            let hfont = CreateFontW(
                20, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, 
                DEFAULT_CHARSET.0 as u32, 
                OUT_DEFAULT_PRECIS.0 as u32, 
                CLIP_DEFAULT_PRECIS.0 as u32, 
                CLEARTYPE_QUALITY.0 as u32, 
                (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, 
                w!("Segoe UI")
            );
            SelectObject(hdc, hfont);

            let len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; len as usize];
            GetWindowTextW(hwnd, &mut buf);
            
            let mut draw_rect = rect;
            draw_rect.left += 10;
            draw_rect.top += 10;
            draw_rect.right -= 10;
            draw_rect.bottom -= 10;
            
            DrawTextW(hdc, &mut buf, &mut draw_rect, DT_LEFT | DT_WORDBREAK);

            DeleteObject(hfont);
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// --- LOGIC CONTROLLERS ---

fn process_selection(app: Arc<Mutex<AppState>>, rect: RECT) {
    let (img, config, _runtime, _client) = {
        let guard = app.lock().unwrap();
        (
            guard.original_screenshot.clone().unwrap(),
            guard.config.clone(),
            (), 
            guard.client.clone()
        )
    };

    let x_virt = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y_virt = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };

    let x_rel = rect.left - x_virt;
    let y_rel = rect.top - y_virt;

    let w = (rect.right - rect.left).abs() as u32;
    let h = (rect.bottom - rect.top).abs() as u32;
    let img_w = img.width();
    let img_h = img.height();

    let crop_x = x_rel.max(0) as u32;
    let crop_y = y_rel.max(0) as u32;
    let crop_w = w.min(img_w.saturating_sub(crop_x));
    let crop_h = h.min(img_h.saturating_sub(crop_y));

    if crop_w == 0 || crop_h == 0 {
        println!("Invalid selection area");
        return;
    }
    
    let cropped = img.view(crop_x, crop_y, crop_w, crop_h).to_image();

    // --- DEBUG: SAVE IMAGE ---
    println!("Debug: Saving captured region to 'debug_crop.png'");
    let _ = cropped.save("debug_crop.png");
    // -------------------------

    let rt = Runtime::new().unwrap();
    let translation_res = rt.block_on(translate_region(config.api_key, config.language, cropped));

    match translation_res {
        Ok(text) => {
            if text.trim().is_empty() { return; }
            std::thread::spawn(move || {
                show_result_window(rect, text);
            });
        }
        Err(e) => {
            println!("Translation Error: {}", e);
        }
    }
}

fn show_selection_overlay() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SnippingOverlay");
        
        let wc = WNDCLASSW {
            lpfnWndProc: Some(selection_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap(),
            lpszClassName: class_name,
            hbrBackground: CreateSolidBrush(COLORREF(0x00000000)),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        
        SCREEN_OFFSET_X = x;
        SCREEN_OFFSET_Y = y;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("Snipping"),
            WS_POPUP | WS_VISIBLE,
            x, y, w, h,
            None, None, instance, None
        );

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 128, LWA_ALPHA); 

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_CLOSE { break; }
        }
        
        UnregisterClassW(class_name, instance);
    }
}

fn show_result_window(target_rect: RECT, text: String) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        let wc = WNDCLASSW {
            lpfnWndProc: Some(result_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            lpszClassName: class_name,
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Calculate required height
        let width = (target_rect.right - target_rect.left).abs();
        let width = width.max(200); 
        
        let mut calc_rect = RECT { left: 0, top: 0, right: width, bottom: 0 };
        
        let hdc = CreateCompatibleDC(None);
        let hfont = CreateFontW(
            20, 0, 0, 0, FW_MEDIUM.0 as i32, 0, 0, 0, 
            DEFAULT_CHARSET.0 as u32, 
            OUT_DEFAULT_PRECIS.0 as u32, 
            CLIP_DEFAULT_PRECIS.0 as u32, 
            CLEARTYPE_QUALITY.0 as u32, 
            (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, 
            w!("Segoe UI")
        );
        SelectObject(hdc, hfont);
        let text_wide = to_wstring(&text);
        DrawTextW(hdc, &mut text_wide.clone(), &mut calc_rect, DT_CALCRECT | DT_WORDBREAK);
        DeleteObject(hfont);
        DeleteDC(hdc);

        let height = (calc_rect.bottom - calc_rect.top) + 30;

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED,
            class_name,
            PCWSTR(text_wide.as_ptr()),
            WS_POPUP | WS_VISIBLE,
            target_rect.left, 
            target_rect.top, 
            width, 
            height,
            None, None, instance, None
        );

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 240, LWA_ALPHA);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(hwnd).as_bool() { break; }
        }
        UnregisterClassW(class_name, instance);
    }
}

// --- MAIN ---

fn main() {
    // IMPORTANT: Force DPI Awareness so mouse coords match screen pixels
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    prompt_user_config();
    let config = load_config();
    println!("=== Screen Translator Running ===");
    println!("Press the Hotkey (Default: `) to select a region.");
    println!("If the hotkey does not work outside the terminal, please run this app as Administrator.");
    
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("HotkeyListener");
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
        
        if !RegisterHotKey(hwnd, 1, HOT_KEY_MODIFIERS(0), config.hotkey).as_bool() {
            println!("Error: Failed to register hotkey. Check if another app is using it.");
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn hotkey_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            if wparam.0 == 1 {
                match capture_full_screen() {
                    Ok((img, _x, _y)) => {
                        {
                            let mut app = APP.lock().unwrap();
                            app.original_screenshot = Some(img);
                        }
                        std::thread::spawn(|| {
                           show_selection_overlay(); 
                        });
                    },
                    Err(e) => println!("Capture failed: {}", e)
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}