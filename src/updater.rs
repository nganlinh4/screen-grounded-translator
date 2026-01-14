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

            // Use a custom manual request with a specific User-Agent to avoid 403 Forbidden
            // GitHub API requires a User-Agent, and self_update's default might be blocked or rate-limited.
            let url = "https://api.github.com/repos/nganlinh4/screen-goated-toolbox/releases?per_page=1&prerelease=false";

            // Use ureq 3.x API - create agent with config
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(10)))
                .build();
            let agent: ureq::Agent = config.into();

            let response = agent
                .get(url)
                .header("User-Agent", "screen-goated-toolbox-checker")
                .call();

            match response {
                Ok(mut resp) => {
                    let release_json: String = match resp.body_mut().read_to_string() {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = tx.send(UpdateStatus::Error(format!(
                                "Failed to read response: {}",
                                e
                            )));
                            return;
                        }
                    };

                    let data: Result<Vec<serde_json::Value>, _> =
                        serde_json::from_str(&release_json);
                    match data {
                        Ok(mut releases) if !releases.is_empty() => {
                            let rel = releases.remove(0);
                            let tag_name =
                                rel.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
                            let version = tag_name.trim_start_matches('v').to_string();
                            let body = rel
                                .get("body")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let current = env!("CARGO_PKG_VERSION");
                            let is_newer = self_update::version::bump_is_greater(current, &version)
                                .unwrap_or(false);

                            if is_newer {
                                let _ = tx.send(UpdateStatus::UpdateAvailable { version, body });
                            } else {
                                let _ = tx.send(UpdateStatus::UpToDate(current.to_string()));
                            }
                        }
                        Ok(_) => {
                            let _ = tx.send(UpdateStatus::Error(
                                "No releases found on GitHub".to_string(),
                            ));
                        }
                        Err(e) => {
                            let _ =
                                tx.send(UpdateStatus::Error(format!("JSON parse error: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let error_msg = {
                        let err_str = e.to_string();
                        if err_str.contains("403") {
                            "Status 403: GitHub API rate limit reached or access forbidden. Please try again later or check your network/VPN.".to_string()
                        } else {
                            format!("Network error: {}", e)
                        }
                    };
                    let _ = tx.send(UpdateStatus::Error(format!(
                        "Failed to fetch info: {}",
                        error_msg
                    )));
                }
            }
        });
    }

    pub fn perform_update(&self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let _ = tx.send(UpdateStatus::Downloading);

            // Get current exe directory
            let exe_dir = match std::env::current_exe() {
                Ok(exe_path) => match exe_path.parent() {
                    Some(dir) => dir.to_path_buf(),
                    None => {
                        let _ = tx.send(UpdateStatus::Error(
                            "Could not find exe directory".to_string(),
                        ));
                        return;
                    }
                },
                Err(_) => {
                    let _ = tx.send(UpdateStatus::Error("Could not get exe path".to_string()));
                    return;
                }
            };

            let temp_path = exe_dir.join("temp_download");
            // We'll set this after getting the asset
            let mut staging_path = exe_dir.join("update_pending.exe");

            // Use a custom HTTP request to get the latest release (the one marked as "Latest" on GitHub)
            let release_json = match ureq::get("https://api.github.com/repos/nganlinh4/screen-goated-toolbox/releases?per_page=1&prerelease=false")
                .header("User-Agent", "screen-goated-toolbox-updater")
                .call()
            {
                Ok(mut response) => {
                    match response.body_mut().read_to_string() {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = tx.send(UpdateStatus::Error(format!("Failed to parse response: {}", e)));
                            return;
                        }
                    }
                }
                Err(e) => {
                    let error_msg = {
                        let err_str = e.to_string();
                        if err_str.contains("403") {
                            "Status 403: GitHub API rate limit reached or access forbidden. Please try again later.".to_string()
                        } else {
                            format!("Failed to fetch release list: {}", e)
                        }
                    };
                    let _ = tx.send(UpdateStatus::Error(error_msg));
                    return;
                }
            };

            // Parse the JSON to get the first release
            let release_data: Result<Vec<serde_json::Value>, _> =
                serde_json::from_str(&release_json);
            let release = match release_data {
                Ok(mut releases) if !releases.is_empty() => {
                    let rel = releases.remove(0);
                    self_update::update::Release {
                        name: rel
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        version: rel
                            .get("tag_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim_start_matches('v')
                            .to_string(),
                        date: rel
                            .get("published_at")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        body: rel
                            .get("body")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        assets: rel
                            .get("assets")
                            .and_then(|a| a.as_array())
                            .unwrap_or(&vec![])
                            .iter()
                            .filter_map(|asset| {
                                let name = asset.get("name")?.as_str()?.to_string();
                                let download_url =
                                    asset.get("browser_download_url")?.as_str()?.to_string();
                                Some(self_update::update::ReleaseAsset { name, download_url })
                            })
                            .collect(),
                    }
                }
                _ => {
                    let _ = tx.send(UpdateStatus::Error("No releases found".to_string()));
                    return;
                }
            };

            // Find the .exe or .zip asset from release
            let asset = match release
                .assets
                .iter()
                .find(|a| a.name.ends_with(".exe") || a.name.ends_with(".zip"))
            {
                Some(a) => a,
                None => {
                    let _ = tx.send(UpdateStatus::Error(
                        "No .exe or .zip found in release assets".to_string(),
                    ));
                    return;
                }
            };

            // Set staging path to the asset name (for display) or update_pending.exe (for extraction)
            if asset.name.ends_with(".exe") {
                staging_path = exe_dir.join(&asset.name);
            }

            // Download the asset
            let mut file = match std::fs::File::create(&temp_path) {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(UpdateStatus::Error(format!(
                        "Failed to create temp file: {}",
                        e
                    )));
                    return;
                }
            };

            match ureq::get(&asset.download_url).call() {
                Ok(response) => {
                    let mut reader = response.into_body().into_reader();
                    if let Err(e) = std::io::copy(&mut reader, &mut file) {
                        let _ = tx.send(UpdateStatus::Error(format!("Download failed: {}", e)));
                        let _ = std::fs::remove_file(&temp_path);
                        return;
                    }
                    drop(file); // Close file before processing

                    // Process the downloaded file
                    if asset.name.ends_with(".zip") {
                        // Extract zip
                        match std::fs::File::open(&temp_path) {
                            Ok(zip_file) => match zip::ZipArchive::new(zip_file) {
                                Ok(mut archive) => match archive.by_index(0) {
                                    Ok(mut zipped_file) => {
                                        match std::fs::File::create(&staging_path) {
                                            Ok(mut exe_file) => {
                                                if std::io::copy(&mut zipped_file, &mut exe_file)
                                                    .is_ok()
                                                {
                                                    let _ = std::fs::remove_file(&temp_path);
                                                    let _ = tx.send(
                                                        UpdateStatus::UpdatedAndRestartRequired,
                                                    );
                                                } else {
                                                    let _ = tx.send(UpdateStatus::Error(
                                                        "Failed to extract zip".to_string(),
                                                    ));
                                                }
                                            }
                                            Err(e) => {
                                                let _ = tx.send(UpdateStatus::Error(format!(
                                                    "Failed to create staging file: {}",
                                                    e
                                                )));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx.send(UpdateStatus::Error(format!(
                                            "Failed to read zip entry: {}",
                                            e
                                        )));
                                    }
                                },
                                Err(e) => {
                                    let _ = tx.send(UpdateStatus::Error(format!(
                                        "Failed to open zip: {}",
                                        e
                                    )));
                                }
                            },
                            Err(e) => {
                                let _ = tx.send(UpdateStatus::Error(format!(
                                    "Failed to open temp file: {}",
                                    e
                                )));
                            }
                        }
                    } else {
                        // Direct exe - move to staging
                        match std::fs::rename(&temp_path, &staging_path) {
                            Ok(_) => {
                                let _ = tx.send(UpdateStatus::UpdatedAndRestartRequired);
                            }
                            Err(e) => {
                                let _ = tx.send(UpdateStatus::Error(format!(
                                    "Failed to stage exe: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(UpdateStatus::Error(format!("Download failed: {}", e)));
                    let _ = std::fs::remove_file(&temp_path);
                }
            }
        });
    }
}
