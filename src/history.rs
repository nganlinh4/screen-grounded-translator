use chrono::Local;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use image::{ImageBuffer, Rgba};
use std::fs;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum HistoryType {
    Image,
    Audio,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: i64,
    pub timestamp: String,
    pub item_type: HistoryType,
    pub text: String,
    pub media_path: String,
}

pub enum HistoryAction {
    SaveImage { img: ImageBuffer<Rgba<u8>, Vec<u8>>, text: String },
    SaveAudio { wav_data: Vec<u8>, text: String },
    Delete(i64),
    ClearAll,
    Prune(usize),
}

pub struct HistoryManager {
    tx: Sender<HistoryAction>,
    pub items: Arc<Mutex<Vec<HistoryItem>>>,
}

impl HistoryManager {
    pub fn new(max_items: usize) -> Self {
        let (tx, rx) = channel();
        // Load initial items
        let (_, db_path, _) = get_paths();
        let initial_items = if db_path.exists() {
            let file = fs::File::open(&db_path).ok();
            if let Some(f) = file {
                serde_json::from_reader(f).unwrap_or_default()
            } else { Vec::new() }
        } else {
            Vec::new()
        };

        let items = Arc::new(Mutex::new(initial_items));
        let items_clone = items.clone();

        thread::spawn(move || {
            process_queue(rx, items_clone, max_items);
        });

        Self { tx, items }
    }

    pub fn save_image(&self, img: ImageBuffer<Rgba<u8>, Vec<u8>>, text: String) {
        let _ = self.tx.send(HistoryAction::SaveImage { img, text });
    }

    pub fn save_audio(&self, wav_data: Vec<u8>, text: String) {
        let _ = self.tx.send(HistoryAction::SaveAudio { wav_data, text });
    }

    pub fn delete(&self, id: i64) {
        let _ = self.tx.send(HistoryAction::Delete(id));
        let mut guard = self.items.lock().unwrap();
        if let Some(pos) = guard.iter().position(|x| x.id == id) {
            guard.remove(pos);
        }
    }

    pub fn clear_all(&self) {
        let _ = self.tx.send(HistoryAction::ClearAll);
        let mut guard = self.items.lock().unwrap();
        guard.clear();
    }

    pub fn request_prune(&self, limit: usize) {
        let _ = self.tx.send(HistoryAction::Prune(limit));
    }
}

fn get_paths() -> (PathBuf, PathBuf, PathBuf) {
    let config_dir = dirs::config_dir().unwrap_or_default().join("screen-goated-toolbox");
    let media_dir = config_dir.join("history_media");
    let db_path = config_dir.join("history.json");
    let _ = fs::create_dir_all(&media_dir);
    (config_dir, db_path, media_dir)
}

fn save_db(items: &Vec<HistoryItem>) {
    let (_, db_path, _) = get_paths();
    if let Ok(file) = fs::File::create(db_path) {
        let _ = serde_json::to_writer_pretty(file, items);
    }
}

fn process_queue(
    rx: Receiver<HistoryAction>, 
    cache: Arc<Mutex<Vec<HistoryItem>>>,
    mut max_items: usize
) {
    let (_, _, media_dir) = get_paths();

    while let Ok(action) = rx.recv() {
        let mut should_save = false;
        let mut items = cache.lock().unwrap();

        match action {
            HistoryAction::SaveImage { img, text } => {
                let now = Local::now();
                let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
                let filename = format!("img_{}.png", now.format("%Y%m%d_%H%M%S_%f"));
                let path = media_dir.join(&filename);
                let id = now.timestamp_nanos_opt().unwrap_or(0);
                
                if img.save(&path).is_ok() {
                    items.insert(0, HistoryItem {
                        id,
                        timestamp,
                        item_type: HistoryType::Image,
                        text,
                        media_path: filename,
                    });
                    should_save = true;
                }
            },
            HistoryAction::SaveAudio { wav_data, text } => {
                let now = Local::now();
                let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
                let filename = format!("audio_{}.wav", now.format("%Y%m%d_%H%M%S_%f"));
                let path = media_dir.join(&filename);
                let id = now.timestamp_nanos_opt().unwrap_or(0);
                
                if fs::write(&path, wav_data).is_ok() {
                    items.insert(0, HistoryItem {
                        id,
                        timestamp,
                        item_type: HistoryType::Audio,
                        text,
                        media_path: filename,
                    });
                    should_save = true;
                }
            },
            HistoryAction::Delete(id) => {
                if let Some(pos) = items.iter().position(|x| x.id == id) {
                    let item = items.remove(pos);
                    let _ = fs::remove_file(media_dir.join(item.media_path));
                    should_save = true;
                }
            },
            HistoryAction::ClearAll => {
                if let Ok(entries) = fs::read_dir(&media_dir) {
                    for entry in entries.flatten() {
                        let _ = fs::remove_file(entry.path());
                    }
                }
                items.clear();
                should_save = true;
            },
            HistoryAction::Prune(new_limit) => {
                max_items = new_limit;
                if items.len() > max_items {
                    while items.len() > max_items {
                        if let Some(item) = items.pop() {
                             let _ = fs::remove_file(media_dir.join(item.media_path));
                        }
                    }
                    should_save = true;
                }
            }
        }
        
        // Handle pruning after saves
        if items.len() > max_items {
             while items.len() > max_items {
                 if let Some(item) = items.pop() {
                      let _ = fs::remove_file(media_dir.join(item.media_path));
                 }
             }
             should_save = true;
        }

        if should_save {
            save_db(&items);
        }
    }
}
