use crate::api::realtime_audio::model_loader::{
    download_parakeet_model, get_parakeet_model_dir, is_model_downloaded,
};
use crate::gui::locale::LocaleText;
use crate::gui::settings_ui::download_manager::{DownloadManager, InstallStatus};
use crate::overlay::realtime_webview::state::REALTIME_STATE;
use eframe::egui;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

pub fn render_downloaded_tools_modal(
    ctx: &egui::Context,
    _ui: &mut egui::Ui,
    show_modal: &mut bool,
    download_manager: &mut DownloadManager,
    text: &LocaleText,
) {
    if *show_modal {
        let mut open = true;
        egui::Window::new(text.downloaded_tools_title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.add_space(8.0);

                // --- Parakeet ---
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text.tool_parakeet).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_downloading = {
                                if let Ok(state) = REALTIME_STATE.lock() {
                                    state.is_downloading
                                } else {
                                    false
                                }
                            };

                            if is_downloading {
                                let progress = {
                                    if let Ok(state) = REALTIME_STATE.lock() {
                                        state.download_progress
                                    } else {
                                        0.0
                                    }
                                };
                                ui.label(format!("{:.0}%", progress));
                                ui.spinner();
                            } else if is_model_downloaded() {
                                let size = get_dir_size(get_parakeet_model_dir());
                                ui.label(
                                    egui::RichText::new(
                                        text.tool_status_installed
                                            .replace("{}", &format_size(size)),
                                    )
                                    .color(egui::Color32::from_rgb(34, 139, 34)),
                                );
                                if ui
                                    .button(
                                        egui::RichText::new(text.tool_action_delete)
                                            .color(egui::Color32::RED),
                                    )
                                    .clicked()
                                {
                                    let _ = fs::remove_dir_all(get_parakeet_model_dir());
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(text.tool_status_missing)
                                        .color(egui::Color32::GRAY),
                                );
                                if ui.button(text.tool_action_download).clicked() {
                                    let stop_signal = Arc::new(AtomicBool::new(false));
                                    thread::spawn(move || {
                                        let _ = download_parakeet_model(stop_signal, false);
                                    });
                                }
                            }
                        });
                    });
                    ui.label(text.tool_desc_parakeet);
                });

                ui.add_space(8.0);

                // --- yt-dlp ---
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text.tool_ytdlp).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let status = download_manager.ytdlp_status.lock().unwrap().clone();
                            match status {
                                InstallStatus::Installed => {
                                    let path = download_manager.bin_dir.join("yt-dlp.exe");
                                    let size = if let Ok(meta) = fs::metadata(&path) {
                                        meta.len()
                                    } else {
                                        0
                                    };
                                    ui.label(
                                        egui::RichText::new(
                                            text.tool_status_installed
                                                .replace("{}", &format_size(size)),
                                        )
                                        .color(egui::Color32::from_rgb(34, 139, 34)),
                                    );
                                    if ui
                                        .button(
                                            egui::RichText::new(text.tool_action_delete)
                                                .color(egui::Color32::RED),
                                        )
                                        .clicked()
                                    {
                                        let _ = fs::remove_file(path);
                                        *download_manager.ytdlp_status.lock().unwrap() =
                                            InstallStatus::Missing;
                                    }
                                }
                                InstallStatus::Downloading(p) => {
                                    ui.label(format!("{:.0}%", p * 100.0));
                                    ui.spinner();
                                }
                                InstallStatus::Extracting => {
                                    ui.label(text.download_status_extracting);
                                    ui.spinner();
                                }
                                InstallStatus::Checking => {
                                    ui.spinner();
                                }
                                _ => {
                                    // Missing or Error
                                    ui.label(
                                        egui::RichText::new(text.tool_status_missing)
                                            .color(egui::Color32::GRAY),
                                    );
                                    if ui.button(text.tool_action_download).clicked() {
                                        download_manager.start_download_ytdlp();
                                    }
                                }
                            }
                        });
                    });
                    ui.label(text.tool_desc_ytdlp);
                });

                ui.add_space(8.0);

                // --- ffmpeg ---
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(text.tool_ffmpeg).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let status = download_manager.ffmpeg_status.lock().unwrap().clone();
                            match status {
                                InstallStatus::Installed => {
                                    // ffmpeg is usually a dir or file. In manager it's bin/ffmpeg.exe?
                                    // The manager checks bin.join("ffmpeg.exe").
                                    let path = download_manager.bin_dir.join("ffmpeg.exe");
                                    let size = if let Ok(meta) = fs::metadata(&path) {
                                        meta.len()
                                    } else {
                                        // might be just the exe size, proper size includes prompt/etc? No, just ffmpeg.
                                        0
                                    };
                                    // Actually ffmpeg download extracts multiple files.
                                    // Deleting just ffmpeg.exe is what manager does?
                                    // Manager delete_dependencies() deletes `yt-dlp.exe` and `ffmpeg.exe` and `ffprobe.exe`.
                                    // We should probably replicate that or call a helper.
                                    // But manager doesn't expose `delete_ffmpeg` separate from `delete_dependencies`.
                                    // I'll delete ffmpeg.exe and ffprobe.exe manually here.

                                    ui.label(
                                        egui::RichText::new(
                                            text.tool_status_installed
                                                .replace("{}", &format_size(size)),
                                        )
                                        .color(egui::Color32::from_rgb(34, 139, 34)),
                                    );
                                    if ui
                                        .button(
                                            egui::RichText::new(text.tool_action_delete)
                                                .color(egui::Color32::RED),
                                        )
                                        .clicked()
                                    {
                                        let _ = fs::remove_file(&path);
                                        let _ = fs::remove_file(
                                            download_manager.bin_dir.join("ffprobe.exe"),
                                        );
                                        *download_manager.ffmpeg_status.lock().unwrap() =
                                            InstallStatus::Missing;
                                    }
                                }
                                InstallStatus::Downloading(p) => {
                                    ui.label(format!("{:.0}%", p * 100.0));
                                    ui.spinner();
                                }
                                InstallStatus::Extracting => {
                                    ui.label(text.download_status_extracting);
                                    ui.spinner();
                                }
                                InstallStatus::Checking => {
                                    ui.spinner();
                                }
                                _ => {
                                    ui.label(
                                        egui::RichText::new(text.tool_status_missing)
                                            .color(egui::Color32::GRAY),
                                    );
                                    if ui.button(text.tool_action_download).clicked() {
                                        download_manager.start_download_ffmpeg();
                                    }
                                }
                            }
                        });
                    });
                    ui.label(text.tool_desc_ffmpeg);
                });
            });

        *show_modal = open;
    }
}

fn get_dir_size(path: PathBuf) -> u64 {
    let mut total_size = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries {
            if let Ok(entry) = entry {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        total_size += get_dir_size(entry.path());
                    } else {
                        total_size += metadata.len();
                    }
                }
            }
        }
    }
    total_size
}

fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    format!("{:.1} MB", mb)
}
