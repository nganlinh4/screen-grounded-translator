//! Edge TTS voice list fetching and caching.

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Edge TTS voice information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeVoice {
    #[serde(rename = "ShortName")]
    pub short_name: String,
    #[serde(rename = "Gender")]
    pub gender: String,
    #[serde(rename = "Locale")]
    pub locale: String,
    #[serde(rename = "FriendlyName")]
    pub friendly_name: String,
}

/// Cached voice list state
pub struct EdgeVoiceCache {
    /// All voices fetched from Edge TTS
    pub voices: Vec<EdgeVoice>,
    /// Grouped by locale (e.g., "en-US" -> [voices])
    pub by_locale: HashMap<String, Vec<EdgeVoice>>,
    /// Grouped by language code (e.g., "en" -> [voices])
    pub by_language: HashMap<String, Vec<EdgeVoice>>,
    /// Whether the cache has been loaded
    pub loaded: bool,
    /// Loading in progress
    pub loading: bool,
    /// Error message if loading failed
    pub error: Option<String>,
}

impl Default for EdgeVoiceCache {
    fn default() -> Self {
        Self {
            voices: Vec::new(),
            by_locale: HashMap::new(),
            by_language: HashMap::new(),
            loaded: false,
            loading: false,
            error: None,
        }
    }
}

lazy_static! {
    /// Global cached Edge TTS voice list
    pub static ref EDGE_VOICE_CACHE: Mutex<EdgeVoiceCache> = Mutex::new(EdgeVoiceCache::default());
}

/// Start loading the Edge TTS voice list in a background thread
pub fn load_edge_voices_async() {
    // Check if already loaded or loading
    {
        let cache = EDGE_VOICE_CACHE.lock().unwrap();
        if cache.loaded || cache.loading {
            return;
        }
    }

    // Mark as loading
    {
        let mut cache = EDGE_VOICE_CACHE.lock().unwrap();
        cache.loading = true;
    }

    // Spawn background thread to fetch voices
    std::thread::spawn(|| {
        let url = "https://speech.platform.bing.com/consumer/speech/synthesize/readaloud/voices/list?trustedclienttoken=6A5AA1D4EAFF4E9FB37E23D68491D6F4";

        match ureq::get(url)
            .set(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .call()
        {
            Ok(response) => {
                match response.into_string() {
                    Ok(body) => {
                        match serde_json::from_str::<Vec<EdgeVoice>>(&body) {
                            Ok(voices) => {
                                let mut cache = EDGE_VOICE_CACHE.lock().unwrap();

                                // Group by locale
                                for voice in &voices {
                                    cache
                                        .by_locale
                                        .entry(voice.locale.clone())
                                        .or_insert_with(Vec::new)
                                        .push(voice.clone());

                                    // Group by language code (first part of locale)
                                    let lang_code = voice
                                        .locale
                                        .split('-')
                                        .next()
                                        .unwrap_or(&voice.locale)
                                        .to_lowercase();
                                    cache
                                        .by_language
                                        .entry(lang_code)
                                        .or_insert_with(Vec::new)
                                        .push(voice.clone());
                                }

                                cache.voices = voices;
                                cache.loaded = true;
                                cache.loading = false;
                                cache.error = None;
                            }
                            Err(e) => {
                                let mut cache = EDGE_VOICE_CACHE.lock().unwrap();
                                cache.loading = false;
                                cache.error = Some(format!("Parse error: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        let mut cache = EDGE_VOICE_CACHE.lock().unwrap();
                        cache.loading = false;
                        cache.error = Some(format!("Read error: {}", e));
                    }
                }
            }
            Err(e) => {
                let mut cache = EDGE_VOICE_CACHE.lock().unwrap();
                cache.loading = false;
                cache.error = Some(format!("Network error: {}", e));
            }
        }
    });
}

/// Get all unique languages with their display names
/// Languages are extracted dynamically from the Edge TTS voice list FriendlyName field
pub fn get_available_languages() -> Vec<(String, String)> {
    let cache = EDGE_VOICE_CACHE.lock().unwrap();
    if !cache.loaded {
        return Vec::new();
    }

    // Build a map of language code -> language name from actual voice data
    let mut lang_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for voice in &cache.voices {
        let lang_code = voice
            .locale
            .split('-')
            .next()
            .unwrap_or(&voice.locale)
            .to_lowercase();

        // Don't overwrite if we already have a name for this language
        if lang_map.contains_key(&lang_code) {
            continue;
        }

        // Extract language name from FriendlyName
        // Format: "Microsoft Xxx Online (Natural) - Language (Region)"
        // We want just "Language" for clarity
        if let Some(dash_pos) = voice.friendly_name.rfind(" - ") {
            let lang_region = &voice.friendly_name[dash_pos + 3..];
            // Get just the language part (before parentheses with region)
            if let Some(paren_pos) = lang_region.find(" (") {
                let lang_only = &lang_region[..paren_pos];
                lang_map.insert(lang_code, lang_only.to_string());
            } else {
                lang_map.insert(lang_code, lang_region.to_string());
            }
        }
    }

    let mut languages: Vec<(String, String)> = lang_map.into_iter().collect();
    languages.sort_by(|a, b| a.1.cmp(&b.1));
    languages
}

/// Get voices for a specific language code
pub fn get_voices_for_language(lang_code: &str) -> Vec<EdgeVoice> {
    let cache = EDGE_VOICE_CACHE.lock().unwrap();
    cache
        .by_language
        .get(&lang_code.to_lowercase())
        .cloned()
        .unwrap_or_default()
}
