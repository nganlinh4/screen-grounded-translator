use eframe::egui;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIcon, FindWindowW, GetSystemMetrics, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON,
    SM_CYICON, SM_CYSMICON, WM_SETICON,
};
use windows_core::BOOL;

// --- Monitor Enumeration (Existing Code) ---

struct MonitorEnumContext {
    monitors: Vec<String>,
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _lprc: *mut RECT,
    dwdata: LPARAM,
) -> BOOL {
    let context = &mut *(dwdata.0 as *mut MonitorEnumContext);
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

    if GetMonitorInfoW(hmonitor, &mut mi as *mut _ as *mut _).as_bool() {
        let device_name = String::from_utf16_lossy(&mi.szDevice);
        let trimmed_name = device_name.trim_matches(char::from(0)).to_string();
        context.monitors.push(trimmed_name);
    }
    BOOL::from(true)
}

pub fn get_monitor_names() -> Vec<String> {
    let mut ctx = MonitorEnumContext {
        monitors: Vec::new(),
    };
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut ctx as *mut _ as isize),
        );
    }
    ctx.monitors
}

// --- Clipboard Helper (Existing Code) ---
pub fn copy_to_clipboard_text(text: &str) {
    crate::overlay::utils::copy_to_clipboard(text, HWND::default());
}

// --- Admin Check (Existing Code) ---

#[cfg(target_os = "windows")]
pub fn is_running_as_admin() -> bool {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut h_token = HANDLE::default();

        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut h_token).is_ok() {
            let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
            let mut return_length: u32 = 0;
            let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;

            if GetTokenInformation(
                h_token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
                size,
                &mut return_length,
            )
            .is_ok()
            {
                return elevation.TokenIsElevated != 0;
            }
        }
        false
    }
}

// --- System Theme Detection ---
pub fn is_system_in_dark_mode() -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        // We check "SystemUsesLightTheme".
        // 0 = Dark Mode (Standard), 1 = Light Mode.
        if let Ok(key) =
            hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        {
            if let Ok(val) = key.get_value::<u32, &str>("SystemUsesLightTheme") {
                return val == 0;
            }
        }
        true
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

// --- Font Configuration (Existing Code) ---

pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let gs_font_name = "google_sans_flex";
    // Use large byte array from centralized assets
    let gs_data = crate::assets::GOOGLE_SANS_FLEX;
    fonts.font_data.insert(
        gs_font_name.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(gs_data)),
    );

    let viet_font_name = "segoe_ui";

    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    let font_dir = std::path::Path::new(&windir).join("Fonts");

    let viet_font_path = font_dir.join("segoeui.ttf");
    let viet_fallback_path = font_dir.join("arial.ttf");
    let viet_data = std::fs::read(&viet_font_path).or_else(|_| std::fs::read(&viet_fallback_path));

    let korean_font_name = "malgun_gothic";
    let korean_font_path = font_dir.join("malgun.ttf");
    let korean_data = std::fs::read(&korean_font_path);

    if let Ok(data) = viet_data {
        fonts.font_data.insert(
            viet_font_name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            vec.insert(0, viet_font_name.to_owned());
        }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            vec.insert(0, viet_font_name.to_owned());
        }
    }
    if let Ok(data) = korean_data {
        fonts.font_data.insert(
            korean_font_name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            let idx = if vec.contains(&viet_font_name.to_string()) {
                1
            } else {
                0
            };
            vec.insert(idx, korean_font_name.to_owned());
        }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            let idx = if vec.contains(&viet_font_name.to_string()) {
                1
            } else {
                0
            };
            vec.insert(idx, korean_font_name.to_owned());
        }
    }

    // Force Google Sans Flex to front
    if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        vec.insert(0, gs_font_name.to_owned());
    }

    ctx.set_fonts(fonts);
}

// --- Task Scheduler / Admin Startup (Existing Code) ---

const TASK_NAME: &str = "ScreenGoatedToolbox_AutoStart";

pub fn set_admin_startup(enable: bool) -> bool {
    if enable {
        let exe_path = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => return false,
        };

        let exe_str = match exe_path.to_str() {
            Some(s) => s,
            None => return false,
        };

        if exe_str.is_empty() {
            return false;
        }

        let mut cmd = Command::new("schtasks");
        cmd.args(&[
            "/create",
            "/tn",
            TASK_NAME,
            "/tr",
            &format!("\"{}\"", exe_str),
            "/sc",
            "onlogon",
            "/rl",
            "highest",
            "/f",
        ]);
        // CREATE_NO_WINDOW = 0x08000000 - prevents console window flash
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);

        let output = cmd.output();

        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    } else {
        let mut cmd = Command::new("schtasks");
        cmd.args(&["/delete", "/tn", TASK_NAME, "/f"]);
        // CREATE_NO_WINDOW = 0x08000000 - prevents console window flash
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);

        let output = cmd.output();

        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
}

pub fn is_admin_startup_enabled() -> bool {
    let mut cmd = Command::new("schtasks");
    cmd.args(&["/query", "/tn", TASK_NAME]);
    // CREATE_NO_WINDOW = 0x08000000 - prevents console window flash
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    let output = cmd.output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

// --- NATIVE ICON UPDATER (FIXED) ---

fn rgba_to_bgra(data: &[u8]) -> Vec<u8> {
    let mut bgra = data.to_vec();
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2); // Swap R and B
    }
    bgra
}

unsafe fn create_hicon_from_bytes(bytes: &[u8], target_w: i32, target_h: i32) -> Option<HANDLE> {
    let img = image::load_from_memory(bytes).ok()?;

    // High-quality resize to fix aliasing
    let resized = img.resize_exact(
        target_w as u32,
        target_h as u32,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8();
    let bgra_data = rgba_to_bgra(rgba.as_raw());

    let mask_len = ((target_w * target_h) / 8) as usize;
    let mask_bits = vec![0u8; mask_len];

    // Fixed: CreateIcon returns a Result<HICON> in windows 0.48+
    let hicon_result = CreateIcon(
        None,
        target_w,
        target_h,
        1,
        32,
        mask_bits.as_ptr(),
        bgra_data.as_ptr(),
    );

    match hicon_result {
        Ok(hicon) => {
            // Fixed: Unwrap HICON and cast to HANDLE - in windows 0.62 HICON wraps *mut c_void
            if hicon.is_invalid() {
                None
            } else {
                Some(std::mem::transmute::<_, HANDLE>(hicon))
            }
        }
        Err(_) => None,
    }
}

pub fn set_window_icon(hwnd: HWND, is_dark_mode: bool) {
    let icon_bytes: &[u8] = if is_dark_mode {
        include_bytes!("../../assets/app-icon-small.png")
    } else {
        include_bytes!("../../assets/app-icon-small-light.png")
    };

    unsafe {
        if !hwnd.is_invalid() {
            let small_w = GetSystemMetrics(SM_CXSMICON);
            let small_h = GetSystemMetrics(SM_CYSMICON);

            let big_w = GetSystemMetrics(SM_CXICON);
            let big_h = GetSystemMetrics(SM_CYICON);

            if let Some(hicon_small) = create_hicon_from_bytes(icon_bytes, small_w, small_h) {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_SETICON,
                    WPARAM(ICON_SMALL as usize),
                    LPARAM(hicon_small.0 as isize),
                );
            }

            if let Some(hicon_big) = create_hicon_from_bytes(icon_bytes, big_w, big_h) {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_SETICON,
                    WPARAM(ICON_BIG as usize),
                    LPARAM(hicon_big.0 as isize),
                );
            }
        }
    }
}

pub fn update_window_icon_native(is_dark_mode: bool) {
    unsafe {
        let class_name = w!("eframe");
        let title_name = w!("Screen Goated Toolbox (SGT by nganlinh4)");

        let mut hwnd = FindWindowW(class_name, title_name).unwrap_or_default();

        if hwnd.is_invalid() {
            hwnd = FindWindowW(None, title_name).unwrap_or_default();
        }

        set_window_icon(hwnd, is_dark_mode);
    }
}
