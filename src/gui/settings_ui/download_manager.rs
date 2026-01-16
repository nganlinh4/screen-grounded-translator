use crate::gui::locale::LocaleText;
use eframe::egui;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, PartialEq, Debug)]
pub enum InstallStatus {
    Checking,
    Missing,
    Downloading(f32), // 0.0 to 1.0
    Extracting,
    Installed,
    Error(String),
}

#[derive(Clone, PartialEq, Debug)]
pub enum DownloadState {
    Idle,
    Downloading(f32, String),  // Progress, Status message
    Finished(PathBuf, String), // File Path, Success message
    Error(String),             // Error message
}

pub struct DownloadManager {
    pub show_window: bool,
    pub ffmpeg_status: Arc<Mutex<InstallStatus>>,
    pub ytdlp_status: Arc<Mutex<InstallStatus>>,
    pub logs: Arc<Mutex<Vec<String>>>,
    pub bin_dir: PathBuf,

    // Downloader State
    pub input_url: String,
    pub download_type: DownloadType,
    pub download_state: Arc<Mutex<DownloadState>>,

    // Config
    pub custom_download_path: Option<PathBuf>,
    pub cancel_flag: Arc<AtomicBool>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum DownloadType {
    Video, // Best video+audio -> mkv/mp4
    Audio, // Audio only -> mp3
}

impl DownloadManager {
    pub fn new() -> Self {
        let bin_dir = dirs::data_local_dir()
            .unwrap_or(PathBuf::from("."))
            .join("screen-goated-toolbox")
            .join("bin");

        let manager = Self {
            show_window: false,
            ffmpeg_status: Arc::new(Mutex::new(InstallStatus::Checking)),
            ytdlp_status: Arc::new(Mutex::new(InstallStatus::Checking)),
            logs: Arc::new(Mutex::new(Vec::new())),
            bin_dir: bin_dir.clone(),
            input_url: String::new(),
            download_type: DownloadType::Video,
            download_state: Arc::new(Mutex::new(DownloadState::Idle)),
            custom_download_path: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };

        manager.check_status();
        manager
    }

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
        let output = std::process::Command::new("powershell")
            .args(&["-Command", "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.ShowDialog() | Out-Null; $f.SelectedPath"])
            .output();

        if let Ok(out) = output {
            if let Ok(path) = String::from_utf8(out.stdout) {
                let path = path.trim().to_string();
                if !path.is_empty() {
                    self.custom_download_path = Some(PathBuf::from(path));
                }
            }
        }
    }

    pub fn start_download_ytdlp(&self) {
        let bin = self.bin_dir.clone();
        let status = self.ytdlp_status.clone();
        let logs = self.logs.clone();
        let cancel = self.cancel_flag.clone();

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
            let url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
            log(&logs, format!("Starting download: {}", url));

            match download_file(url, &bin.join("yt-dlp.exe"), &status, &cancel) {
                Ok(_) => {
                    *status.lock().unwrap() = InstallStatus::Installed;
                    log(&logs, "yt-dlp installed successfully");
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
        let logs = self.logs.clone();
        let cancel = self.cancel_flag.clone();

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

    pub fn start_media_download(&self, progress_fmt: String) {
        let url = self.input_url.trim().to_string();
        if url.is_empty() {
            return;
        }

        let bin_dir = self.bin_dir.clone();
        let download_type = self.download_type.clone();
        let state = self.download_state.clone();
        let logs = self.logs.clone();

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

            // Force UTF-8 output to correctly capture filenames with non-ASCII characters (e.g. Korean)
            args.push("--encoding".to_string());
            args.push("utf-8".to_string());

            // Point to ffmpeg
            args.push("--ffmpeg-location".to_string());
            args.push(bin_dir.to_string_lossy().to_string());

            // Progress per line for potential parsing
            args.push("--newline".to_string());
            args.push("--no-playlist".to_string());

            match download_type {
                DownloadType::Video => {
                    // Best video + best audio, merge to mp4
                    args.push("-f".to_string());
                    args.push("bestvideo+bestaudio/best".to_string());
                    args.push("--merge-output-format".to_string());
                    args.push("mp4".to_string());
                }
                DownloadType::Audio => {
                    // Extract audio to mp3
                    args.push("-x".to_string());
                    args.push("--audio-format".to_string());
                    args.push("mp3".to_string());
                    args.push("--audio-quality".to_string());
                    args.push("0".to_string()); // Best quality
                }
            }

            // Output template designed to be parsable but we rely on yt-dlp log for filename
            args.push("-o".to_string());
            let out_tmpl = download_path.join("%(title)s.%(ext)s");
            args.push(out_tmpl.to_string_lossy().to_string());

            args.push(url);

            use std::io::{BufRead, BufReader};
            #[cfg(target_os = "windows")]
            use std::os::windows::process::CommandExt;
            use std::process::{Command, Stdio};

            let mut cmd = Command::new(ytdlp_exe);
            cmd.args(&args);

            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // On Windows, create no window
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

                    // Spawn thread for stdout
                    let stdout_thread = thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                // Simple progress extraction
                                // [download]  23.5% of   10.55MiB at    5.20MiB/s ETA 00:01
                                if l.contains("[download]") && l.contains("%") {
                                    if let Some(start) = l.find("%") {
                                        // Parse percentage
                                        let substr = &l[..start]; // ... [download]  23.5
                                        if let Some(space) = substr.rfind(' ') {
                                            if let Ok(p) = substr[space + 1..].parse::<f32>() {
                                                // Dynamic parsing of yt-dlp output
                                                // Standard: [download]  23.5% of   10.55MiB at    5.20MiB/s ETA 00:01
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

                                                // Construct message based on format string and available data
                                                // fmt_str: "{}% of {}, at {}, ETA {}" (Example)
                                                // We treat it as 5 segments split by "{}"
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
                                                            // Fallback unit if total is missing
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
                                                        // Fallback if format string is weird
                                                        status_msg = format!("{}%", p_str);
                                                    }
                                                } else {
                                                    // Should not happen if we parsed p ok
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

                                // Capture destination filename
                                if l.contains("Merging formats into \"") {
                                    if let Some(start) = l.find("Merging formats into \"") {
                                        let raw_path =
                                            &l[start + "Merging formats into \"".len()..];
                                        // Trim trailing quote and whitespace
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
                    // Spawn thread for stderr
                    let stderr_thread = thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                log(&logs_clone_err, format!("ERR: {}", l));
                            }
                        }
                    });

                    let status = child.wait();

                    // Wait for IO threads to finish reading
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
                                    // Fallback
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

    pub fn render(&mut self, ctx: &egui::Context, text: &LocaleText) {
        if !self.show_window {
            return;
        }

        let mut open = true;
        egui::Window::new(text.download_feature_title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                // Dependency Check
                let ffmpeg_ok = matches!(
                    *self.ffmpeg_status.lock().unwrap(),
                    InstallStatus::Installed
                );
                let ytdlp_ok =
                    matches!(*self.ytdlp_status.lock().unwrap(), InstallStatus::Installed);

                if !ffmpeg_ok || !ytdlp_ok {
                    ui.label(text.download_deps_missing);

                    // yt-dlp section
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(text.download_deps_ytdlp);
                            let status = self.ytdlp_status.lock().unwrap().clone();
                            match status {
                                InstallStatus::Checking => {
                                    ui.spinner();
                                }
                                InstallStatus::Missing | InstallStatus::Error(_) => {
                                    if ui.button(text.download_deps_download_btn).clicked() {
                                        self.start_download_ytdlp();
                                    }
                                    if let InstallStatus::Error(e) = status {
                                        ui.colored_label(egui::Color32::RED, e);
                                    }
                                }
                                InstallStatus::Downloading(p) => {
                                    ui.label(format!("{:.0}%", p * 100.0));
                                    ui.add(egui::ProgressBar::new(p).desired_width(120.0));
                                    if ui.button(text.download_cancel_btn).clicked() {
                                        self.cancel_download();
                                    }
                                }
                                InstallStatus::Extracting => {
                                    ui.label(text.download_status_extracting);
                                    ui.spinner();
                                    if ui.button(text.download_cancel_btn).clicked() {
                                        self.cancel_download();
                                    }
                                }
                                InstallStatus::Installed => {
                                    ui.label(text.download_status_ready);
                                }
                            }
                        });
                    });

                    // ffmpeg section
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(text.download_deps_ffmpeg);
                            let status = self.ffmpeg_status.lock().unwrap().clone();
                            match status {
                                InstallStatus::Checking => {
                                    ui.spinner();
                                }
                                InstallStatus::Missing | InstallStatus::Error(_) => {
                                    if ui.button(text.download_deps_download_btn).clicked() {
                                        self.start_download_ffmpeg();
                                    }
                                    if let InstallStatus::Error(e) = status {
                                        ui.colored_label(egui::Color32::RED, e);
                                    }
                                }
                                InstallStatus::Downloading(p) => {
                                    ui.label(format!("{:.0}%", p * 100.0));
                                    ui.add(egui::ProgressBar::new(p).desired_width(120.0));
                                    if ui.button(text.download_cancel_btn).clicked() {
                                        self.cancel_download();
                                    }
                                }
                                InstallStatus::Extracting => {
                                    ui.label(text.download_status_extracting);
                                    ui.spinner();
                                    if ui.button(text.download_cancel_btn).clicked() {
                                        self.cancel_download();
                                    }
                                }
                                InstallStatus::Installed => {
                                    ui.label(text.download_status_ready);
                                }
                            }
                        });
                    });
                } else {
                    // MAIN DOWNLOADER UI

                    // Options Gear
                    ui.horizontal(|ui| {
                        ui.menu_button("⚙", |ui| {
                            if ui.button(text.download_change_folder_btn).clicked() {
                                self.change_download_folder();
                                ui.close();
                            }

                            // Delete Dependencies
                            let (ytdlp_size, ffmpeg_size) = self.get_dependency_sizes();
                            let del_btn_text = text
                                .download_delete_deps_btn
                                .replacen("{}", &ytdlp_size, 1)
                                .replacen("{}", &ffmpeg_size, 1);

                            if ui
                                .button(egui::RichText::new(del_btn_text).color(egui::Color32::RED))
                                .clicked()
                            {
                                self.delete_dependencies();
                                ui.close();
                            }
                        });

                        // Show current path tooltip or trimmed text?
                        let current_path = self
                            .custom_download_path
                            .clone()
                            .unwrap_or_else(|| dirs::download_dir().unwrap_or(PathBuf::from(".")));
                        let path_str = current_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("...");
                        ui.label(format!("({})", path_str));
                    });

                    ui.add_space(8.0);

                    // URL Input
                    ui.horizontal(|ui| {
                        ui.label(text.download_url_label);
                        ui.text_edit_singleline(&mut self.input_url);
                    });

                    // Format Selection
                    ui.horizontal(|ui| {
                        ui.label(text.download_format_label);
                        ui.radio_value(&mut self.download_type, DownloadType::Video, "Video");
                        ui.radio_value(&mut self.download_type, DownloadType::Audio, "Audio");
                    });

                    ui.add_space(8.0);

                    let state = self.download_state.lock().unwrap().clone();
                    match &state {
                        DownloadState::Idle | DownloadState::Error(_) => {
                            if ui.button(text.download_start_btn).clicked() {
                                if !self.input_url.is_empty() {
                                    self.start_media_download(
                                        text.download_progress_info_fmt.to_string(),
                                    );
                                }
                            }
                            if let DownloadState::Error(err) = &state {
                                ui.colored_label(
                                    egui::Color32::RED,
                                    format!("{} {}", text.download_status_error, err),
                                );
                            }
                        }
                        DownloadState::Finished(path, _msg) => {
                            // Darker green for readability on light themes
                            let success_color = if ctx.style().visuals.dark_mode {
                                egui::Color32::GREEN
                            } else {
                                egui::Color32::from_rgb(0, 128, 0)
                            };

                            ui.colored_label(success_color, text.download_status_finished);

                            // File info
                            if let Some(name) = path.file_name() {
                                ui.label(format!(
                                    "{} {}",
                                    text.download_file_label,
                                    name.to_string_lossy()
                                ));
                            }
                            // Size if exists
                            if let Ok(meta) = fs::metadata(path) {
                                let size_mb = meta.len() as f64 / 1024.0 / 1024.0;
                                ui.label(format!("{} {:.2} MB", text.download_size_label, size_mb));
                            }

                            ui.horizontal(|ui| {
                                // Only enable buttons if path is valid (non-empty)
                                let enabled = path.components().next().is_some(); // cheap check for non-empty

                                if ui
                                    .add_enabled(
                                        enabled,
                                        egui::Button::new(text.download_open_file_btn),
                                    )
                                    .clicked()
                                {
                                    let _ = open::that(&path);
                                }
                                if ui
                                    .add_enabled(
                                        enabled,
                                        egui::Button::new(text.download_open_folder_btn),
                                    )
                                    .clicked()
                                {
                                    if let Some(parent) = path.parent() {
                                        let _ = open::that(parent);
                                    } else {
                                        // Try opening path itself (might be dir) or "."
                                        let _ = open::that(&path);
                                    }
                                }
                            });

                            ui.add_space(4.0);
                            if ui.button(text.download_start_btn).clicked() {
                                if !self.input_url.is_empty() {
                                    self.start_media_download(
                                        text.download_progress_info_fmt.to_string(),
                                    );
                                }
                            }
                        }
                        DownloadState::Downloading(progress, msg) => {
                            if msg == "Starting..." {
                                ui.label(text.download_status_starting);
                            } else {
                                let clean_msg = msg.replace("[download]", "").trim().to_string();
                                ui.label(clean_msg);
                            }
                            ui.add(egui::ProgressBar::new(*progress));
                        }
                    }
                }
            });

        self.show_window = open;
    }
}

fn log(logs: &Arc<Mutex<Vec<String>>>, msg: impl Into<String>) {
    logs.lock().unwrap().push(msg.into());
}

fn download_file(
    url: &str,
    path: &PathBuf,
    status: &Arc<Mutex<InstallStatus>>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;

    let total_size = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    // Download to temp file first
    let temp_path = path.with_extension("tmp");
    let mut reader = resp.into_body().into_reader();
    let mut file = fs::File::create(&temp_path).map_err(|e| e.to_string())?;

    let mut buffer = [0; 8192];
    let mut downloaded: u64 = 0;

    loop {
        if cancel.load(Ordering::Relaxed) {
            // Cleanup temp file on cancel
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err("Cancelled".to_string());
        }
        let bytes_read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read])
            .map_err(|e| e.to_string())?;
        downloaded += bytes_read as u64;

        if total_size > 0 {
            let progress = downloaded as f32 / total_size as f32;
            *status.lock().unwrap() = InstallStatus::Downloading(progress);
        }
    }

    // Ensure file is flushed and closed before rename
    drop(file);

    // Rename temp file to final path
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("Failed to rename temp file: {}", e)
    })?;

    Ok(())
}

fn extract_ffmpeg(zip_path: &PathBuf, bin_dir: &PathBuf) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name();

        // We only care about ffmpeg.exe
        if name.ends_with("ffmpeg.exe") {
            let mut out_file =
                fs::File::create(bin_dir.join("ffmpeg.exe")).map_err(|e| e.to_string())?;
            io::copy(&mut file, &mut out_file).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    Err("ffmpeg.exe not found in archive".to_string())
}
