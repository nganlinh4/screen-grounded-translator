use super::types::{CookieBrowser, DownloadState, DownloadType, InstallStatus, UpdateStatus};
use super::utils::{download_file, extract_ffmpeg, log};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

use super::DownloadManager;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

impl DownloadManager {
    pub fn check_status(&self) {
        let bin = self.bin_dir.clone();
        let ffmpeg_s = self.ffmpeg_status.clone();
        let ytdlp_s = self.ytdlp_status.clone();
        let logs = self.logs.clone();

        thread::spawn(move || {
            if !bin.exists() {
                let _ = fs::create_dir_all(&bin);
            }

            // Check yt-dlp
            let ytdlp_path = bin.join("yt-dlp.exe");
            if ytdlp_path.exists() {
                *ytdlp_s.lock().unwrap() = InstallStatus::Installed;
            } else {
                *ytdlp_s.lock().unwrap() = InstallStatus::Missing;
                log(&logs, "yt-dlp missing");
            }

            // Check ffmpeg
            let ffmpeg_path = bin.join("ffmpeg.exe");
            if ffmpeg_path.exists() {
                *ffmpeg_s.lock().unwrap() = InstallStatus::Installed;
            } else {
                *ffmpeg_s.lock().unwrap() = InstallStatus::Missing;
                log(&logs, "ffmpeg missing");
            }

            // Cleanup any partial downloads (.tmp files)
            if let Ok(entries) = fs::read_dir(&bin) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |ext| ext == "tmp") {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        });
    }

    pub fn check_updates(&self) {
        if self.is_checking_updates.load(Ordering::Relaxed) {
            return;
        }
        self.is_checking_updates.store(true, Ordering::Relaxed);

        let bin = self.bin_dir.clone();
        let ytdlp_status_store = self.ytdlp_update_status.clone();
        let ffmpeg_status_store = self.ffmpeg_update_status.clone();
        let ytdlp_ver = self.ytdlp_version.clone();
        let ffmpeg_ver = self.ffmpeg_version.clone();
        let logs = self.logs.clone();
        let ytdlp_install = self.ytdlp_status.clone();
        let ffmpeg_install = self.ffmpeg_status.clone();
        let checking_flag = self.is_checking_updates.clone();

        thread::spawn(move || {
            log(&logs, "Checking for updates...");

            // Set Checking
            *ytdlp_status_store.lock().unwrap() = UpdateStatus::Checking;
            *ffmpeg_status_store.lock().unwrap() = UpdateStatus::Checking;

            // 1. Check yt-dlp
            // Only if installed
            let mut check_ytdlp = false;
            {
                let s = ytdlp_install.lock().unwrap();
                if *s == InstallStatus::Installed {
                    check_ytdlp = true;
                } else {
                    *ytdlp_status_store.lock().unwrap() = UpdateStatus::Idle;
                }
            }
            if check_ytdlp {
                let ytdlp_path = bin.join("yt-dlp.exe");
                let output = std::process::Command::new(&ytdlp_path)
                    .arg("--version")
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();

                if let Ok(out) = output {
                    let local_ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    *ytdlp_ver.lock().unwrap() = Some(local_ver.clone());

                    // Fetch latest
                    let _resp = ureq::get(
                        "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest",
                    )
                    .header("User-Agent", "Mozilla/5.0")
                    .call();

                    if let Ok(r) = ureq::get(
                        "https://api.github.com/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest",
                    )
                    .header("User-Agent", "ScreenGoatedToolbox")
                    .call()
                    {
                        if let Ok(json_str) = r.into_body().read_to_string() {
                            // Manual JSON parse for "tag_name"
                            if let Some(pos) = json_str.find("\"tag_name\"") {
                                let sub = &json_str[pos..];
                                if let Some(colon) = sub.find(':') {
                                    if let Some(quote1) = sub[colon..].find('"') {
                                        let start = colon + quote1 + 1;
                                        if let Some(quote2) = sub[start..].find('"') {
                                            let remote_ver = &sub[start..start + quote2];
                                            log(
                                                &logs,
                                                format!(
                                                    "yt-dlp: local={}, remote={}",
                                                    local_ver, remote_ver
                                                ),
                                            );
                                            if remote_ver != local_ver && !remote_ver.is_empty() {
                                                *ytdlp_status_store.lock().unwrap() =
                                                    UpdateStatus::UpdateAvailable(
                                                        remote_ver.to_string(),
                                                    );
                                            } else {
                                                *ytdlp_status_store.lock().unwrap() =
                                                    UpdateStatus::UpToDate;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 2. Check ffmpeg
            let mut check_ffmpeg = false;
            {
                let s = ffmpeg_install.lock().unwrap();
                if *s == InstallStatus::Installed {
                    check_ffmpeg = true;
                } else {
                    *ffmpeg_status_store.lock().unwrap() = UpdateStatus::Idle;
                }
            }
            if check_ffmpeg {
                let ffmpeg_path = bin.join("ffmpeg.exe");
                let output = std::process::Command::new(&ffmpeg_path)
                    .arg("-version")
                    .creation_flags(0x08000000)
                    .output();

                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    // Line 1: ffmpeg version 6.1.1-essentials...
                    if let Some(line) = stdout.lines().next() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 3 && parts[0] == "ffmpeg" && parts[1] == "version" {
                            let ver_chunk = parts[2]; // 6.1.1-essentials...
                                                      // Extract just the version number (digits and dots)
                            let local_ver: String = ver_chunk
                                .chars()
                                .take_while(|c| c.is_ascii_digit() || *c == '.')
                                .collect();
                            *ffmpeg_ver.lock().unwrap() = Some(local_ver.clone());

                            // Fetch remote
                            if let Ok(r) =
                                ureq::get("https://www.gyan.dev/ffmpeg/builds/release-version")
                                    .header("User-Agent", "ScreenGoatedToolbox")
                                    .call()
                            {
                                if let Ok(remote_ver) = r.into_body().read_to_string() {
                                    let remote_ver = remote_ver.trim();
                                    log(
                                        &logs,
                                        format!(
                                            "ffmpeg: local={}, remote={}",
                                            local_ver, remote_ver
                                        ),
                                    );
                                    if remote_ver != local_ver && !remote_ver.is_empty() {
                                        *ffmpeg_status_store.lock().unwrap() =
                                            UpdateStatus::UpdateAvailable(remote_ver.to_string());
                                    } else {
                                        *ffmpeg_status_store.lock().unwrap() =
                                            UpdateStatus::UpToDate;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            checking_flag.store(false, Ordering::Relaxed);
            log(&logs, "Update check complete.");
        });
    }

    pub fn get_dependency_sizes(&self) -> (String, String) {
        let ytdlp_path = self.bin_dir.join("yt-dlp.exe");
        let ffmpeg_path = self.bin_dir.join("ffmpeg.exe");

        let size_to_string = |path: PathBuf| -> String {
            if let Ok(metadata) = fs::metadata(path) {
                let size_mb = metadata.len() as f64 / 1024.0 / 1024.0;
                format!("{:.1} MB", size_mb)
            } else {
                "0 MB".to_string()
            }
        };

        (size_to_string(ytdlp_path), size_to_string(ffmpeg_path))
    }

    pub fn delete_dependencies(&self) {
        let ytdlp_path = self.bin_dir.join("yt-dlp.exe");
        let ffmpeg_path = self.bin_dir.join("ffmpeg.exe");

        let _ = fs::remove_file(ytdlp_path);
        let _ = fs::remove_file(ffmpeg_path);

        // Reset status
        *self.ytdlp_status.lock().unwrap() = InstallStatus::Missing;
        *self.ffmpeg_status.lock().unwrap() = InstallStatus::Missing;
    }

    pub fn cancel_download(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn change_download_folder(&mut self) {
        // PowerShell hack to open folder picker
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(&["-Command", "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.ShowDialog() | Out-Null; $f.SelectedPath"]);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);

        let output = cmd.output();

        if let Ok(out) = output {
            if let Ok(path) = String::from_utf8(out.stdout) {
                let path = path.trim().to_string();
                if !path.is_empty() {
                    self.custom_download_path = Some(PathBuf::from(path));
                    self.save_settings();
                }
            }
        }
    }

    pub fn start_download_ytdlp(&self) {
        let bin = self.bin_dir.clone();
        let status = self.ytdlp_status.clone();
        let update_status = self.ytdlp_update_status.clone();
        let logs = self.logs.clone();
        let cancel = self.cancel_flag.clone();
        // Cannot pass self_clone easily to thread as DownloadManager is not Clone-able or meant to be?
        // Actually DownloadManager is not Clone. But we need to call check_updates.
        // check_updates uses Arc fields. We can separate the check logic or just reset status to Idle and let user check again?
        // Better: Reset status to Checking and spawn a delayed check.

        // Wait, self.check_updates() is on &self. The struct has Arcs.
        // We can't nicely call methods from the thread if we don't own self.
        // But we can reset update_status to Idle.

        let bin_clone = bin.clone();

        // We will need to re-run the check logic manually or extract it.
        // For simplicity: Clear version info and Reset update status to Idle so "Check Update" button appears.
        // Even better: Set it to UpToDate if we trust the download.
        // But version string needs update.
        // Let's just set to Idle, so user clicks "Check Update" or we simulate it.
        // Actually, let's explicitly run the version check part for ytdlp here again.

        let ytdlp_ver_store = self.ytdlp_version.clone();

        {
            let mut s = status.lock().unwrap();
            if matches!(
                *s,
                InstallStatus::Downloading(_) | InstallStatus::Extracting
            ) {
                return;
            }
            *s = InstallStatus::Downloading(0.0);
            cancel.store(false, Ordering::Relaxed);
        }

        thread::spawn(move || {
            let url = "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest/download/yt-dlp.exe";
            log(&logs, format!("Starting download: {}", url));

            match download_file(url, &bin.join("yt-dlp.exe"), &status, &cancel) {
                Ok(_) => {
                    *status.lock().unwrap() = InstallStatus::Installed;
                    log(&logs, "yt-dlp installed successfully");
                    *update_status.lock().unwrap() = UpdateStatus::Idle;

                    // Update version string locally
                    #[cfg(target_os = "windows")]
                    use std::os::windows::process::CommandExt;
                    let output = std::process::Command::new(bin_clone.join("yt-dlp.exe"))
                        .arg("--version")
                        .creation_flags(0x08000000)
                        .output();
                    if let Ok(out) = output {
                        let local_ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        *ytdlp_ver_store.lock().unwrap() = Some(local_ver);
                    }
                }
                Err(e) => {
                    *status.lock().unwrap() = InstallStatus::Error(e.clone());
                    log(&logs, format!("yt-dlp error: {}", e));
                }
            }
        });
    }

    pub fn start_download_ffmpeg(&self) {
        let bin = self.bin_dir.clone();
        let status = self.ffmpeg_status.clone();
        let update_status = self.ffmpeg_update_status.clone();
        let logs = self.logs.clone();
        let cancel = self.cancel_flag.clone();
        let app_bin = bin.clone();
        let ffmpeg_ver_store = self.ffmpeg_version.clone();

        {
            let mut s = status.lock().unwrap();
            if matches!(
                *s,
                InstallStatus::Downloading(_) | InstallStatus::Extracting
            ) {
                return;
            }
            *s = InstallStatus::Downloading(0.0);
            cancel.store(false, Ordering::Relaxed);
        }

        thread::spawn(move || {
            let url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
            log(&logs, format!("Starting download: {}", url));

            let zip_path = bin.join("ffmpeg.zip");
            match download_file(url, &zip_path, &status, &cancel) {
                Ok(_) => {
                    log(&logs, "Download complete. Extracting...");
                    *status.lock().unwrap() = InstallStatus::Extracting;

                    if cancel.load(Ordering::Relaxed) {
                        *status.lock().unwrap() = InstallStatus::Error("Cancelled".to_string());
                        return;
                    }

                    match extract_ffmpeg(&zip_path, &bin) {
                        Ok(_) => {
                            *status.lock().unwrap() = InstallStatus::Installed;
                            log(&logs, "ffmpeg installed successfully");
                            let _ = fs::remove_file(zip_path); // Cleanup
                            *update_status.lock().unwrap() = UpdateStatus::Idle;

                            // Update version string
                            #[cfg(target_os = "windows")]
                            use std::os::windows::process::CommandExt;
                            let output = std::process::Command::new(app_bin.join("ffmpeg.exe"))
                                .arg("-version")
                                .creation_flags(0x08000000)
                                .output();

                            if let Ok(out) = output {
                                let stdout = String::from_utf8_lossy(&out.stdout);
                                if let Some(line) = stdout.lines().next() {
                                    let parts: Vec<&str> = line.split_whitespace().collect();
                                    if parts.len() >= 3
                                        && parts[0] == "ffmpeg"
                                        && parts[1] == "version"
                                    {
                                        let ver_chunk = parts[2];
                                        let local_ver: String = ver_chunk
                                            .chars()
                                            .take_while(|c| c.is_ascii_digit() || *c == '.')
                                            .collect();
                                        *ffmpeg_ver_store.lock().unwrap() = Some(local_ver);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            *status.lock().unwrap() = InstallStatus::Error(e.clone());
                            log(&logs, format!("Extract error: {}", e));
                        }
                    }
                }
                Err(e) => {
                    *status.lock().unwrap() = InstallStatus::Error(e.clone());
                    log(&logs, format!("ffmpeg download error: {}", e));
                }
            }
        });
    }

    pub fn start_analysis(&mut self) {
        let url = self.input_url.trim().to_string();
        if url.is_empty() {
            return;
        }

        let bin_dir = self.bin_dir.clone();
        let cookie_browser = self.cookie_browser.clone();
        let formats_clone = self.available_formats.clone();
        let is_analyzing = self.is_analyzing.clone();
        let error_clone = self.analysis_error.clone();

        self.last_url_analyzed = url.clone();
        *is_analyzing.lock().unwrap() = true;
        *error_clone.lock().unwrap() = None;

        // Clear previous formats
        formats_clone.lock().unwrap().clear();
        self.selected_format = None;

        use super::utils::fetch_video_formats;

        thread::spawn(
            move || match fetch_video_formats(&url, &bin_dir, cookie_browser) {
                Ok(formats) => {
                    *formats_clone.lock().unwrap() = formats;
                    *is_analyzing.lock().unwrap() = false;
                }
                Err(e) => {
                    *error_clone.lock().unwrap() = Some(e);
                    *is_analyzing.lock().unwrap() = false;
                }
            },
        );
    }

    pub fn start_media_download(&self, progress_fmt: String) {
        let url = self.input_url.trim().to_string();
        if url.is_empty() {
            return;
        }

        let bin_dir = self.bin_dir.clone();
        let download_type = self.download_type.clone();
        let state = self.download_state.clone();
        let logs = self.logs.clone();

        // Capture advanced flags
        let use_metadata = self.use_metadata;
        let use_sponsorblock = self.use_sponsorblock;
        let use_subtitles = self.use_subtitles;
        let use_playlist = self.use_playlist;
        let cookie_browser = self.cookie_browser.clone();
        let selected_format = self.selected_format.clone();

        let download_path = self
            .custom_download_path
            .clone()
            .unwrap_or_else(|| dirs::download_dir().unwrap_or(PathBuf::from(".")));

        {
            let mut s = state.lock().unwrap();
            if matches!(*s, DownloadState::Downloading(_, _)) {
                return;
            }
            *s = DownloadState::Downloading(0.0, "Starting...".to_string());
        }

        thread::spawn(move || {
            log(&logs, format!("Processing URL: {}", url));
            let ytdlp_exe = bin_dir.join("yt-dlp.exe");

            let mut args = Vec::new();

            // Force UTF-8 output to correctly capture filenames with non-ASCII characters
            args.push("--encoding".to_string());
            args.push("utf-8".to_string());

            // Point to ffmpeg
            args.push("--ffmpeg-location".to_string());
            args.push(bin_dir.to_string_lossy().to_string());

            // Progress per line for potential parsing
            args.push("--newline".to_string());

            if !use_playlist {
                args.push("--no-playlist".to_string());
            } else {
                args.push("--yes-playlist".to_string());
            }

            if use_metadata {
                args.push("--embed-metadata".to_string());
                args.push("--embed-chapters".to_string());
                args.push("--embed-thumbnail".to_string());
            }

            if use_sponsorblock {
                args.push("--sponsorblock-remove".to_string());
                args.push("all".to_string());
            }

            if use_subtitles {
                args.push("--write-subs".to_string());
                args.push("--write-auto-subs".to_string());
                args.push("--sub-langs".to_string());
                args.push("all".to_string());
                args.push("--embed-subs".to_string());
            }

            match cookie_browser {
                CookieBrowser::None => {}
                CookieBrowser::Chrome => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("chrome".to_string());
                }
                CookieBrowser::Firefox => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Edge => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("edge".to_string());
                }
                CookieBrowser::Brave => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("brave".to_string());
                }
                CookieBrowser::Opera => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("opera".to_string());
                }
                CookieBrowser::Vivaldi => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("vivaldi".to_string());
                }
                CookieBrowser::Chromium => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("chromium".to_string());
                }
                CookieBrowser::Whale => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("whale".to_string());
                }
                // Forks: Map to parent engine
                CookieBrowser::LibreWolf => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Waterfox => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::PaleMoon => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Zen => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Thorium => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("chrome".to_string());
                }
                CookieBrowser::Arc => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("chrome".to_string());
                }
                CookieBrowser::Floorp => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Mercury => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Pulse => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("firefox".to_string());
                }
                CookieBrowser::Comet => {
                    args.push("--cookies-from-browser".to_string());
                    args.push("chrome".to_string());
                }
            }

            match download_type {
                DownloadType::Video => {
                    args.push("-f".to_string());
                    if let Some(fmt_str) = selected_format {
                        // fmt_str is like "1080p"
                        let height = fmt_str.trim_end_matches('p');
                        // format: bestvideo[height=1080]+bestaudio/best[height=1080]
                        let selector =
                            format!("bestvideo[height={0}]+bestaudio/best[height={0}]", height);
                        args.push(selector);
                    } else {
                        args.push("bestvideo+bestaudio/best".to_string());
                    }
                    args.push("--merge-output-format".to_string());
                    args.push("mp4".to_string());
                }
                DownloadType::Audio => {
                    args.push("-x".to_string());
                    args.push("--audio-format".to_string());
                    args.push("mp3".to_string());
                    args.push("--audio-quality".to_string());
                    args.push("0".to_string());
                }
            }

            args.push("-o".to_string());
            let out_tmpl = download_path.join("%(title)s.%(ext)s");
            args.push(out_tmpl.to_string_lossy().to_string());

            args.push(url);

            use std::process::{Command, Stdio};

            let mut cmd = Command::new(ytdlp_exe);
            cmd.args(&args);

            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            #[cfg(target_os = "windows")]
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

            log(&logs, format!("Running: yt-dlp ..."));

            match cmd.spawn() {
                Ok(mut child) => {
                    let stdout = child.stdout.take().expect("Failed to open stdout");
                    let stderr = child.stderr.take().expect("Failed to open stderr");

                    let logs_clone = logs.clone();
                    let state_clone = state.clone();
                    let final_filename = Arc::new(Mutex::new(None));
                    let final_filename_clone = final_filename.clone();
                    let fmt_str = progress_fmt.clone();

                    let stdout_thread = thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                if l.contains("[download]") && l.contains("%") {
                                    if let Some(start) = l.find("%") {
                                        let substr = &l[..start];
                                        if let Some(space) = substr.rfind(' ') {
                                            if let Ok(p) = substr[space + 1..].parse::<f32>() {
                                                let parts: Vec<&str> =
                                                    l.split_whitespace().collect();

                                                let mut p_val = None;
                                                let mut t_val = None;
                                                let mut s_val = None;
                                                let mut e_val = None;

                                                for (i, part) in parts.iter().enumerate() {
                                                    if part.contains("%") {
                                                        p_val = Some(part.trim_end_matches('%'));
                                                    } else if *part == "of" && i + 1 < parts.len() {
                                                        let val = parts[i + 1];
                                                        if val != "Unknown" && val != "N/A" {
                                                            t_val = Some(val);
                                                        }
                                                    } else if *part == "at" && i + 1 < parts.len() {
                                                        let val = parts[i + 1];
                                                        if val != "Unknown" && val != "N/A" {
                                                            s_val = Some(val);
                                                        }
                                                    } else if *part == "ETA" && i + 1 < parts.len()
                                                    {
                                                        let val = parts[i + 1];
                                                        if val != "Unknown" && val != "N/A" {
                                                            e_val = Some(val);
                                                        }
                                                    }
                                                }

                                                let fmt_segments: Vec<&str> =
                                                    fmt_str.split("{}").collect();
                                                let mut status_msg = String::new();

                                                if let Some(p_str) = p_val {
                                                    if fmt_segments.len() >= 5 {
                                                        status_msg.push_str(fmt_segments[0]);
                                                        status_msg.push_str(p_str);

                                                        if let Some(t) = t_val {
                                                            status_msg.push_str(fmt_segments[1]);
                                                            status_msg.push_str(t);
                                                        } else {
                                                            status_msg.push_str("%");
                                                        }

                                                        if let Some(s) = s_val {
                                                            status_msg.push_str(fmt_segments[2]);
                                                            status_msg.push_str(s);
                                                        }

                                                        if let Some(e) = e_val {
                                                            status_msg.push_str(fmt_segments[3]);
                                                            status_msg.push_str(e);
                                                            status_msg.push_str(fmt_segments[4]);
                                                        }
                                                    } else {
                                                        status_msg = format!("{}%", p_str);
                                                    }
                                                } else {
                                                    status_msg = l.clone();
                                                }

                                                if let Ok(mut s) = state_clone.lock() {
                                                    *s = DownloadState::Downloading(
                                                        p / 100.0,
                                                        status_msg,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                if l.contains("Merging formats into \"") {
                                    if let Some(start) = l.find("Merging formats into \"") {
                                        let raw_path =
                                            &l[start + "Merging formats into \"".len()..];
                                        let clean_path = raw_path.trim().trim_end_matches('"');
                                        *final_filename_clone.lock().unwrap() =
                                            Some(PathBuf::from(clean_path));
                                    }
                                } else if l.contains("Destination: ") {
                                    if final_filename_clone.lock().unwrap().is_none() {
                                        if let Some(start) = l.find("Destination: ") {
                                            let raw_path = &l[start + "Destination: ".len()..];
                                            let clean_path = raw_path.trim();
                                            *final_filename_clone.lock().unwrap() =
                                                Some(PathBuf::from(clean_path));
                                        }
                                    }
                                }
                                if l.contains("[ExtractAudio] Destination: ") {
                                    if let Some(start) = l.find("[ExtractAudio] Destination: ") {
                                        let raw_path =
                                            &l[start + "[ExtractAudio] Destination: ".len()..];
                                        let clean_path = raw_path.trim();
                                        *final_filename_clone.lock().unwrap() =
                                            Some(PathBuf::from(clean_path));
                                    }
                                }
                                log(&logs_clone, l);
                            }
                        }
                    });

                    let logs_clone_err = logs.clone();
                    let stderr_thread = thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                log(&logs_clone_err, format!("ERR: {}", l));
                            }
                        }
                    });

                    let status = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();

                    match status {
                        Ok(exit_status) => {
                            if exit_status.success() {
                                let final_path = final_filename.lock().unwrap().clone();
                                if let Some(path) = final_path {
                                    *state.lock().unwrap() = DownloadState::Finished(
                                        path,
                                        "Download Completed!".to_string(),
                                    );
                                } else {
                                    *state.lock().unwrap() = DownloadState::Finished(
                                        PathBuf::new(),
                                        "Download Completed!".to_string(),
                                    );
                                }
                                log(&logs, "Download Finished Successfully.");
                            } else {
                                *state.lock().unwrap() =
                                    DownloadState::Error(format!("Exit Code: {}", exit_status));
                                log(&logs, "Download Failed.");
                            }
                        }
                        Err(e) => {
                            *state.lock().unwrap() = DownloadState::Error(e.to_string());
                            log(&logs, format!("Wait Error: {}", e));
                        }
                    }
                }
                Err(e) => {
                    *state.lock().unwrap() = DownloadState::Error(e.to_string());
                    log(&logs, format!("Execution Error: {}", e));
                }
            }
        });
    }
}
