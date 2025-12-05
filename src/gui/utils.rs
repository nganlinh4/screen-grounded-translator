use windows::Win32::Foundation::{BOOL, LPARAM, RECT, HWND};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR, GetMonitorInfoW, MONITORINFOEXW};
use eframe::egui;
use std::process::Command;

// --- Monitor Enumeration ---

struct MonitorEnumContext {
    monitors: Vec<String>,
}

unsafe extern "system" fn monitor_enum_proc(hmonitor: HMONITOR, _hdc: HDC, _lprc: *mut RECT, dwdata: LPARAM) -> BOOL {
    let context = &mut *(dwdata.0 as *mut MonitorEnumContext);
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    
    if GetMonitorInfoW(hmonitor, &mut mi as *mut _ as *mut _).as_bool() {
        let device_name = String::from_utf16_lossy(&mi.szDevice);
        let trimmed_name = device_name.trim_matches(char::from(0)).to_string();
        context.monitors.push(trimmed_name);
    }
    BOOL(1)
}

pub fn get_monitor_names() -> Vec<String> {
    let mut ctx = MonitorEnumContext { monitors: Vec::new() };
    unsafe {
        EnumDisplayMonitors(HDC(0), None, Some(monitor_enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.monitors
}

// --- Clipboard Helper ---
pub fn copy_to_clipboard_text(text: &str) {
    crate::overlay::utils::copy_to_clipboard(text, HWND(0));
}

// --- Admin Check ---

#[cfg(target_os = "windows")]
pub fn is_running_as_admin() -> bool {
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION};
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::Win32::Foundation::HANDLE;
    
    unsafe {
        let mut h_token = HANDLE::default();
        
        // Use raw windows API - ctypes compatible
        extern "system" {
            fn OpenProcessToken(
                ProcessHandle: HANDLE,
                DesiredAccess: u32,
                TokenHandle: *mut HANDLE,
            ) -> windows::Win32::Foundation::BOOL;
        }
        
        const TOKEN_READ: u32 = 0x20008;
        
        if OpenProcessToken(GetCurrentProcess(), TOKEN_READ, &mut h_token).as_bool() {
            let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
            let mut return_length: u32 = 0;
            let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;

            if GetTokenInformation(
                h_token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
                size,
                &mut return_length
            ).as_bool() {
                 return elevation.TokenIsElevated != 0;
            }
        }
        false
    }
}

// --- Font Configuration ---

pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let viet_font_name = "segoe_ui";
    
    // Dynamic Windows font path
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    let font_dir = std::path::Path::new(&windir).join("Fonts");
    
    let viet_font_path = font_dir.join("segoeui.ttf");
    let viet_fallback_path = font_dir.join("arial.ttf");
    let viet_data = std::fs::read(&viet_font_path).or_else(|_| std::fs::read(&viet_fallback_path));

    let korean_font_name = "malgun_gothic";
    let korean_font_path = font_dir.join("malgun.ttf");
    let korean_data = std::fs::read(&korean_font_path);

    if let Ok(data) = viet_data {
        fonts.font_data.insert(viet_font_name.to_owned(), egui::FontData::from_owned(data));
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) { vec.insert(0, viet_font_name.to_owned()); }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) { vec.insert(0, viet_font_name.to_owned()); }
    }
    if let Ok(data) = korean_data {
        fonts.font_data.insert(korean_font_name.to_owned(), egui::FontData::from_owned(data));
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) { 
            let idx = if vec.contains(&viet_font_name.to_string()) { 1 } else { 0 };
            vec.insert(idx, korean_font_name.to_owned()); 
        }
        if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) { 
             let idx = if vec.contains(&viet_font_name.to_string()) { 1 } else { 0 };
             vec.insert(idx, korean_font_name.to_owned()); 
        }
    }
    ctx.set_fonts(fonts);
}

// --- Task Scheduler / Admin Startup ---

const TASK_NAME: &str = "ScreenGoatedToolbox_AutoStart";

/// Create or delete a Windows Scheduled Task for Admin auto-startup
pub fn set_admin_startup(enable: bool) -> bool {
    if enable {
        // Get current executable path
        let exe_path = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => return false,
        };
        
        let exe_str = match exe_path.to_str() {
            Some(s) => s,
            None => return false,
        };
        
        if exe_str.is_empty() { return false; }

        // Create Scheduled Task via schtasks
        // /SC ONLOGON : Trigger at user login
        // /RL HIGHEST : Run with highest privileges (Admin)
        // /F          : Force overwrite if exists
        // /TN         : Task Name
        // /TR         : Task Run (path to executable)
        let output = Command::new("schtasks")
            .args(&[
                "/create",
                "/tn", TASK_NAME,
                "/tr", &format!("\"{}\"", exe_str),
                "/sc", "onlogon",
                "/rl", "highest",
                "/f"
            ])
            .output();

        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    } else {
        // Delete the scheduled task
        let output = Command::new("schtasks")
            .args(&["/delete", "/tn", TASK_NAME, "/f"])
            .output();
            
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
}

/// Check if the admin auto-startup task exists
#[allow(dead_code)]
pub fn is_admin_startup_enabled() -> bool {
    let output = Command::new("schtasks")
        .args(&["/query", "/tn", TASK_NAME])
        .output();
        
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}
