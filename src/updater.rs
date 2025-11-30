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

            let status = self_update::backends::github::Update::configure()
                .repo_owner("nganlinh4")
                .repo_name("screen-grounded-translator")
                .bin_name("screen-grounded-translator")
                .show_download_progress(false)
                .current_version(env!("CARGO_PKG_VERSION"))
                .build();

            match status {
                Ok(updater) => {
                    // This performs the "Rename current to .old -> Download new -> Verify" dance
                    match updater.update() {
                        Ok(_) => {
                            let _ = tx.send(UpdateStatus::UpdatedAndRestartRequired);
                        }
                        Err(e) => {
                            let _ = tx.send(UpdateStatus::Error(format!("Update failed: {}", e)));
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
