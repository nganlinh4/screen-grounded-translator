//! Ollama API Integration
//! Supports local LLM inference with vision and text models

use anyhow::Result;
use image::{ImageBuffer, Rgba};
use base64::{Engine as _, engine::general_purpose};
use std::io::{Cursor, BufRead, BufReader};
use serde::Deserialize;
use super::client::UREQ_AGENT;
use crate::gui::locale::LocaleText;

/// Ollama streaming chunk response
#[derive(Deserialize, Debug)]
pub struct OllamaStreamChunk {
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub done: bool,
}

/// Ollama non-streaming response
#[derive(Deserialize, Debug)]
pub struct OllamaGenerateResponse {
    #[serde(default)]
    pub response: String,
}

/// Ollama model info from /api/tags
#[derive(Deserialize, Debug, Clone)]
pub struct OllamaModel {
    pub name: String,
}

/// Response from /api/tags
#[derive(Deserialize, Debug)]
pub struct OllamaTagsResponse {
    #[serde(default)]
    pub models: Vec<OllamaModel>,
}

/// Model with detected capabilities
#[derive(Clone, Debug)]
pub struct OllamaModelWithCaps {
    pub name: String,
    pub has_vision: bool,
}

/// Response from /api/show
#[derive(Deserialize, Debug)]
struct OllamaShowResponse {
    #[serde(default)]
    pub modelfile: String,
    #[serde(default)]
    pub details: OllamaModelDetails,
}

#[derive(Deserialize, Debug, Default)]
struct OllamaModelDetails {
    #[serde(default)]
    pub families: Vec<String>,
}

/// Fetch available models from Ollama
pub fn fetch_ollama_models(base_url: &str) -> Result<Vec<OllamaModel>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    
    let resp = UREQ_AGENT.get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to connect to Ollama: {}", e))?;
    
    let tags: OllamaTagsResponse = resp.into_json()
        .map_err(|e| anyhow::anyhow!("Failed to parse Ollama response: {}", e))?;
    
    Ok(tags.models)
}

/// Check if a model has vision capability by querying /api/show
fn check_model_has_vision(base_url: &str, model_name: &str) -> bool {
    let url = format!("{}/api/show", base_url.trim_end_matches('/'));
    
    let payload = serde_json::json!({
        "name": model_name
    });
    
    let resp = match UREQ_AGENT.post(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send_json(&payload) {
            Ok(r) => r,
            Err(_) => return false,
        };
    
    if let Ok(show_resp) = resp.into_json::<OllamaShowResponse>() {
        // Check families for vision-related names
        let families_str = show_resp.details.families.join(" ").to_lowercase();
        if families_str.contains("clip") || families_str.contains("vision") {
            return true;
        }
        
        // Check modelfile for projector (indicates vision capability)
        let modelfile_lower = show_resp.modelfile.to_lowercase();
        if modelfile_lower.contains("projector") || modelfile_lower.contains("vision") {
            return true;
        }
        
        // Check model name patterns for common vision models
        let name_lower = model_name.to_lowercase();
        if name_lower.contains("vision") || name_lower.contains("-vl") || 
           name_lower.contains("llava") || name_lower.contains("bakllava") ||
           name_lower.contains("moondream") || name_lower.contains("minicpm-v") {
            return true;
        }
    }
    
    false
}

/// Fetch models with their capabilities (vision/text)
pub fn fetch_ollama_models_with_caps(base_url: &str) -> Result<Vec<OllamaModelWithCaps>> {
    let models = fetch_ollama_models(base_url)?;
    
    let mut result = Vec::new();
    for model in models {
        let has_vision = check_model_has_vision(base_url, &model.name);
        result.push(OllamaModelWithCaps {
            name: model.name,
            has_vision,
        });
    }
    
    Ok(result)
}


/// Generate text with Ollama (text-only, no image)
pub fn ollama_generate_text<F>(
    base_url: &str,
    model: &str,
    prompt: &str,
    streaming_enabled: bool,
    ui_language: &str,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    
    let payload = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": streaming_enabled
    });
    
    let resp = UREQ_AGENT.post(&url)
        .timeout(std::time::Duration::from_secs(300))
        .send_json(&payload)
        .map_err(|e| anyhow::anyhow!("Ollama API Error: {}", e))?;
    
    let mut full_content = String::new();
    
    if streaming_enabled {
        let reader = BufReader::new(resp.into_reader());
        let mut thinking_shown = false;
        let mut content_started = false;
        let locale = LocaleText::get(ui_language);
        
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() { continue; }
            
            match serde_json::from_str::<OllamaStreamChunk>(&line) {
                Ok(chunk) => {
                    // Handle thinking tokens (qwen3 and similar models)
                    if let Some(thinking) = &chunk.thinking {
                        if !thinking.is_empty() && !thinking_shown && !content_started {
                            on_chunk(locale.model_thinking);
                            thinking_shown = true;
                        }
                    }
                    
                    // Handle response content
                    if !chunk.response.is_empty() {
                        if !content_started && thinking_shown {
                            // Wipe thinking message on first content
                            content_started = true;
                            full_content.push_str(&chunk.response);
                            let wipe_content = format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                            on_chunk(&wipe_content);
                        } else {
                            content_started = true;
                            full_content.push_str(&chunk.response);
                            on_chunk(&chunk.response);
                        }
                    }
                    
                    if chunk.done {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
    } else {
        let ollama_resp: OllamaGenerateResponse = resp.into_json()
            .map_err(|e| anyhow::anyhow!("Failed to parse Ollama response: {}", e))?;
        
        full_content = ollama_resp.response;
        on_chunk(&full_content);
    }
    
    Ok(full_content)
}

/// Generate with Ollama vision model (image + text)
pub fn ollama_generate_vision<F>(
    base_url: &str,
    model: &str,
    prompt: &str,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    streaming_enabled: bool,
    ui_language: &str,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    
    // Encode image as base64 PNG
    let mut image_data = Vec::new();
    image.write_to(&mut Cursor::new(&mut image_data), image::ImageFormat::Png)?;
    let b64_image = general_purpose::STANDARD.encode(&image_data);
    
    let payload = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "images": [b64_image],
        "stream": streaming_enabled
    });
    
    let resp = UREQ_AGENT.post(&url)
        .timeout(std::time::Duration::from_secs(300))
        .send_json(&payload)
        .map_err(|e| anyhow::anyhow!("Ollama Vision API Error: {}", e))?;
    
    let mut full_content = String::new();
    
    if streaming_enabled {
        let reader = BufReader::new(resp.into_reader());
        let mut thinking_shown = false;
        let mut content_started = false;
        let locale = LocaleText::get(ui_language);
        
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() { continue; }
            
            match serde_json::from_str::<OllamaStreamChunk>(&line) {
                Ok(chunk) => {
                    // Handle thinking tokens
                    if let Some(thinking) = &chunk.thinking {
                        if !thinking.is_empty() && !thinking_shown && !content_started {
                            on_chunk(locale.model_thinking);
                            thinking_shown = true;
                        }
                    }
                    
                    // Handle response content
                    if !chunk.response.is_empty() {
                        if !content_started && thinking_shown {
                            content_started = true;
                            full_content.push_str(&chunk.response);
                            let wipe_content = format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                            on_chunk(&wipe_content);
                        } else {
                            content_started = true;
                            full_content.push_str(&chunk.response);
                            on_chunk(&chunk.response);
                        }
                    }
                    
                    if chunk.done {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
    } else {
        let ollama_resp: OllamaGenerateResponse = resp.into_json()
            .map_err(|e| anyhow::anyhow!("Failed to parse Ollama response: {}", e))?;
        
        full_content = ollama_resp.response;
        on_chunk(&full_content);
    }
    
    Ok(full_content)
}
