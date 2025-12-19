use anyhow::Result;
use std::io::{BufRead, BufReader};
use crate::APP;
use crate::overlay::result::RefineContext;
use crate::gui::locale::LocaleText;
use super::client::UREQ_AGENT;
use super::types::{StreamChunk, ChatCompletionResponse};
use super::vision::translate_image_streaming as vision_translate_image_streaming;

fn get_context_quote(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let len = words.len();
    if len > 50 {
        format!("\"... {}\"", words[len - 50..].join(" "))
    } else {
        format!("\"... {}\"", words.join(" "))
    }
}


pub fn translate_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    text: String,
    instruction: String, // CHANGED: Renamed from target_lang to instruction
    model: String,
    provider: String,
    streaming_enabled: bool,
    use_json_format: bool,
    search_label: Option<String>, // Localized preset name for compound model search UI
    ui_language: &str, // Language code for localized search UI strings
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let mut full_content = String::new();
    // CHANGED: Use the instruction directly instead of hardcoded template
    let prompt = format!("{}\n\n{}", instruction, text);

    if provider == "google" {
        // --- GEMINI TEXT API ---
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:[REDACTED:api-key]"));
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

        // Check if this is a compound model (agentic with web search)
        let is_compound = model.starts_with("groq/compound");
        
        if is_compound {
            // --- COMPOUND MODEL API ---
            // Compound models are non-streaming agentic models that do web search
            let payload = serde_json::json!({
                "model": model,
                "messages": [
                    { 
                        "role": "system", 
                        "content": "IMPORTANT: Limit yourself to a maximum of 3 tool calls total. Make 1-2 focused searches, then answer. Do not visit websites unless absolutely necessary. Be efficient." 
                    },
                    { "role": "user", "content": prompt }
                ],
                "temperature": 1,
                "max_completion_tokens": 8192,
                "stream": false,
                "compound_custom": {
                    "tools": {
                        "enabled_tools": ["web_search", "visit_website"]
                    }
                }
            });

            // Show initial searching state with localized preset name
            let locale = LocaleText::get(ui_language);
            let context_quote = get_context_quote(&prompt);
            let search_msg = match &search_label {
                Some(label) => format!("{}\n\n🔍 {} {}...", context_quote, locale.search_doing, label),
                None => format!("{}\n\n🔍 {} {}...", context_quote, locale.search_doing, locale.search_searching),
            };
            on_chunk(&search_msg);
            
            let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
                .set("Authorization", &format!("Bearer {}", groq_api_key))
                .timeout(std::time::Duration::from_secs(60)) // Compound can take longer
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

            // Parse the compound response
            let json: serde_json::Value = resp.into_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse compound response: {}", e))?;

            // Show search phase: Display search queries and sources
            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                if let Some(first_choice) = choices.first() {
                    if let Some(message) = first_choice.get("message") {
                        
                        // PHASE 1: Extract and show search queries from executed_tools arguments
                        if let Some(executed_tools) = message.get("executed_tools").and_then(|t| t.as_array()) {
                            let mut search_queries = Vec::new();
                            for tool in executed_tools {
                                if let Some(tool_type) = tool.get("type").and_then(|t| t.as_str()) {
                                    if tool_type == "search" {
                                        if let Some(args) = tool.get("arguments").and_then(|a| a.as_str()) {
                                            // Parse JSON arguments to get query
                                            if let Ok(args_json) = serde_json::from_str::<serde_json::Value>(args) {
                                                if let Some(query) = args_json.get("query").and_then(|q| q.as_str()) {
                                                    search_queries.push(query.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            
                            let context_quote = get_context_quote(&prompt);
                            if !search_queries.is_empty() {
                                let phase1_header = match &search_label {
                                    Some(label) => format!("{}\n\n🔍 {} {}...\n\n", context_quote, locale.search_doing.to_uppercase(), label.to_uppercase()),
                                    None => format!("{}\n\n🔍 {} {}...\n\n", context_quote, locale.search_doing.to_uppercase(), locale.search_searching.to_uppercase()),
                                };
                                let mut phase1 = phase1_header;
                                phase1.push_str(&format!("{}\n", locale.search_query_label));
                                for (i, query) in search_queries.iter().enumerate() {
                                    phase1.push_str(&format!("  {}. \"{}\"\n", i + 1, query));
                                }
                                on_chunk(&phase1);
                                std::thread::sleep(std::time::Duration::from_millis(800));
                            }
                            
                            // PHASE 2: Show sources found with details
                            let mut all_sources = Vec::new();
                            for tool in executed_tools {
                                if let Some(search_results) = tool.get("search_results")
                                    .and_then(|s| s.get("results"))
                                    .and_then(|r| r.as_array()) 
                                {
                                    for result in search_results {
                                        let title = result.get("title").and_then(|t| t.as_str()).unwrap_or(locale.search_no_title);
                                        let url = result.get("url").and_then(|u| u.as_str()).unwrap_or("");
                                        let score = result.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);
                                        let content = result.get("content").and_then(|c| c.as_str()).unwrap_or("");
                                        
                                        all_sources.push((title.to_string(), url.to_string(), score, content.to_string()));
                                    }
                                }
                            }
                            
                            if !all_sources.is_empty() {
                                // Sort by score descending
                                all_sources.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
                                
                                let context_quote = get_context_quote(&prompt);
                                let mut phase2 = format!("{}\n\n{}\n\n", context_quote, locale.search_found_sources.replace("{}", &all_sources.len().to_string()));
                                phase2.push_str(&format!("{}\n\n", locale.search_sources_label));
                                
                                for (i, (title, url, score, content)) in all_sources.iter().take(6).enumerate() {
                                    // Truncate title if too long (character-aware for UTF-8)
                                    let title_display = if title.chars().count() > 60 {
                                        format!("{}...", title.chars().take(57).collect::<String>())
                                    } else {
                                        title.clone()
                                    };
                                    
                                    // Extract domain from URL
                                    let domain = url.split('/').nth(2).unwrap_or(url);
                                    
                                    // Show score as percentage
                                    let score_pct = (score * 100.0) as i32;
                                    
                                    phase2.push_str(&format!("{}. {} [{}%]\n", i + 1, title_display, score_pct));
                                    phase2.push_str(&format!("   🔗 {}\n", domain));
                                    
                                    // Show content preview (first 100 chars)
                                    if !content.is_empty() {
                                        let preview = if content.len() > 100 {
                                            format!("{}...", content.chars().take(100).collect::<String>().replace('\n', " "))
                                        } else {
                                            content.replace('\n', " ")
                                        };
                                        phase2.push_str(&format!("   📄 {}\n", preview));
                                    }
                                    phase2.push('\n');
                                }
                                
                                on_chunk(&phase2);
                                std::thread::sleep(std::time::Duration::from_millis(1200));
                                
                                // PHASE 3: Synthesizing message
                                let context_quote = get_context_quote(&prompt);
                                let phase3 = format!(
                                    "{}\n\n{}\n\n{}\n{}\n",
                                    context_quote,
                                    locale.search_synthesizing,
                                    locale.search_analyzed_sources.replace("{}", &all_sources.len().min(6).to_string()),
                                    locale.search_processing
                                );
                                on_chunk(&phase3);
                                std::thread::sleep(std::time::Duration::from_millis(600));
                            }
                        }
                        
                        // FINAL PHASE: Show the actual content (final answer)
                        if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                            full_content = content.to_string();
                            on_chunk(&full_content);
                        }
                    }
                }
            }
            
        } else {
            // --- STANDARD GROQ API ---
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
    ui_language: &str, // Language code for localized search UI strings
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
             if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY:[REDACTED:api-key]")); }
             
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
            
            // Check if this is a compound model
            let is_compound = p_model.starts_with("groq/compound");
            
            if is_compound {
                // Compound model handling
                let payload = serde_json::json!({
                    "model": p_model,
                    "messages": [
                        { 
                            "role": "system", 
                            "content": "IMPORTANT: Limit yourself to a maximum of 3 tool calls total. Make 1-2 focused searches, then answer. Do not visit websites unless absolutely necessary. Be efficient." 
                        },
                        { "role": "user", "content": final_prompt }
                    ],
                    "temperature": 1,
                    "max_completion_tokens": 8192,
                    "stream": false,
                    "compound_custom": {
                        "tools": {
                            "enabled_tools": ["web_search", "visit_website"]
                        }
                    }
                });
                
                let locale = LocaleText::get(ui_language);
                let context_quote = get_context_quote(&final_prompt);
                on_chunk(&format!("{}\n\n🔍 {} {}...", context_quote, locale.search_doing, locale.search_searching));
                
                let resp = UREQ_AGENT.post("https://api.groq.com/openai/v1/chat/completions")
                    .set("Authorization", &format!("Bearer {}", groq_api_key))
                    .timeout(std::time::Duration::from_secs(60))
                    .send_json(payload)
                    .map_err(|e| anyhow::anyhow!("Groq Compound Refine Error: {}", e))?;
                
                if let Some(remaining) = resp.header("x-ratelimit-remaining-requests") {
                    let limit = resp.header("x-ratelimit-limit-requests").unwrap_or("?");
                    let usage_str = format!("{} / {}", remaining, limit);
                    if let Ok(mut app) = APP.lock() {
                        app.model_usage_stats.insert(p_model.clone(), usage_str);
                    }
                }
                
                let json: serde_json::Value = resp.into_json()?;
                
                if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                    if let Some(first_choice) = choices.first() {
                        if let Some(message) = first_choice.get("message") {
                            // Show sources if available
                            if let Some(executed_tools) = message.get("executed_tools").and_then(|t| t.as_array()) {
                                // Extract search queries
                                let mut search_queries = Vec::new();
                                for tool in executed_tools {
                                    if tool.get("type").and_then(|t| t.as_str()) == Some("search") {
                                        if let Some(args) = tool.get("arguments").and_then(|a| a.as_str()) {
                                            if let Ok(args_json) = serde_json::from_str::<serde_json::Value>(args) {
                                                if let Some(query) = args_json.get("query").and_then(|q| q.as_str()) {
                                                    search_queries.push(query.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                if !search_queries.is_empty() {
                                    let context_quote = get_context_quote(&final_prompt);
                                    let mut phase1 = format!("{}\n\n🔍 {} {}...\n\n{}\n", 
                                        context_quote,
                                        locale.search_doing.to_uppercase(), 
                                        locale.search_searching.to_uppercase(),
                                        locale.search_query_label);
                                    for (i, q) in search_queries.iter().enumerate() {
                                        phase1.push_str(&format!("  {}. \"{}\"\n", i + 1, q));
                                    }
                                    on_chunk(&phase1);
                                    std::thread::sleep(std::time::Duration::from_millis(600));
                                }
                                
                                // Collect sources
                                let mut all_sources = Vec::new();
                                for tool in executed_tools {
                                    if let Some(results) = tool.get("search_results").and_then(|s| s.get("results")).and_then(|r| r.as_array()) {
                                        for r in results {
                                            let title = r.get("title").and_then(|t| t.as_str()).unwrap_or(locale.search_no_title);
                                            let url = r.get("url").and_then(|u| u.as_str()).unwrap_or("");
                                            let score = r.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);
                                            all_sources.push((title.to_string(), url.to_string(), score));
                                        }
                                    }
                                }
                                
                                if !all_sources.is_empty() {
                                    all_sources.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
                                    let context_quote = get_context_quote(&final_prompt);
                                    let mut phase2 = format!("{}\n\n{}\n\n", context_quote, locale.search_found_sources.replace("{}", &all_sources.len().to_string()));
                                    for (i, (title, url, score)) in all_sources.iter().take(5).enumerate() {
                                        let t = if title.chars().count() > 50 { format!("{}...", title.chars().take(47).collect::<String>()) } else { title.clone() };
                                        let domain = url.split('/').nth(2).unwrap_or("");
                                        phase2.push_str(&format!("{}. {} [{}%]\n   🔗 {}\n", i + 1, t, (score * 100.0) as i32, domain));
                                    }
                                    phase2.push_str(&format!("\n{}", locale.search_synthesizing));
                                    on_chunk(&phase2);
                                    std::thread::sleep(std::time::Duration::from_millis(800));
                                }
                            }
                            
                            if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                                full_content = content.to_string();
                                on_chunk(&full_content);
                            }
                        }
                    }
                }
            } else {
                // Standard Groq model
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
        }
        
        Ok(full_content)
    };

    match context {
        RefineContext::Image(img_bytes) => {
            if target_provider == "google" {
                if gemini_api_key.trim().is_empty() { return Err(anyhow::anyhow!("NO_API_KEY:[REDACTED:api-key]")); }
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
