use anyhow::{anyhow, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

// Helper function to download file or read local file
pub fn download_file(
    url: &str,
    path: &Path,
    stop_signal: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    use std::io::Write;

    println!("Downloading file from: {}", url);
    let response = ureq::get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .call()
        .map_err(|e| anyhow!("Download failed: {}", e))?;

    let total_size = response
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = response.into_body().into_reader();
    let mut file = fs::File::create(path)?;

    let mut buffer = [0; 8192];
    let mut downloaded: u64 = 0;

    let update_interval = std::time::Duration::from_millis(100);
    let mut last_update = std::time::Instant::now();

    loop {
        if stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = fs::remove_file(path);
            return Err(anyhow!("Download cancelled"));
        }

        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read])?;
        downloaded += bytes_read as u64;

        if total_size > 0 && last_update.elapsed() >= update_interval {
            let progress = (downloaded as f32 / total_size as f32) * 100.0;
            use crate::overlay::realtime_webview::state::REALTIME_STATE;
            if let Ok(mut state) = REALTIME_STATE.lock() {
                state.download_progress = progress;
            }
            last_update = std::time::Instant::now();

            use super::WM_DOWNLOAD_PROGRESS;
            use crate::overlay::realtime_webview::state::REALTIME_HWND;
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

            unsafe {
                if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
                    let _ = PostMessageW(
                        Some(REALTIME_HWND),
                        WM_DOWNLOAD_PROGRESS,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    }

    Ok(())
}

pub fn get_parakeet_model_dir() -> PathBuf {
    let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(".gemini");
    path.push("parakeet-model");
    path
}

pub fn is_model_downloaded() -> bool {
    let dir = get_parakeet_model_dir();
    dir.join("encoder.onnx").exists()
        && dir.join("decoder_joint.onnx").exists()
        && dir.join("tokenizer.json").exists()
}

pub fn download_parakeet_model(
    stop_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    let dir = get_parakeet_model_dir();

    use crate::overlay::realtime_webview::state::REALTIME_STATE;
    if let Ok(mut state) = REALTIME_STATE.lock() {
        state.is_downloading = true;
        state.download_title = "Downloading Parakeet (0.6 GB)".to_string();
        state.download_message = "Please wait...".to_string();
        state.download_progress = 0.0;
    }

    use super::WM_DOWNLOAD_PROGRESS;
    use crate::overlay::realtime_webview::state::REALTIME_HWND;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    unsafe {
        if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
            let _ = PostMessageW(
                Some(REALTIME_HWND),
                WM_DOWNLOAD_PROGRESS,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    let result = (|| {
        let files_to_download = vec![
             ("encoder.onnx", "https://huggingface.co/altunenes/parakeet-rs/resolve/main/realtime_eou_120m-v1-onnx/encoder.onnx"),
             ("decoder_joint.onnx", "https://huggingface.co/altunenes/parakeet-rs/resolve/main/realtime_eou_120m-v1-onnx/decoder_joint.onnx"),
             ("tokenizer.json", "https://huggingface.co/altunenes/parakeet-rs/resolve/main/realtime_eou_120m-v1-onnx/tokenizer.json"),
        ];

        for (filename, url) in files_to_download {
            if let Ok(mut state) = REALTIME_STATE.lock() {
                state.download_message = format!("Downloading {}...", filename);
            }
            unsafe {
                if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
                    let _ = PostMessageW(
                        Some(REALTIME_HWND),
                        WM_DOWNLOAD_PROGRESS,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }

            download_file(url, &dir.join(filename), &stop_signal)?;
        }

        Ok(())
    })();

    if let Ok(mut state) = REALTIME_STATE.lock() {
        state.is_downloading = false;
    }
    unsafe {
        if !std::ptr::addr_of!(REALTIME_HWND).read().is_invalid() {
            let _ = PostMessageW(
                Some(REALTIME_HWND),
                WM_DOWNLOAD_PROGRESS,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    result
}
