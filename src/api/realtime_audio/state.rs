//! Shared state for realtime transcription and translation

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Timeout for User Silence (Wait for user to finish thought)
/// Reduced from 2000ms to 800ms for snappier response with Parakeet
pub const USER_SILENCE_TIMEOUT_MS: u64 = 800;
/// Timeout for AI Silence (Wait if AI stops generating)
/// Reduced from 2000ms to 1000ms
pub const AI_SILENCE_TIMEOUT_MS: u64 = 1000;

/// Minimum characters required to trigger a force-commit on silence
const MIN_FORCE_COMMIT_CHARS: usize = 10;

// ============================================
// PARAKEET-SPECIFIC TIMEOUT CONSTANTS
// ============================================

pub const PARAKEET_BASE_TIMEOUT_MS: u64 = 800;
pub const PARAKEET_MIN_WORDS: usize = 2;
pub const PARAKEET_MIN_TIMEOUT_MS: u64 = 350;
pub const PARAKEET_TIMEOUT_DECAY_RATE: f64 = 2.5;

/// Transcription method being used
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TranscriptionMethod {
    GeminiLive,
    Parakeet,
}

impl Default for TranscriptionMethod {
    fn default() -> Self {
        TranscriptionMethod::GeminiLive
    }
}

pub struct RealtimeState {
    pub full_transcript: String,
    pub display_transcript: String,

    /// Position after the last FULLY FINISHED sentence that was translated
    pub last_committed_pos: usize,
    /// The length of full_transcript when we last triggered a translation
    pub last_processed_len: usize,

    pub committed_translation: String,
    pub uncommitted_translation: String,
    pub display_translation: String,

    pub translation_history: Vec<(String, String)>,

    pub last_transcript_append_time: Instant,
    pub last_translation_update_time: Instant,

    pub is_downloading: bool,
    pub download_title: String,
    pub download_message: String,
    pub download_progress: f32,

    pub transcription_method: TranscriptionMethod,
    pub parakeet_segment_start_time: Instant,
}

impl RealtimeState {
    pub fn new() -> Self {
        Self {
            full_transcript: String::new(),
            display_transcript: String::new(),
            last_committed_pos: 0,
            last_processed_len: 0,
            committed_translation: String::new(),
            uncommitted_translation: String::new(),
            display_translation: String::new(),
            translation_history: Vec::new(),
            last_transcript_append_time: Instant::now(),
            last_translation_update_time: Instant::now(),
            is_downloading: false,
            download_title: String::new(),
            download_message: String::new(),
            download_progress: 0.0,
            transcription_method: TranscriptionMethod::GeminiLive,
            parakeet_segment_start_time: Instant::now(),
        }
    }

    fn update_display_transcript(&mut self) {
        self.display_transcript = self.full_transcript.clone();
    }

    fn update_display_translation(&mut self) {
        let full = if self.committed_translation.is_empty() {
            self.uncommitted_translation.clone()
        } else if self.uncommitted_translation.is_empty() {
            self.committed_translation.clone()
        } else {
            format!(
                "{} {}",
                self.committed_translation, self.uncommitted_translation
            )
        };
        self.display_translation = full;
    }

    pub fn append_transcript(&mut self, new_text: &str) {
        if self.transcription_method == TranscriptionMethod::Parakeet {
            if self.last_committed_pos >= self.full_transcript.len() {
                self.parakeet_segment_start_time = Instant::now();
            }
        }

        let mut text_to_append = new_text.to_string();

        if self.transcription_method == TranscriptionMethod::Parakeet {
            let needs_cap =
                self.full_transcript.trim().is_empty() || self.source_ends_with_sentence();

            if needs_cap {
                if let Some(first_char_idx) = text_to_append.find(|c: char| !c.is_whitespace()) {
                    let c = text_to_append.chars().nth(first_char_idx).unwrap();
                    let pre_space = &text_to_append[..first_char_idx];
                    let rest = &text_to_append[first_char_idx + 1..];
                    text_to_append = format!("{}{}{}", pre_space, c.to_uppercase(), rest);
                }
            }
        }

        self.full_transcript.push_str(&text_to_append);
        self.last_transcript_append_time = Instant::now();
        self.update_display_transcript();
    }

    pub fn set_transcription_method(&mut self, method: TranscriptionMethod) {
        self.transcription_method = method;
        if method == TranscriptionMethod::Parakeet {
            self.parakeet_segment_start_time = Instant::now();
        }
    }

    fn count_uncommitted_words(&self) -> usize {
        if self.last_committed_pos >= self.full_transcript.len() {
            return 0;
        }
        if !self
            .full_transcript
            .is_char_boundary(self.last_committed_pos)
        {
            return 0;
        }
        let uncommitted = &self.full_transcript[self.last_committed_pos..];
        uncommitted.split_whitespace().count()
    }

    fn calculate_parakeet_timeout_ms(&self) -> u64 {
        let segment_len = if self.last_committed_pos >= self.full_transcript.len() {
            0
        } else if self
            .full_transcript
            .is_char_boundary(self.last_committed_pos)
        {
            self.full_transcript[self.last_committed_pos..].len()
        } else {
            0
        };

        let threshold = 30usize;
        if segment_len <= threshold {
            return PARAKEET_BASE_TIMEOUT_MS;
        }

        let excess_chars = segment_len - threshold;
        let decay = (excess_chars as f64 * PARAKEET_TIMEOUT_DECAY_RATE) as u64;

        let timeout = PARAKEET_BASE_TIMEOUT_MS.saturating_sub(decay);
        timeout.max(PARAKEET_MIN_TIMEOUT_MS)
    }

    pub fn source_ends_with_sentence(&self) -> bool {
        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];
        if self.last_committed_pos >= self.full_transcript.len() {
            return false;
        }
        let uncommitted_source = &self.full_transcript[self.last_committed_pos..];
        uncommitted_source
            .trim()
            .chars()
            .last()
            .map(|c| sentence_delimiters.contains(&c))
            .unwrap_or(false)
    }

    pub fn should_force_commit_on_timeout(&self) -> bool {
        if self.transcription_method == TranscriptionMethod::Parakeet {
            if self.last_committed_pos >= self.full_transcript.len() {
                return false;
            }
            let word_count = self.count_uncommitted_words();
            if word_count < PARAKEET_MIN_WORDS {
                return false;
            }
            let now = Instant::now();
            let user_timeout = self.calculate_parakeet_timeout_ms();
            let user_silent = now.duration_since(self.last_transcript_append_time)
                > Duration::from_millis(user_timeout);

            return user_silent;
        }

        if self.uncommitted_translation.is_empty() {
            return false;
        }

        if self.last_committed_pos < self.full_transcript.len() {
            let pending_len = self.full_transcript.len() - self.last_committed_pos;
            if pending_len < MIN_FORCE_COMMIT_CHARS {
                return false;
            }
        }

        let now = Instant::now();
        let user_silent = now.duration_since(self.last_transcript_append_time)
            > Duration::from_millis(USER_SILENCE_TIMEOUT_MS);
        let ai_silent = now.duration_since(self.last_translation_update_time)
            > Duration::from_millis(AI_SILENCE_TIMEOUT_MS);

        let source_ready = self.source_ends_with_sentence()
            || self.last_committed_pos < self.full_transcript.len();

        source_ready && user_silent && ai_silent
    }

    pub fn force_commit_all(&mut self) {
        if self.transcription_method == TranscriptionMethod::Parakeet {
            if self.last_committed_pos < self.full_transcript.len()
                && !self.source_ends_with_sentence()
            {
                self.full_transcript.push_str(". ");
                self.update_display_transcript();
            }
            return;
        }

        if self.uncommitted_translation.is_empty() {
            return;
        }

        let trans_segment = self.uncommitted_translation.trim().to_string();

        if !trans_segment.is_empty() {
            let source_segment = if self.last_committed_pos < self.full_transcript.len() {
                self.full_transcript[self.last_committed_pos..]
                    .trim()
                    .to_string()
            } else {
                "[continued]".to_string()
            };

            self.add_to_history(source_segment, trans_segment.clone());

            if self.committed_translation.is_empty() {
                self.committed_translation = trans_segment;
            } else {
                self.committed_translation.push(' ');
                self.committed_translation.push_str(&trans_segment);
            }

            self.last_committed_pos = self.full_transcript.len();
            self.uncommitted_translation.clear();
        }

        self.update_display_translation();
    }

    // ============================================
    // CRITICAL FIX: SMARTER CHUNKING
    // ============================================

    /// Get text to translate.
    /// CRITICAL CHANGE: If a sentence delimiter exists, ONLY return the text up to that delimiter.
    /// This ensures we don't send "finished sentence + half sentence" to the AI, which causes
    /// alignment issues and repetition when we commit.
    pub fn get_translation_chunk(&self) -> Option<(String, bool)> {
        if self.last_committed_pos >= self.full_transcript.len() {
            return None;
        }
        if !self
            .full_transcript
            .is_char_boundary(self.last_committed_pos)
        {
            return None;
        }

        let text = &self.full_transcript[self.last_committed_pos..];
        if text.trim().is_empty() {
            return None;
        }

        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];

        // Find the LAST delimiter in the chunk
        let mut split_idx: Option<usize> = None;
        for (i, c) in text.char_indices() {
            if sentence_delimiters.contains(&c) {
                // Include the delimiter length
                split_idx = Some(i + c.len_utf8());
            }
        }

        if let Some(idx) = split_idx {
            // We found a delimiter! Return ONLY the finished sentence(s).
            // This aligns perfectly with what commit_finished_sentences() will consume.
            let chunk = text[..idx].to_string();
            Some((chunk, true))
        } else {
            // No delimiter yet, return the whole incomplete buffer for live preview
            Some((text.to_string(), false))
        }
    }

    pub fn is_transcript_unchanged(&self) -> bool {
        self.full_transcript.len() == self.last_processed_len
    }

    pub fn update_last_processed_len(&mut self) {
        self.last_processed_len = self.full_transcript.len();
    }

    /// Commit source based on delimiters.
    pub fn commit_finished_sentences(&mut self) -> bool {
        let sentence_delimiters = ['.', '!', '?', '。', '！', '？'];

        if self.last_committed_pos >= self.full_transcript.len() {
            return false;
        }

        if !self
            .full_transcript
            .is_char_boundary(self.last_committed_pos)
        {
            return false;
        }

        let source_text = &self.full_transcript[self.last_committed_pos..];

        let mut last_delimiter_pos: Option<usize> = None;
        for (i, c) in source_text.char_indices() {
            if sentence_delimiters.contains(&c) {
                last_delimiter_pos = Some(i + c.len_utf8());
            }
        }

        let src_end = match last_delimiter_pos {
            Some(pos) => pos,
            None => return false,
        };

        let final_src_pos = self.last_committed_pos + src_end;
        self.last_committed_pos = final_src_pos;

        true
    }

    pub fn start_new_translation(&mut self) {
        self.uncommitted_translation.clear();
        self.update_display_translation();
    }

    pub fn commit_current_translation(&mut self) {
        let trans_segment = self.uncommitted_translation.trim().to_string();
        if !trans_segment.is_empty() {
            if self.committed_translation.is_empty() {
                self.committed_translation = trans_segment;
            } else {
                self.committed_translation.push(' ');
                self.committed_translation.push_str(&trans_segment);
            }
            self.uncommitted_translation.clear();
        }
        self.update_display_translation();
    }

    pub fn append_translation(&mut self, new_text: &str) {
        self.uncommitted_translation.push_str(new_text);
        self.last_translation_update_time = Instant::now();
        self.update_display_translation();
    }

    pub fn add_to_history(&mut self, source: String, translation: String) {
        self.translation_history.push((source, translation));
        while self.translation_history.len() > 3 {
            self.translation_history.remove(0);
        }
    }

    pub fn get_history_messages(&self, target_language: &str) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();

        for (source, translation) in &self.translation_history {
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("Translate to {}:\n{}", target_language, source)
            }));
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": translation
            }));
        }

        messages
    }
}

pub type SharedRealtimeState = Arc<Mutex<RealtimeState>>;
