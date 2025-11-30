use std::sync::mpsc::Sender;
use std::thread;

#[derive(Debug, Clone)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate(String), // Current version
    UpdateAvailable { version: String, body: String },
    Downloading,
    Error(String),
    UpdatedAndRestartRequired,
}

pub struct Updater {
    tx: Sender<UpdateStatus>,
}

impl Updater {
    pub fn new(tx: Sender<UpdateStatus>) -> Self {
        Self { tx }
    }

    pub fn check_for_updates(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(UpdateStatus::Checking);

            // Configure the updater to check GitHub Releases
            // NOTE: Ensure your GitHub release asset is either just the .exe 
            // OR a .zip containing the binary named "screen-grounded-translator.exe"
            let status = self_update::backends::github::Update::configure()
                .repo_owner("nganlinh4")
                .repo_name("screen-grounded-translator")
                .bin_name("screen-grounded-translator") 
                .show_download_progress(false) 
                .current_version(env!("CARGO_PKG_VERSION"))
                .build();

            match status {
                Ok(updater) => {
                    match updater.get_latest_release() {
                        Ok(release) => {
                            let current = env!("CARGO_PKG_VERSION");
                            let is_newer = self_update::version::bump_is_greater(current, &release.version).unwrap_or(false);

                            if is_newer {
                                let _ = tx.send(UpdateStatus::UpdateAvailable { 
                                    version: release.version,
                                    body: release.body.unwrap_or_default()
                                });
                            } else {
                                let _ = tx.send(UpdateStatus::UpToDate(current.to_string()));
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(UpdateStatus::Error(format!("Failed to fetch info: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(UpdateStatus::Error(format!("Config error: {}", e)));
                }
            }
        });
    }

    pub fn perform_update(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(UpdateStatus::Downloading);

            // Get the latest release and download the first .exe or .zip we find
            match self_update::backends::github::Update::configure()
                .repo_owner("nganlinh4")
                .repo_name("screen-grounded-translator")
                .bin_name("screen-grounded-translator")
                .show_download_progress(false)
                .current_version(env!("CARGO_PKG_VERSION"))
                .build()
            {
                Ok(updater) => {
                    match updater.get_latest_release() {
                        Ok(release) => {
                            // Find first .exe or .zip asset
                            let asset = release.assets.iter()
                                .find(|a| a.name.ends_with(".exe") || a.name.ends_with(".zip"));
                            
                            match asset {
                                Some(asset) => {
                                    // Download the asset
                                    match std::fs::File::create("temp_download") {
                                        Ok(mut file) => {
                                            match ureq::get(&asset.download_url)
                                                .call()
                                            {
                                                Ok(response) => {
                                                    if let Err(e) = std::io::copy(&mut response.into_reader(), &mut file) {
                                                        let _ = tx.send(UpdateStatus::Error(format!("Download failed: {}", e)));
                                                        return;
                                                    }

                                                    // Get current exe path
                                                    if let Ok(exe_path) = std::env::current_exe() {
                                                        // Backup current exe
                                                        let backup_path = exe_path.with_extension("exe.old");
                                                        let _ = std::fs::copy(&exe_path, &backup_path);
                                                        
                                                        // If .zip, extract it
                                                        if asset.name.ends_with(".zip") {
                                                            match zip::ZipArchive::new(&mut std::fs::File::open("temp_download").unwrap()) {
                                                                Ok(mut archive) => {
                                                                    if let Ok(mut file) = archive.by_index(0) {
                                                                        if let Ok(mut exe_file) = std::fs::File::create(&exe_path) {
                                                                            let _ = std::io::copy(&mut file, &mut exe_file);
                                                                            let _ = std::fs::remove_file("temp_download");
                                                                            let _ = tx.send(UpdateStatus::UpdatedAndRestartRequired);
                                                                        } else {
                                                                            let _ = tx.send(UpdateStatus::Error("Failed to write new exe".to_string()));
                                                                        }
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    let _ = tx.send(UpdateStatus::Error(format!("Failed to extract zip: {}", e)));
                                                                }
                                                            }
                                                        } else {
                                                            // Direct .exe - just move it
                                                            match std::fs::rename("temp_download", &exe_path) {
                                                                Ok(_) => {
                                                                    let _ = tx.send(UpdateStatus::UpdatedAndRestartRequired);
                                                                }
                                                                Err(e) => {
                                                                    let _ = tx.send(UpdateStatus::Error(format!("Failed to replace exe: {}", e)));
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        let _ = tx.send(UpdateStatus::Error("Could not get exe path".to_string()));
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ = tx.send(UpdateStatus::Error(format!("Download failed: {}", e)));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(UpdateStatus::Error(format!("Failed to create temp file: {}", e)));
                                        }
                                    }
                                }
                                None => {
                                    let _ = tx.send(UpdateStatus::Error("No .exe or .zip found in release".to_string()));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(UpdateStatus::Error(format!("Failed to fetch release: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(UpdateStatus::Error(format!("Builder error: {}", e)));
                }
            }
        });
    }
}
