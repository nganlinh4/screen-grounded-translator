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

lazy_static::lazy_static! {
    static ref ALL_MODELS: Vec<ModelConfig> = vec![
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
            "Cân bằng",
            "균형",
            "Balanced",
            "meta-llama/llama-4-maverick-17b-128e-instruct",
            ModelType::Vision,
            true,
            "1000 lượt/ngày",
            "1000 요청/일",
            "1000 requests/day"
        ),
        ModelConfig::new(
            "gemma-3-27b-it",
            "google",
            "Chính xác",
            "정확함",
            "Accurate",
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
            "gemini-flash-lite-latest",
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
            "gemini-flash-latest",
            ModelType::Vision,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-pro",
            "google",
            "Siêu chính xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-robotics-er-1.5-preview",
            ModelType::Vision,
            true,
            "250 lượt/ngày",
            "250 요청/일",
            "250 requests/day"
        ),
        ModelConfig::new(
            "fast_text",
            "groq",
            "Cực nhanh",
            "초고속",
            "Super Fast",
            "openai/gpt-oss-20b",
            ModelType::Text,
            true,
            "1000 lượt/ngày",
            "1000 요청/일",
            "1000 requests/day"
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
            "text_gemma_3_27b_it",
            "google",
            "Cân bằng",
            "균형",
            "Balanced",
            "gemma-3-27b-it",
            ModelType::Text,
            true,
            "14400 lượt/ngày",
            "14400 요청/일",
            "14400 requests/day"
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
            "text_gemini_flash_lite",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-flash-lite-latest",
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
            "gemini-flash-latest",
            ModelType::Text,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "text_gemini_pro",
            "google",
            "Siêu chính xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-robotics-er-1.5-preview",
            ModelType::Text,
            true,
            "250 lượt/ngày",
            "250 요청/일",
            "250 requests/day"
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
            "gemini-audio",
            "google",
            "Chính xác hơn",
            "더 정확함",
            "More Accurate",
            "gemini-flash-lite-latest",
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
            "gemini-flash-latest",
            ModelType::Audio,
            true,
            "20 lượt/ngày",
            "20 요청/일",
            "20 requests/day"
        ),
        ModelConfig::new(
            "gemini-audio-pro",
            "google",
            "Siêu chính xác, chậm",
            "초정밀, 느림",
            "Super Accurate, Slow",
            "gemini-robotics-er-1.5-preview",
            ModelType::Audio,
            true,
            "250 lượt/ngày",
            "250 요청/일",
            "250 requests/day"
        ),
    ];
}

pub fn get_all_models() -> &'static [ModelConfig] {
    &ALL_MODELS
}

pub fn get_model_by_id(id: &str) -> Option<ModelConfig> {
    get_all_models().iter().find(|m| m.id == id).cloned()
}
