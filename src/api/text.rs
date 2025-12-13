use anyhow::Result;
use std::io::{BufRead, BufReader};
use crate::APP;
use crate::overlay::result::RefineContext;
use super::client::UREQ_AGENT;
use super::types::{StreamChunk, ChatCompletionResponse};
use super::vision::translate_image_streaming as vision_translate_image_streaming;

pub fn translate_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    text: String,
    target_lang: String,
    model: String,
    provider: String,
    streaming_enabled: bool,
    use_json_format: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let mut full_content = String::new();
    let prompt = format!(
        "Translate the following text to {}. Output ONLY the translation. Text:\n\n{}",
        target_lang, text
    );

    if provider == "google" {
        // --- GEMINI TEXT API ---
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:google"));
        }

        let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
        let url = if streaming_enabled {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
                model, method
            )
        } else {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
            )
        };

        let payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }]
        });

        let resp = UREQ_AGENT.post(&url)
            .set("x-goog-api-key", gemini_api_key)
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("Gemini Text API Error: {}", err_str)
                }
            })?;

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            for line in reader.lines() {
                let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
                if line.starts_with("data: ") {
                    let json_str = &line["data: ".len()..];
                    if json_str.trim() == "[DONE]" { break; }

                    if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                            if let Some(first_candidate) = candidates.first() {
                                if let Some(parts) = first_candidate.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                    if let Some(first_part) = parts.first() {
                                        if let Some(text) = first_part.get("text").and_then(|t| t.as_str()) {
                                            full_content.push_str(text);
                                            on_chunk(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let chat_resp: serde_json::Value = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
                if let Some(first_choice) = candidates.first() {
                    if let Some(parts) = first_choice.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                        full_content = parts.iter()
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<String>();
                        on_chunk(&full_content);
                    }
                }
            }
        }

    } else {
        // --- GROQ API (Default) ---
        if groq_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:groq"));
        }

        let payload = if streaming_enabled {
            serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": true
            })
        } else {
            let mut payload_obj = serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": prompt }
                ],
                "stream": false
            });
            
            if use_json_format {
                payload_obj["response_format"] = serde_json::json!({ "type": "json_object" });
            }
            
            payload_obj
        };

        let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
            .set("Authorization", &format!("Bearer {}", groq_api_key))
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("{}", err_str)
                }
            })?;

        // --- CAPTURE RATE LIMITS ---
        if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
             let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
             let usage_str = format!("{} / {}", remaining, limit);
             
             if let Ok(mut app) = APP.lock() {
                 app.model_usage_stats.insert(model.clone(), usage_str);
             }
        }
        // ---------------------------

        if streaming_enabled {
            let reader = BufReader::new(resp.into_reader());
            
            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" { break; }
                    
                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            if let Some(content) = chunk.choices.get(0)
                                .and_then(|c| c.delta.content.as_ref()) {
                                full_content.push_str(content);
                                on_chunk(content);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                let content_str = &choice.message.content;
                
                if use_json_format {
                    if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(content_str) {
                        if let Some(translation) = json_obj.get("translation").and_then(|v| v.as_str()) {
                            full_content = translation.to_string();
                        } else {
                            full_content = content_str.clone();
                        }
                    } else {
                        full_content = content_str.clone();
                    }
                } else {
                    full_content = content_str.clone();
                }
                
                on_chunk(&full_content);
            }
        }
    }

    Ok(full_content)
}

pub fn refine_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    context: RefineContext,
    previous_text: String,
    user_prompt: String,
    original_model_id: &str,
    original_provider: &str,
    streaming_enabled: bool,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let final_prompt = format!(
        "Content:\n{}\n\nInstruction:\n{}\n\nOutput ONLY the result.",
        previous_text, user_prompt
    );

    let (mut target_id_or_name, mut target_provider) = match context {
        RefineContext::Image(_) => {
            (original_model_id.to_string(), original_provider.to_string())
        },
        _ => {
            if !original_model_id.trim().is_empty() && original_model_id != "scout" {
                 (original_model_id.to_string(), original_provider.to_string())
            } else {
                if !gemini_api_key.trim().is_empty() {
                     ("gemini-flash-lite".to_string(), "google".to_string()) 
                } else if !groq_api_key.trim().is_empty() {
                     ("text_accurate_kimi".to_string(), "groq".to_string()) 
                } else {
                     (original_model_id.to_string(), original_provider.to_string())
                }
            }
        }
    };

    if let Some(conf) = crate::model_config::get_model_by_id(&target_id_or_name) {
        target_id_or_name = conf.full_name;
        target_provider = conf.provider;
    }
    
    let mut exec_text_only = |p_model: String, p_provider: String| -> Result<String> {
        let mut full_content = String::new();

        if p_provider == "google" {
             if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY:google")); }
             
             let method = if streaming_enabled { "streamGenerateContent" } else { "generateContent" };
             let url = if streaming_enabled {
                 format!("https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse", p_model, method)
             } else {
                 format!("https://generativelanguage.googleapis.com/v1beta/models/{}:{}", p_model, method)
             };

             let payload = serde_json::json!({
                 "contents": [{ "role": "user", "parts": [{ "text": final_prompt }] }]
             });

             let resp = UREQ_AGENT.post(&url)
                 .set("x-goog-api-key", gemini_api_key)
                 .send_json(payload)
                 .map_err(|e| anyhow::anyhow!("Gemini Refine Error: {}", e))?;

             if streaming_enabled {
                 let reader = BufReader::new(resp.into_reader());
                 for line in reader.lines() {
                     let line = line?;
                     if line.starts_with("data: ") {
                         let json_str = &line["data: ".len()..];
                         if json_str.trim() == "[DONE]" { break; }
                         if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                             if let Some(candidates) = chunk_resp.get("candidates").and_then(|c| c.as_array()) {
                                 if let Some(first) = candidates.first() {
                                     if let Some(parts) = first.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                         if let Some(p) = parts.first() {
                                             if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                                                 full_content.push_str(t);
                                                 on_chunk(t);
                                             }
                                         }
                                     }
                                 }
                             }
                         }
                     }
                 }
             } else {
                 let json: serde_json::Value = resp.into_json()?;
                 if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
                     if let Some(first) = candidates.first() {
                         if let Some(parts) = first.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                            full_content = parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect::<String>();
                            on_chunk(&full_content);
                         }
                     }
                 }
             }
        } else {
            if groq_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY:groq")); }
            
            let payload = serde_json::json!({
                "model": p_model,
                "messages": [{ "role": "user", "content": final_prompt }],
                "stream": streaming_enabled
            });
            
            let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
                .set("Authorization", &format!("Bearer {}", groq_api_key))
                .send_json(payload)
                .map_err(|e| anyhow::anyhow!("Groq Refine Error: {}", e))?;

            if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
                 let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
                 let usage_str = format!("{} / {}", remaining, limit);
                 if let Ok(mut app) = APP.lock() {
                     app.model_usage_stats.insert(p_model.clone(), usage_str);
                 }
            }

            if streaming_enabled {
                let reader = BufReader::new(resp.into_reader());
                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("data: ") {
                         let data = &line[6..];
                         if data == "[DONE]" { break; }
                         if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                             if let Some(content) = chunk.choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                                 full_content.push_str(content);
                                 on_chunk(content);
                             }
                         }
                    }
                }
            } else {
                 let json: ChatCompletionResponse = resp.into_json()?;
                 if let Some(choice) = json.choices.first() {
                     full_content = choice.message.content.clone();
                     on_chunk(&full_content);
                 }
            }
        }
        
        Ok(full_content)
    };

    match context {
        RefineContext::Image(img_bytes) => {
            if target_provider == "google" {
                if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY:google")); }
                let img = image::load_from_memory(&img_bytes)?.to_rgba8();
                vision_translate_image_streaming(groq_api_key, gemini_api_key, final_prompt, target_id_or_name, target_provider, img, streaming_enabled, false, on_chunk)
            } else {
                if groq_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY:groq")); }
                let img = image::load_from_memory(&img_bytes)?.to_rgba8();
                vision_translate_image_streaming(groq_api_key, gemini_api_key, final_prompt, target_id_or_name, target_provider, img, streaming_enabled, false, on_chunk)
            }
        },
        RefineContext::None => {
            exec_text_only(target_id_or_name, target_provider)
        }
    }
}
