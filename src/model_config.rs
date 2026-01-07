/// Centralized Model Configuration

#[derive(Clone, Debug, PartialEq)]
pub enum ModelType {
    Vision,
    Text,
    Audio,
}

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub id: String,
    pub provider: String,
    pub name_vi: String,
    pub name_ko: String,
    pub name_en: String,
    pub full_name: String,
    pub model_type: ModelType,
    pub enabled: bool,
    pub quota_limit_vi: String,
    pub quota_limit_ko: String,
    pub quota_limit_en: String,
}

impl ModelConfig {
    pub fn new(
        id: &str,
        provider: &str,
        name_vi: &str,
        name_ko: &str,
        name_en: &str,
        full_name: &str,
        model_type: ModelType,
        enabled: bool,
        quota_limit_vi: &str,
        quota_limit_ko: &str,
        quota_limit_en: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            provider: provider.to_string(),
            name_vi: name_vi.to_string(),
            name_ko: name_ko.to_string(),
            name_en: name_en.to_string(),
            full_name: full_name.to_string(),
            model_type,
            enabled,
            quota_limit_vi: quota_limit_vi.to_string(),
            quota_limit_ko: quota_limit_ko.to_string(),
            quota_limit_en: quota_limit_en.to_string(),
        }
    }
}

/// Check if a model is a non-LLM model (doesn't use prompts)
/// These are specialized models that process input directly without instructions.
pub fn model_is_non_llm(model_id: &str) -> bool {
    match model_id {
        // QR Scanner - just decodes QR codes
        "qr-scanner" => true,
        // Google Translate (GTX) - translation only, language from instruction
        "google-gtx" => true,
        // Whisper models - speech-to-text only
        "whisper-fast" | "whisper-accurate" => true,
        _ => false,
    }
}

lazy_static::lazy_static! {
    static ref ALL_MODELS: Vec<ModelConfig> = vec![
        ModelConfig::new(
            "google-gtx",
            "google-gtx",
            "Google Dịch",
            "Google 번역",
            "Google Translate",
            "translate.googleapis.com/gtx",
            ModelType::Text,
            true,
            "Không giới hạn",
            "무제한",
            "Unlimited"
        ),
        ModelConfig::new(
            "qr-scanner",
            "qrserver",
            "Quét mã QR",
            "QR 스캔",
            "QR Scanner",
            "api.qrserver.com/read-qr-code",
            ModelType::Vision,
            true,
            "Không giới hạn",
            "무제한",
            "Unlimited"
        ),
        ModelConfig::new(
            "scout",
            "groq",
            "Nhanh",
            "빠름",
            "Fast",
            "meta-llama/llama-4-scout-17b-16e-instruct",
            ModelType::Vision,
            true,
            "1000 lượt/ngày",
            "1000 요청/일",
            "1000 requests/day"
        ),
        ModelConfig::new(
            "maverick",
            "groq",
            "Chính xác",
            "정확함",
            "Accurate",
            "meta-llama/llama-4-maverick-17b-128e-instruct",
            ModelType::Vision,
            true,
            "1000 lượt/ngày",
            "1000 요청/일",
            "1000 requests/day"
        ),
        ModelConfig::new(
            "gemini-live-vision",
            "gemini-live",
            "Thử nghiệm",
            "실험적",
            "Experimental",
            "gemini-2.5-flash-native-audio-preview-12-2025",
            ModelType::Vision,
            true,
            "Không giới hạn",
            "무제한",
            "Unlimited"
        ),
        ModelConfig::new(
            "gemma-3-27b-vision",
            "google",
            "Cân bằng, chậm",
            "균형잡힌, 느림",
            "Balanced, Slow",
            "gemma-3-27b-it",
            ModelType::Vision,
            true,
            "14400 lượt/ngày",
            "14400 요청/일",
            "14400 requests/day"
        ),
        ModelConfig::new(
            "gemini-flash-lite",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-2.5-flash-lite",
            ModelType::Vision,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-flash",
            "google",
            "Rất chính xác",
            "매우 정확함",
            "Very Accurate",
            "gemini-2.5-flash",
            ModelType::Vision,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-pro",
            "google",
            "Siêu ch.xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-robotics-er-1.5-preview",
            ModelType::Vision,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-3-flash-preview",
            "google",
            "Siêu chính xác",
            "초정밀",
            "Super Accurate",
            "gemini-3-flash-preview",
            ModelType::Vision,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "or-nemotron-vl",
            "openrouter",
            "OR-Cân bằng",
            "OR-균형",
            "OR-Balanced",
            "nvidia/nemotron-nano-12b-v2-vl:free",
            ModelType::Vision,
            true,
            "50 lượt chung/ngày",
            "50 공유 요청/일",
            "50 shared requests/day"
        ),
        ModelConfig::new(
            "text_fast_120b",
            "groq",
            "Nhanh",
            "빠름",
            "Fast",
            "openai/gpt-oss-120b",
            ModelType::Text,
            true,
            "1000 lượt/ngày",
            "1000 요청/일",
            "1000 requests/day"
        ),
        ModelConfig::new(
            "text_accurate_kimi",
            "groq",
            "Chính xác",
            "정확함",
            "Accurate",
            "moonshotai/kimi-k2-instruct-0905",
            ModelType::Text,
            true,
            "1000 lượt/ngày",
            "1000 요청/일",
            "1000 requests/day"
        ),
        ModelConfig::new(
            "compound_mini",
            "groq",
            "Search nhanh",
            "빠른 검색",
            "Quick Search",
            "groq/compound-mini",
            ModelType::Text,
            true,
            "250 lượt/ngày",
            "250 요청/일",
            "250 requests/day"
        ),
        ModelConfig::new(
            "compound",
            "groq",
            "Search kỹ",
            "상세 검색",
            "Deep Search",
            "groq/compound",
            ModelType::Text,
            true,
            "250 lượt/ngày",
            "250 요청/일",
            "250 requests/day"
        ),
        ModelConfig::new(
            "cerebras_llama33_70b",
            "cerebras",
            "C-Nhanh",
            "C-빠름",
            "C-Fast",
            "llama-3.3-70b",
            ModelType::Text,
            true,
            "14400 lượt/ngày",
            "14400 요청/일",
            "14400 requests/day"
        ),
        ModelConfig::new(
            "cerebras_gpt_oss",
            "cerebras",
            "C-Chính xác",
            "C-정확함",
            "C-Accurate",
            "gpt-oss-120b",
            ModelType::Text,
            true,
            "14400 lượt/ngày",
            "14400 요청/일",
            "14400 requests/day"
        ),
        ModelConfig::new(
            "cerebras_qwen3",
            "cerebras",
            "C-Rất chính xác",
            "C-매우 정확함",
            "C-Very Accurate",
            "qwen-3-235b-a22b-instruct-2507",
            ModelType::Text,
            true,
            "1440 lượt/ngày",
            "1440 요청/일",
            "1440 requests/day"
        ),
        ModelConfig::new(
            "cerebras_zai_glm",
            "cerebras",
            "C-Siêu chính xác",
            "C-초정밀",
            "C-Super Accurate",
            "zai-glm-4.6",
            ModelType::Text,
            true,
            "100 lượt/ngày",
            "100 요청/일",
            "100 requests/day"
        ),
        ModelConfig::new(
            "gemini-live-text",
            "gemini-live",
            "Thử nghiệm",
            "실험적",
            "Experimental",
            "gemini-2.5-flash-native-audio-preview-12-2025",
            ModelType::Text,
            true,
            "Không giới hạn",
            "무제한",
            "Unlimited"
        ),
        ModelConfig::new(
            "gemma-3-27b",
            "google",
            "Cân bằng, chậm",
            "균형잡힌, 느림",
            "Balanced, Slow",
            "gemma-3-27b-it",
            ModelType::Text,
            true,
            "14400 lượt/ngày",
            "14400 요청/일",
            "14400 requests/day"
        ),
        ModelConfig::new(
            "text_gemini_flash_lite",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-2.5-flash-lite",
            ModelType::Text,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "text_gemini_flash",
            "google",
            "Rất chính xác",
            "매우 정확함",
            "Very Accurate",
            "gemini-2.5-flash",
            ModelType::Text,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "text_gemini_pro",
            "google",
            "Siêu ch.xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-robotics-er-1.5-preview",
            ModelType::Text,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "text_gemini_3_0_flash",
            "google",
            "Siêu chính xác",
            "초정밀",
            "Super Accurate",
            "gemini-3-flash-preview",
            ModelType::Text,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "or-nemotron-text",
            "openrouter",
            "OR-Nhanh",
            "OR-빠름",
            "OR-Fast",
            "nvidia/nemotron-3-nano-30b-a3b:free",
            ModelType::Text,
            true,
            "50 lượt chung/ngày",
            "50 공유 요청/일",
            "50 shared requests/day"
        ),
        ModelConfig::new(
            "or-mimo",
            "openrouter",
            "OR-Cân bằng",
            "OR-균형",
            "OR-Balanced",
            "xiaomi/mimo-v2-flash:free",
            ModelType::Text,
            true,
            "50 lượt chung/ngày",
            "50 공유 요청/일",
            "50 shared requests/day"
        ),
        ModelConfig::new(
            "or-deepseek-chimera",
            "openrouter",
            "OR-Ch.xác, chậm",
            "OR-정확, 느림",
            "OR-Accurate, Slow",
            "tngtech/deepseek-r1t2-chimera:free",
            ModelType::Text,
            true,
            "50 lượt chung/ngày",
            "50 공유 요청/일",
            "50 shared requests/day"
        ),
        ModelConfig::new(
            "or-kat-coder",
            "openrouter",
            "OR-Chính xác",
            "OR-정확함",
            "OR-Accurate",
            "kwaipilot/kat-coder-pro:free",
            ModelType::Text,
            true,
            "50 lượt chung/ngày",
            "50 공유 요청/일",
            "50 shared requests/day"
        ),
        ModelConfig::new(
            "or-devstral",
            "openrouter",
            "OR-Rất ch.xác",
            "OR-매우 정확",
            "OR-Very Accurate",
            "mistralai/devstral-2512:free",
            ModelType::Text,
            true,
            "50 lượt chung/ngày",
            "50 공유 요청/일",
            "50 shared requests/day"
        ),

        ModelConfig::new(
            "whisper-fast",
            "groq",
            "Nhanh",
            "빠름",
            "Fast",
            "whisper-large-v3-turbo",
            ModelType::Audio,
            true,
            "8 giờ audio/ngày",
            "8시간 오디오/일",
            "8 hours audio/day"
        ),
        ModelConfig::new(
            "whisper-accurate",
            "groq",
            "Chính xác",
            "정확함",
            "Accurate",
            "whisper-large-v3",
            ModelType::Audio,
            true,
            "8 giờ audio/ngày",
            "8시간 오디오/일",
            "8 hours audio/day"
        ),
        ModelConfig::new(
            "gemini-live-audio",
            "gemini-live",
            "Thử nghiệm",
            "실험적",
            "Experimental",
            "gemini-2.5-flash-native-audio-preview-12-2025",
            ModelType::Audio,
            true,
            "Không giới hạn",
            "무제한",
            "Unlimited"
        ),
        ModelConfig::new(
            "gemini-audio",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-2.5-flash-lite",
            ModelType::Audio,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-audio-flash",
            "google",
            "Rất chính xác",
            "매우 정확함",
            "Very Accurate",
            "gemini-2.5-flash",
            ModelType::Audio,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-audio-pro",
            "google",
            "Siêu ch.xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-robotics-er-1.5-preview",
            ModelType::Audio,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-audio-3.0-flash",
            "google",
            "Siêu chính xác",
            "초정밀",
            "Super Accurate",
            "gemini-3-flash-preview",
            ModelType::Audio,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "parakeet-local",
            "parakeet",
            "Offline (Nhanh)",
            "오프라인 (빠름)",
            "Offline (Fast)",
            "parakeet-120m-v1",
            ModelType::Audio,
            true,
            "Không giới hạn",
            "무제한",
            "Unlimited"
        ),

    ];
}

pub fn get_all_models() -> &'static [ModelConfig] {
    &ALL_MODELS
}

pub fn get_model_by_id(id: &str) -> Option<ModelConfig> {
    get_all_models().iter().find(|m| m.id == id).cloned()
}

/// Resolve a fallback model for retry logic
/// Prioritizes:
/// 1. Same provider, same type (Prioritize based on list order - treating list as priority queue)
/// 2. Different provider, same type
use crate::config::Config;

/// Resolve a fallback model for retry logic
/// Prioritizes:
/// 1. Same provider, same type (Prioritize based on list order - treating list as priority queue)
/// 2. Different provider, same type
/// Checks if the provider is actually configured (has API key) before suggesting it.
pub fn resolve_fallback_model(
    failed_model_id: &str,
    failed_model_ids: &[String],
    current_model_type: &ModelType,
    config: &Config,
) -> Option<ModelConfig> {
    let all_models = get_all_models_with_ollama();
    let current_model_opt = get_model_by_id(failed_model_id);
    let current_provider = current_model_opt
        .as_ref()
        .map(|m| m.provider.as_str())
        .unwrap_or("");

    // Helper to check if a provider is configured
    let is_provider_configured = |provider: &str| -> bool {
        match provider {
            "groq" => !config.api_key.is_empty(),
            "google" => !config.gemini_api_key.is_empty(),
            "openai" => false, // We don't have openai_api_key in config struct (only openrouter/cerebras) - wait, checking Config struct..
            // Ah, standard OpenAI is not in the Config struct I saw.
            "openrouter" => !config.openrouter_api_key.is_empty(),
            "cerebras" => !config.cerebras_api_key.is_empty(),
            "ollama" => config.use_ollama, // No key needed, just enabled
            _ => true, // Assume others (like internal ones) are "configured" or we can't check
        }
    };

    // 1. Try Same Provider
    if !current_provider.is_empty() {
        // If the current provider itself isn't configured (e.g. key removed during run?), we shouldn't retry same provider
        // but likely it WAS configured if we just failed a request.

        let same_provider_candidates: Vec<&ModelConfig> = all_models
            .iter()
            .filter(|m| {
                m.provider == current_provider
                    && m.model_type == *current_model_type
                    && m.id != failed_model_id
                    && !failed_model_ids.contains(&m.id)
            })
            .collect();

        if !same_provider_candidates.is_empty() {
            return Some((*same_provider_candidates[0]).clone());
        }
    }

    // 2. Try Different Provider
    let diff_provider_candidates: Vec<&ModelConfig> = all_models
        .iter()
        .filter(|m| {
            m.provider != current_provider
                && m.model_type == *current_model_type
                && !failed_model_ids.contains(&m.id)
                && is_provider_configured(&m.provider)
        })
        .collect();

    if !diff_provider_candidates.is_empty() {
        // Preference for "Google" as a reliable fallback for Text/Vision
        if let Some(google_model) = diff_provider_candidates
            .iter()
            .find(|m| m.provider == "google")
        {
            return Some((**google_model).clone());
        }
        // Fallback to first available
        return Some((*diff_provider_candidates[0]).clone());
    }

    None
}

/// Get all models including dynamically fetched Ollama models
/// This combines static models with Ollama models (if Ollama is enabled)
pub fn get_all_models_with_ollama() -> Vec<ModelConfig> {
    let mut models: Vec<ModelConfig> = ALL_MODELS.iter().cloned().collect();

    // Add cached Ollama models
    let cached = OLLAMA_MODEL_CACHE.lock().unwrap();
    for ollama_model in cached.iter() {
        models.push(ollama_model.clone());
    }

    models
}

/// Check if a model supports search capabilities (grounding/web search) by its Full Name (API Name)
pub fn model_supports_search_by_name(full_name: &str) -> bool {
    // Exclusions
    if full_name.contains("gemma-3-27b-it") {
        return false;
    }
    if full_name.contains("gemini-3-flash-preview") {
        return false;
    }

    // Inclusions
    if full_name.contains("gemini") {
        return true;
    }
    if full_name.contains("gemma") {
        return true;
    }
    if full_name.contains("compound") {
        return true;
    }

    false
}

/// Check if a model supports search capabilities (grounding/web search) by its Internal ID
pub fn model_supports_search_by_id(id: &str) -> bool {
    if let Some(conf) = get_model_by_id(id) {
        return model_supports_search_by_name(&conf.full_name);
    }

    // Fallback logic for models not in static config (though currently most are)
    if id.contains("compound") {
        return true;
    }

    false
}

// === OLLAMA MODEL CACHE ===

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

lazy_static::lazy_static! {
    /// Cached Ollama models (populated by background scan)
    static ref OLLAMA_MODEL_CACHE: Mutex<Vec<ModelConfig>> = Mutex::new(Vec::new());

    /// Whether a scan is currently in progress
    static ref OLLAMA_SCAN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

    /// Last scan time (for debouncing) - initialized to 10s ago so first scan works immediately
    static ref OLLAMA_LAST_SCAN: Mutex<std::time::Instant> = Mutex::new(
        std::time::Instant::now().checked_sub(std::time::Duration::from_secs(10)).unwrap_or_else(std::time::Instant::now)
    );
}

/// Check if Ollama model scan is in progress
pub fn is_ollama_scan_in_progress() -> bool {
    OLLAMA_SCAN_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Trigger background scan for Ollama models (non-blocking)
/// Returns immediately, models will be populated in cache when ready
pub fn trigger_ollama_model_scan() {
    // Check if Ollama is enabled
    let (use_ollama, base_url) = if let Ok(app) = crate::APP.lock() {
        (app.config.use_ollama, app.config.ollama_base_url.clone())
    } else {
        return;
    };

    if !use_ollama {
        return;
    }

    // Debounce: don't scan more than once per 5 seconds
    {
        let last_scan = OLLAMA_LAST_SCAN.lock().unwrap();
        if last_scan.elapsed().as_secs() < 5 {
            return;
        }
    }

    // Check if already scanning
    if OLLAMA_SCAN_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return; // Already scanning
    }

    // Update last scan time
    {
        let mut last_scan = OLLAMA_LAST_SCAN.lock().unwrap();
        *last_scan = std::time::Instant::now();
    }

    // Spawn background thread to scan
    std::thread::spawn(move || {
        let result = crate::api::ollama::fetch_ollama_models_with_caps(&base_url);

        if let Ok(ollama_models) = result {
            let mut new_models = Vec::new();

            for ollama_model in ollama_models {
                // Create model ID from name (e.g., "qwen3-vl:2b" -> "ollama-qwen3-vl-2b")
                let model_id = format!(
                    "ollama-{}",
                    ollama_model.name.replace(":", "-").replace("/", "-")
                );
                let display_name = format!("{} (Local)", ollama_model.name);

                // Vision models can do BOTH vision and text, so we add them to both
                // Text-only models just get Text type
                if ollama_model.has_vision {
                    // Add as Vision model
                    new_models.push(ModelConfig {
                        id: format!("{}-vision", model_id),
                        provider: "ollama".to_string(),
                        name_vi: display_name.clone(),
                        name_ko: display_name.clone(),
                        name_en: display_name.clone(),
                        full_name: ollama_model.name.clone(),
                        model_type: ModelType::Vision,
                        enabled: true,
                        quota_limit_vi: "Không giới hạn".to_string(),
                        quota_limit_ko: "무제한".to_string(),
                        quota_limit_en: "Unlimited".to_string(),
                    });

                    // Also add as Text model (vision models can do text too)
                    new_models.push(ModelConfig {
                        id: model_id,
                        provider: "ollama".to_string(),
                        name_vi: display_name.clone(),
                        name_ko: display_name.clone(),
                        name_en: display_name.clone(),
                        full_name: ollama_model.name.clone(),
                        model_type: ModelType::Text,
                        enabled: true,
                        quota_limit_vi: "Không giới hạn".to_string(),
                        quota_limit_ko: "무제한".to_string(),
                        quota_limit_en: "Unlimited".to_string(),
                    });
                } else {
                    // Text-only model
                    new_models.push(ModelConfig {
                        id: model_id,
                        provider: "ollama".to_string(),
                        name_vi: display_name.clone(),
                        name_ko: display_name.clone(),
                        name_en: display_name,
                        full_name: ollama_model.name,
                        model_type: ModelType::Text,
                        enabled: true,
                        quota_limit_vi: "Không giới hạn".to_string(),
                        quota_limit_ko: "무제한".to_string(),
                        quota_limit_en: "Unlimited".to_string(),
                    });
                }
            }

            // Update cache
            let mut cache = OLLAMA_MODEL_CACHE.lock().unwrap();
            *cache = new_models;
        }

        OLLAMA_SCAN_IN_PROGRESS.store(false, Ordering::SeqCst);
    });
}
