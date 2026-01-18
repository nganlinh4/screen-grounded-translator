use std::fs;
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::System::LibraryLoader::SetDllDirectoryW;

/// Unpacks embedded DLLs to the local app data directory if they are missing.
/// This avoids cluttering the folder where the EXE is located.
pub fn unpack_dlls() {
    // Determine the path to %LOCALAPPDATA%/screen-goated-toolbox/bin
    let mut bin_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    bin_dir.push("screen-goated-toolbox");
    bin_dir.push("bin");

    // Ensure the directory exists
    let _ = fs::create_dir_all(&bin_dir);

    // We embed the DLLs using include_bytes!
    let dlls: &[(&str, &[u8])] = &[
        (
            "vcruntime140.dll",
            include_bytes!("embed_dlls/vcruntime140.dll"),
        ),
        (
            "vcruntime140_1.dll",
            include_bytes!("embed_dlls/vcruntime140_1.dll"),
        ),
        ("msvcp140.dll", include_bytes!("embed_dlls/msvcp140.dll")),
        (
            "msvcp140_1.dll",
            include_bytes!("embed_dlls/msvcp140_1.dll"),
        ),
        ("DirectML.dll", include_bytes!("embed_dlls/DirectML.dll")),
    ];

    for (name, bytes) in dlls {
        let path = bin_dir.join(name);
        // Only write if missing to avoid unnecessary disk IO
        if !path.exists() {
            let _ = fs::write(&path, bytes);
        }
    }

    // Tell Windows to look in our private bin directory for DLLs (for the current process)
    unsafe {
        let path_wide: Vec<u16> = bin_dir
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = SetDllDirectoryW(windows::core::PCWSTR(path_wide.as_ptr()));
    }

    // Add to PATH so child processes (like WebView2 helpers) can also find them
    if let Ok(current_path) = std::env::var("PATH") {
        let new_path = format!("{};{}", bin_dir.to_string_lossy(), current_path);
        std::env::set_var("PATH", new_path);
    }

    crate::log_info!("[Unpacker] DLLs verified/unpacked to {:?}", bin_dir);
}
