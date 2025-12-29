//! Persistent input history for text inputs
//! Supports arrow up/down navigation through previous inputs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// Maximum number of history entries to keep
const MAX_HISTORY_SIZE: usize = 100;

lazy_static::lazy_static! {
    /// Global input history manager
    pub static ref INPUT_HISTORY: Mutex<InputHistory> = Mutex::new(InputHistory::load());
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputHistory {
    /// List of previous inputs (newest first)
    entries: Vec<String>,

    /// Current navigation index (-1 means not navigating)
    #[serde(skip)]
    nav_index: i32,

    /// Temporary storage for current input when navigating
    #[serde(skip)]
    current_draft: String,
}

impl Default for InputHistory {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            nav_index: -1,
            current_draft: String::new(),
        }
    }
}

impl InputHistory {
    /// Get the path to the history file
    fn history_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_default()
            .join("screen-goated-toolbox");
        let _ = fs::create_dir_all(&config_dir);
        config_dir.join("input_history.json")
    }

    /// Load history from disk
    pub fn load() -> Self {
        let path = Self::history_path();
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(history) = serde_json::from_str::<InputHistory>(&data) {
                    return history;
                }
            }
        }
        Self::default()
    }

    /// Save history to disk
    fn save(&self) {
        let path = Self::history_path();
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, data);
        }
    }

    /// Add a new entry to history (called on submit)
    pub fn add_entry(&mut self, text: &str) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }

        // Remove duplicate if exists (so we can move it to the end)
        self.entries.retain(|e| e != &text);

        // Add to end (Newest)
        self.entries.push(text);

        // Limit size (remove from front/Oldest)
        while self.entries.len() > MAX_HISTORY_SIZE {
            self.entries.remove(0);
        }

        // Reset navigation state
        self.reset_navigation();

        // Persist
        self.save();
    }

    /// Start navigation (called when arrow key pressed on empty navigation state)
    /// Returns the text to show, or None if no history
    pub fn navigate_up(&mut self, current_text: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        // If not currently navigating, start at the end (Newest + 1)
        if self.nav_index == -1 {
            self.current_draft = current_text.to_string();
            self.nav_index = self.entries.len() as i32;
        }

        // Move up (towards older entries / index 0)
        if self.nav_index > 0 {
            self.nav_index -= 1;
        }

        // Return entry at current index
        if (self.nav_index as usize) < self.entries.len() {
            self.entries.get(self.nav_index as usize).cloned()
        } else {
            None
        }
    }

    /// Navigate down (towards newer entries or back to draft)
    /// Returns the text to show
    pub fn navigate_down(&mut self, _current_text: &str) -> Option<String> {
        if self.nav_index == -1 {
            return None;
        }

        // If we haven't started (at -1), we can't go down.
        // Logic handles nav_index = len() as "Draft".

        if (self.nav_index as usize) < self.entries.len() {
            self.nav_index += 1;
        }

        if (self.nav_index as usize) >= self.entries.len() {
            // Returned to draft
            let draft = self.current_draft.clone();
            self.reset_navigation();
            Some(draft)
        } else {
            self.entries.get(self.nav_index as usize).cloned()
        }
    }

    /// Reset navigation state (called when input is hidden or submitted)
    pub fn reset_navigation(&mut self) {
        self.nav_index = -1;
        self.current_draft.clear();
    }
}

/// Convenience function to add entry to global history
pub fn add_to_history(text: &str) {
    if let Ok(mut history) = INPUT_HISTORY.lock() {
        history.add_entry(text);
    }
}

/// Navigate up in history, returns text to show or None
pub fn navigate_history_up(current_text: &str) -> Option<String> {
    if let Ok(mut history) = INPUT_HISTORY.lock() {
        history.navigate_up(current_text)
    } else {
        None
    }
}

/// Navigate down in history, returns text to show or None
pub fn navigate_history_down(current_text: &str) -> Option<String> {
    if let Ok(mut history) = INPUT_HISTORY.lock() {
        history.navigate_down(current_text)
    } else {
        None
    }
}

/// Reset history navigation
pub fn reset_history_navigation() {
    if let Ok(mut history) = INPUT_HISTORY.lock() {
        history.reset_navigation();
    }
}
