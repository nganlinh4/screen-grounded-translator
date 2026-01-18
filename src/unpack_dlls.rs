use std::env;
use std::fs;

/// Unpacks embedded DLLs to the current executable directory if they are missing.
/// This allows the application to run on systems without the VC++ Redistributable installed.
pub fn unpack_dlls() {
    // We embed the DLLs using include_bytes!
    // These are placed in src/embed_dlls/ by the build process or manually.
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

    let exe_path = env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new("."));

    for (name, bytes) in dlls {
        let path = exe_dir.join(name);
        // Only write if missing to avoid unnecessary disk IO
        if !path.exists() {
            let _ = fs::write(&path, bytes);
        }
    }
}
