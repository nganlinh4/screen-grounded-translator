use crate::gui::locale::LocaleText;
use crate::updater::{UpdateStatus, Updater};
use eframe::egui;

pub fn render_update_section_content(
    ui: &mut egui::Ui,
    updater: &Option<Updater>,
    status: &UpdateStatus,
    text: &LocaleText,
) {
    match status {
        UpdateStatus::Idle => {
            ui.horizontal(|ui| {
                let ver_string = format!(
                    "{} v{}",
                    text.current_version_label,
                    env!("CARGO_PKG_VERSION")
                );
                ui.label(ver_string);
                if ui.button(text.check_for_updates_btn).clicked() {
                    if let Some(u) = updater {
                        u.check_for_updates();
                    }
                }
            });
        }
        UpdateStatus::Checking => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text.checking_github);
            });
        }
        UpdateStatus::UpToDate(ver) => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} (v{})", text.up_to_date, ver))
                        .color(egui::Color32::from_rgb(34, 139, 34)),
                );
                if ui.button(text.check_again_btn).clicked() {
                    if let Some(u) = updater {
                        u.check_for_updates();
                    }
                }
            });
        }
        UpdateStatus::UpdateAvailable { version, body } => {
            ui.colored_label(
                egui::Color32::YELLOW,
                format!("{} {}", text.new_version_available, version),
            );
            ui.collapsing(text.release_notes_label, |ui| {
                ui.label(body);
            });
            ui.add_space(5.0);
            if ui
                .button(egui::RichText::new(text.download_update_btn).strong())
                .clicked()
            {
                if let Some(u) = updater {
                    u.perform_update();
                }
            }
        }
        UpdateStatus::Downloading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text.downloading_update);
            });
        }
        UpdateStatus::Error(e) => {
            ui.colored_label(egui::Color32::RED, format!("{} {}", text.update_failed, e));
            ui.label(egui::RichText::new(text.app_folder_writable_hint).size(11.0));
            if ui.button(text.retry_btn).clicked() {
                if let Some(u) = updater {
                    u.check_for_updates();
                }
            }
        }
        UpdateStatus::UpdatedAndRestartRequired => {
            ui.label(
                egui::RichText::new(text.update_success)
                    .color(egui::Color32::GREEN)
                    .heading(),
            );
            ui.label(text.restart_to_use_new_version);
            if ui.button(text.restart_app_btn).clicked() {
                if let Ok(exe_path) = std::env::current_exe() {
                    if let Some(exe_dir) = exe_path.parent() {
                        if let Ok(entries) = std::fs::read_dir(exe_dir) {
                            if let Some(newest_exe) = entries
                                .filter_map(|e| e.ok())
                                .filter(|e| {
                                    let name = e.file_name();
                                    let name_str = name.to_string_lossy();
                                    name_str.starts_with("ScreenGoatedToolbox_v")
                                        && name_str.ends_with(".exe")
                                })
                                .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                            {
                                let path = newest_exe.path();
                                println!("Attempting to spawn with delay: {:?}", path);

                                // Create a temporary batch file to handle the delayed restart reliably
                                // This avoids complex escaping issues with cmd /C inline commands
                                let kill_mutex_cmd = format!("timeout /t 2 /nobreak > NUL");
                                let start_cmd =
                                    format!("start \"\" \"{}\"", path.to_string_lossy());
                                let self_del_cmd = "(goto) 2>nul & del \"%~f0\"";

                                let batch_content = format!(
                                    "@echo off\r\n{}\r\n{}\r\n{}",
                                    kill_mutex_cmd, start_cmd, self_del_cmd
                                );

                                let temp_dir = std::env::temp_dir();
                                let bat_path = temp_dir
                                    .join(format!("sgt_restart_{}.bat", std::process::id()));

                                println!("Writing batch file to: {:?}", bat_path);
                                if let Ok(_) = std::fs::write(&bat_path, batch_content) {
                                    // Spawn the batch file hidden via cmd /C
                                    let status = std::process::Command::new("cmd")
                                        .args(["/C", &bat_path.to_string_lossy()])
                                        .spawn();

                                    match status {
                                        Ok(_) => std::process::exit(0),
                                        Err(e) => {
                                            eprintln!("Failed to spawn batch file: {}", e);
                                        }
                                    }
                                } else {
                                    eprintln!("Failed to write batch file");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
