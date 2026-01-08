use super::client::UREQ_AGENT;
use super::types::{ChatCompletionResponse, StreamChunk};
use super::vision::translate_image_streaming as vision_translate_image_streaming;
use crate::gui::locale::LocaleText;
use crate::overlay::result::RefineContext;
use crate::overlay::utils::get_context_quote;
use crate::APP;
use anyhow::Result;
use std::io::{BufRead, BufReader};

pub fn translate_text_streaming<F>(
    groq_api_key: &str,
    gemini_api_key: &str,
    text: String,
    instruction: String,
    model: String,
    provider: String,
    streaming_enabled: bool,
    use_json_format: bool,
    search_label: Option<String>,
    ui_language: &str,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let openrouter_api_key = crate::APP
        .lock()
        .ok()
        .and_then(|app| {
            let config = app.config.clone();
            if config.openrouter_api_key.is_empty() {
                None
            } else {
                Some(config.openrouter_api_key.clone())
            }
        })
        .unwrap_or_default();

    let cerebras_api_key = crate::APP
        .lock()
        .ok()
        .and_then(|app| {
            let config = app.config.clone();
            if config.cerebras_api_key.is_empty() {
                None
            } else {
                Some(config.cerebras_api_key.clone())
            }
        })
        .unwrap_or_default();

    let mut full_content = String::new();
    let prompt = format!("{}\n\n{}", instruction, text);

    if provider == "ollama" {
        // --- OLLAMA LOCAL API ---
        let (ollama_base_url, ollama_text_model) = crate::APP
            .lock()
            .ok()
            .map(|app| {
                let config = app.config.clone();
                (
                    config.ollama_base_url.clone(),
                    config.ollama_text_model.clone(),
                )
            })
            .unwrap_or_else(|| ("http://localhost:11434".to_string(), model.clone()));

        let actual_model = if ollama_text_model.is_empty() {
            model.clone()
        } else {
            ollama_text_model
        };

        return super::ollama::ollama_generate_text(
            &ollama_base_url,
            &actual_model,
            &prompt,
            streaming_enabled,
            ui_language,
            on_chunk,
        );
    } else if provider == "gemini-live" {
        // --- GEMINI LIVE API (WebSocket-based low-latency streaming) ---
        return super::gemini_live::gemini_live_generate(
            text,
            instruction,
            None, // No image for text-only
            None, // No audio for text-only
            streaming_enabled,
            ui_language,
            on_chunk,
        );
    } else if provider == "google-gtx" {
        // --- GOOGLE TRANSLATE (GTX) API ---
        // Non-LLM translation model - no API key required
        // Extract target language from the instruction (e.g., "Translate to Vietnamese" -> "Vietnamese")
        let target_lang = instruction
            .to_lowercase()
            .split("translate to ")
            .nth(1)
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "English".to_string());

        // Capitalize first letter for language lookup
        let target_lang = target_lang
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if i == 0 {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c
                }
            })
            .collect::<String>();

        match crate::api::realtime_audio::translate_with_google_gtx(&text, &target_lang) {
            Some(translated) => {
                on_chunk(&translated);
                return Ok(translated);
            }
            None => {
                return Err(anyhow::anyhow!("GTX translation failed"));
            }
        }
    } else if provider == "google" {
        // --- GEMINI TEXT API ---
        if gemini_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:gemini"));
        }

        let method = if streaming_enabled {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
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

        let mut payload = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }]
        });

        // Enable thinking for Gemini 2.5+ models (gemini-2.5-flash and gemini-robotics-er)
        // Enable thinking for Gemini 2.5+ models (gemini-2.5-flash, 3.0-flash, and gemini-robotics-er)
        let supports_thinking = (model.contains("gemini-2.5-flash") && !model.contains("lite"))
            || model.contains("gemini-3-flash-preview")
            || model.contains("gemini-robotics");
        if supports_thinking {
            payload["generationConfig"] = serde_json::json!({
                "thinkingConfig": {
                    "includeThoughts": true
                }
            });
        }

        if crate::model_config::model_supports_search_by_name(&model) {
            payload["tools"] = serde_json::json!([
                { "url_context": {} },
                { "google_search": {} }
            ]);
        }

        let resp = UREQ_AGENT
            .post(&url)
            .header("x-goog-api-key", gemini_api_key)
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
            let reader = BufReader::new(resp.into_body().into_reader());
            let mut thinking_shown = false;
            let mut content_started = false;
            let locale = LocaleText::get(ui_language);

            for line in reader.lines() {
                let line = line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))?;
                if line.starts_with("data: ") {
                    let json_str = &line["data: ".len()..];
                    if json_str.trim() == "[DONE]" {
                        break;
                    }

                    if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(candidates) =
                            chunk_resp.get("candidates").and_then(|c| c.as_array())
                        {
                            if let Some(first_candidate) = candidates.first() {
                                if let Some(parts) = first_candidate
                                    .get("content")
                                    .and_then(|c| c.get("parts"))
                                    .and_then(|p| p.as_array())
                                {
                                    for part in parts {
                                        let is_thought = part
                                            .get("thought")
                                            .and_then(|t| t.as_bool())
                                            .unwrap_or(false);

                                        if let Some(text) =
                                            part.get("text").and_then(|t| t.as_str())
                                        {
                                            if is_thought {
                                                // Model is thinking - show thinking indicator (only once)
                                                if !thinking_shown && !content_started {
                                                    on_chunk(locale.model_thinking);
                                                    thinking_shown = true;
                                                }
                                                // Consume thought, don't display
                                            } else {
                                                // Regular content
                                                if !content_started && thinking_shown {
                                                    // Wipe thinking message on first content
                                                    content_started = true;
                                                    full_content.push_str(text);
                                                    let wipe_content = format!(
                                                        "{}{}",
                                                        crate::api::WIPE_SIGNAL,
                                                        full_content
                                                    );
                                                    on_chunk(&wipe_content);
                                                } else {
                                                    content_started = true;
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
                }
            }
        } else {
            let chat_resp: serde_json::Value = resp
                .into_body()
                .read_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(candidates) = chat_resp.get("candidates").and_then(|c| c.as_array()) {
                if let Some(first_choice) = candidates.first() {
                    if let Some(parts) = first_choice
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.as_array())
                    {
                        // Filter out thought parts and collect only content
                        full_content = parts
                            .iter()
                            .filter(|p| {
                                !p.get("thought").and_then(|t| t.as_bool()).unwrap_or(false)
                            })
                            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                            .collect::<String>();
                        on_chunk(&full_content);
                    }
                }
            }
        }
    } else if provider == "cerebras" {
        // --- CEREBRAS API ---
        if cerebras_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:cerebras"));
        }

        let payload = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "stream": streaming_enabled
        });

        let resp = UREQ_AGENT
            .post("https://api.cerebras.ai/v1/chat/completions")
            .header("Authorization", &format!("Bearer {}", cerebras_api_key))
            .header("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("Cerebras API Error: {}", err_str)
                }
            })?;

        // Extract rate limit info
        // Extract rate limit info
        let remaining = resp
            .headers()
            .get("x-ratelimit-remaining-requests-day")
            .or_else(|| resp.headers().get("x-ratelimit-remaining-requests"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?");

        let mut limit = resp
            .headers()
            .get("x-ratelimit-limit-requests-day")
            .or_else(|| resp.headers().get("x-ratelimit-limit-requests"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?")
            .to_string();

        if limit == "?" {
            if let Some(conf) = crate::model_config::get_model_by_id(&model) {
                if let Some(val) = conf.quota_limit_en.split_whitespace().next() {
                    limit = val.to_string();
                }
            }
        }

        if remaining != "?" || limit != "?" {
            let usage_str = format!("{} / {}", remaining, limit);
            if let Ok(mut app) = APP.lock() {
                app.model_usage_stats.insert(model.clone(), usage_str);
            }
        }

        if streaming_enabled {
            let reader = BufReader::new(resp.into_body().into_reader());
            let mut thinking_shown = false;
            let mut content_started = false;
            let locale = LocaleText::get(ui_language);

            // Cerebras reasoning models handle thinking phase
            let is_reasoning_model = model.contains("gpt-oss") || model.contains("zai-glm");

            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            // Check for reasoning tokens (thinking phase)
                            if let Some(reasoning) = chunk
                                .choices
                                .get(0)
                                .and_then(|c| c.delta.reasoning.as_ref())
                                .filter(|s| !s.is_empty())
                            {
                                // Model is thinking - show thinking indicator (only once)
                                if !thinking_shown && !content_started {
                                    on_chunk(locale.model_thinking);
                                    thinking_shown = true;
                                }
                                let _ = reasoning; // Just consume reasoning, don't display
                            } else if is_reasoning_model && !content_started && !thinking_shown {
                                // Fallback thinking indicator for reasoning models if no reasoning field is present yet
                                on_chunk(locale.model_thinking);
                                thinking_shown = true;
                            }

                            // Check for content tokens (final result)
                            if let Some(content) = chunk
                                .choices
                                .get(0)
                                .and_then(|c| c.delta.content.as_ref())
                                .filter(|s| !s.is_empty())
                            {
                                // Content started - wipe thinking message on first content chunk
                                if !content_started && thinking_shown {
                                    content_started = true;
                                    // Use WIPE_SIGNAL to tell callback to clear accumulator
                                    full_content.push_str(content);
                                    let wipe_content =
                                        format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                                    on_chunk(&wipe_content);
                                } else {
                                    content_started = true;
                                    full_content.push_str(content);
                                    on_chunk(content);
                                }
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp
                .into_body()
                .read_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                full_content = choice.message.content.clone();
                on_chunk(&full_content);
            }
        }
    } else if provider == "openrouter" {
        // --- OPENROUTER API ---
        if openrouter_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:openrouter"));
        }

        let payload = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "stream": streaming_enabled
        });

        let resp = UREQ_AGENT
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", &format!("Bearer {}", openrouter_api_key))
            .header("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|e| {
                let err_str = e.to_string();
                if err_str.contains("401") || err_str.contains("403") {
                    anyhow::anyhow!("INVALID_API_KEY")
                } else {
                    anyhow::anyhow!("OpenRouter API Error: {}", err_str)
                }
            })?;

        if streaming_enabled {
            let reader = BufReader::new(resp.into_body().into_reader());
            let mut thinking_shown = false;
            let mut content_started = false;
            let locale = LocaleText::get(ui_language);

            for line in reader.lines() {
                let line = line?;
                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            // Check for reasoning tokens (thinking phase)
                            if let Some(reasoning) = chunk
                                .choices
                                .get(0)
                                .and_then(|c| c.delta.reasoning.as_ref())
                                .filter(|s| !s.is_empty())
                            {
                                // Model is thinking - show thinking indicator (only once)
                                if !thinking_shown && !content_started {
                                    on_chunk(locale.model_thinking);
                                    thinking_shown = true;
                                }
                                let _ = reasoning; // Just consume reasoning, don't display
                            }

                            // Check for content tokens (final result)
                            if let Some(content) = chunk
                                .choices
                                .get(0)
                                .and_then(|c| c.delta.content.as_ref())
                                .filter(|s| !s.is_empty())
                            {
                                // Content started - wipe thinking message on first content chunk
                                if !content_started && thinking_shown {
                                    content_started = true;
                                    // Use WIPE_SIGNAL to tell callback to clear accumulator
                                    full_content.push_str(content);
                                    let wipe_content =
                                        format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                                    on_chunk(&wipe_content);
                                } else {
                                    content_started = true;
                                    full_content.push_str(content);
                                    on_chunk(content);
                                }
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        } else {
            let chat_resp: ChatCompletionResponse = resp
                .into_body()
                .read_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse non-streaming response: {}", e))?;

            if let Some(choice) = chat_resp.choices.first() {
                full_content = choice.message.content.clone();
                on_chunk(&full_content);
            }
        }
    } else {
        // --- GROQ API (Default) ---
        if groq_api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("NO_API_KEY:groq"));
        }

        let is_compound = model.starts_with("groq/compound");

        if is_compound {
            // --- COMPOUND MODEL API ---
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
                "max_tokens": 8192,
                "stream": false,
                "compound_custom": {
                    "tools": {
                        "enabled_tools": ["web_search", "visit_website"]
                    }
                }
            });

            let locale = LocaleText::get(ui_language);
            let context_quote = get_context_quote(&prompt);
            let search_msg = match &search_label {
                Some(label) => format!(
                    "{}\n\n🔍 {} {}...",
                    context_quote, locale.search_doing, label
                ),
                None => format!(
                    "{}\n\n🔍 {} {}...",
                    context_quote, locale.search_doing, locale.search_searching
                ),
            };
            on_chunk(&search_msg);

            let resp = UREQ_AGENT
                .post("https://api.groq.com/openai/v1/chat/completions")
                .header("Authorization", &format!("Bearer {}", groq_api_key))
                .send_json(payload)
                .map_err(|e| {
                    let err_str = e.to_string();
                    if err_str.contains("401") {
                        anyhow::anyhow!("INVALID_API_KEY")
                    } else {
                        anyhow::anyhow!("{}", err_str)
                    }
                })?;

            if let Some(remaining) = resp
                .headers()
                .get("x-ratelimit-remaining-requests")
                .and_then(|v| v.to_str().ok())
            {
                let limit = resp
                    .headers()
                    .get("x-ratelimit-limit-requests")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("?");
                let usage_str = format!("{} / {}", remaining, limit);

                if let Ok(mut app) = APP.lock() {
                    app.model_usage_stats.insert(model.clone(), usage_str);
                }
            }

            let json: serde_json::Value = resp
                .into_body()
                .read_json()
                .map_err(|e| anyhow::anyhow!("Failed to parse compound response: {}", e))?;

            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                if let Some(first_choice) = choices.first() {
                    if let Some(message) = first_choice.get("message") {
                        if let Some(executed_tools) =
                            message.get("executed_tools").and_then(|t| t.as_array())
                        {
                            let mut search_queries = Vec::new();
                            for tool in executed_tools {
                                if let Some(tool_type) = tool.get("type").and_then(|t| t.as_str()) {
                                    if tool_type == "search" {
                                        if let Some(args) =
                                            tool.get("arguments").and_then(|a| a.as_str())
                                        {
                                            if let Ok(args_json) =
                                                serde_json::from_str::<serde_json::Value>(args)
                                            {
                                                if let Some(query) =
                                                    args_json.get("query").and_then(|q| q.as_str())
                                                {
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
                                    Some(label) => format!(
                                        "{}\n\n🔍 {} {}...\n\n",
                                        context_quote,
                                        locale.search_doing.to_uppercase(),
                                        label.to_uppercase()
                                    ),
                                    None => format!(
                                        "{}\n\n🔍 {} {}...\n\n",
                                        context_quote,
                                        locale.search_doing.to_uppercase(),
                                        locale.search_searching.to_uppercase()
                                    ),
                                };
                                let mut phase1 = phase1_header;
                                phase1.push_str(&format!("{}\n", locale.search_query_label));
                                for (i, query) in search_queries.iter().enumerate() {
                                    phase1.push_str(&format!("  {}. \"{}\"\n", i + 1, query));
                                }
                                on_chunk(&phase1);
                                std::thread::sleep(std::time::Duration::from_millis(800));
                            }

                            let mut all_sources = Vec::new();
                            for tool in executed_tools {
                                if let Some(search_results) = tool
                                    .get("search_results")
                                    .and_then(|s| s.get("results"))
                                    .and_then(|r| r.as_array())
                                {
                                    for result in search_results {
                                        let title = result
                                            .get("title")
                                            .and_then(|t| t.as_str())
                                            .unwrap_or(locale.search_no_title);
                                        let url = result
                                            .get("url")
                                            .and_then(|u| u.as_str())
                                            .unwrap_or("");
                                        let score = result
                                            .get("score")
                                            .and_then(|s| s.as_f64())
                                            .unwrap_or(0.0);
                                        let content = result
                                            .get("content")
                                            .and_then(|c| c.as_str())
                                            .unwrap_or("");

                                        all_sources.push((
                                            title.to_string(),
                                            url.to_string(),
                                            score,
                                            content.to_string(),
                                        ));
                                    }
                                }
                            }

                            if !all_sources.is_empty() {
                                all_sources.sort_by(|a, b| {
                                    b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
                                });

                                let context_quote = get_context_quote(&prompt);
                                let mut phase2 = format!(
                                    "{}\n\n{}\n\n",
                                    context_quote,
                                    locale
                                        .search_found_sources
                                        .replace("{}", &all_sources.len().to_string())
                                );
                                phase2.push_str(&format!("{}\n\n", locale.search_sources_label));

                                for (i, (title, url, score, content)) in
                                    all_sources.iter().take(6).enumerate()
                                {
                                    let title_display = if title.chars().count() > 60 {
                                        format!("{}...", title.chars().take(57).collect::<String>())
                                    } else {
                                        title.clone()
                                    };

                                    let domain = url.split('/').nth(2).unwrap_or(url);
                                    let score_pct = (score * 100.0) as i32;

                                    phase2.push_str(&format!(
                                        "{}. {} [{}%]\n",
                                        i + 1,
                                        title_display,
                                        score_pct
                                    ));
                                    phase2.push_str(&format!("   🔗 {}\n", domain));

                                    if !content.is_empty() {
                                        let preview = if content.len() > 100 {
                                            format!(
                                                "{}...",
                                                content
                                                    .chars()
                                                    .take(100)
                                                    .collect::<String>()
                                                    .replace('\n', " ")
                                            )
                                        } else {
                                            content.replace('\n', " ")
                                        };
                                        phase2.push_str(&format!("   📄 {}\n", preview));
                                    }
                                    phase2.push('\n');
                                }

                                on_chunk(&phase2);
                                std::thread::sleep(std::time::Duration::from_millis(1200));

                                let context_quote = get_context_quote(&prompt);
                                let phase3 = format!(
                                    "{}\n\n{}\n\n{}\n{}\n",
                                    context_quote,
                                    locale.search_synthesizing,
                                    locale
                                        .search_analyzed_sources
                                        .replace("{}", &all_sources.len().min(6).to_string()),
                                    locale.search_processing
                                );
                                on_chunk(&phase3);
                                std::thread::sleep(std::time::Duration::from_millis(600));
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

            let resp = UREQ_AGENT
                .post("https://api.groq.com/openai/v1/chat/completions")
                .header("Authorization", &format!("Bearer {}", groq_api_key))
                .send_json(payload)
                .map_err(|e| {
                    let err_str = e.to_string();
                    if err_str.contains("401") {
                        anyhow::anyhow!("INVALID_API_KEY")
                    } else {
                        anyhow::anyhow!("{}", err_str)
                    }
                })?;

            if let Some(remaining) = resp
                .headers()
                .get("x-ratelimit-remaining-requests")
                .and_then(|v| v.to_str().ok())
            {
                let limit = resp
                    .headers()
                    .get("x-ratelimit-limit-requests")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("?");
                let usage_str = format!("{} / {}", remaining, limit);

                if let Ok(mut app) = APP.lock() {
                    app.model_usage_stats.insert(model.clone(), usage_str);
                }
            }

            if streaming_enabled {
                let reader = BufReader::new(resp.into_body().into_reader());

                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" {
                            break;
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                if let Some(content) =
                                    chunk.choices.get(0).and_then(|c| c.delta.content.as_ref())
                                {
                                    full_content.push_str(content);
                                    on_chunk(content);
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
            } else {
                let chat_resp: ChatCompletionResponse =
                    resp.into_body().read_json().map_err(|e| {
                        anyhow::anyhow!("Failed to parse non-streaming response: {}", e)
                    })?;

                if let Some(choice) = chat_resp.choices.first() {
                    let content_str = &choice.message.content;

                    if use_json_format {
                        if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(content_str)
                        {
                            if let Some(translation) =
                                json_obj.get("translation").and_then(|v| v.as_str())
                            {
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
    ui_language: &str,
    mut on_chunk: F,
) -> Result<String>
where
    F: FnMut(&str),
{
    let openrouter_api_key = crate::APP
        .lock()
        .ok()
        .and_then(|app| {
            let config = app.config.clone();
            if config.openrouter_api_key.is_empty() {
                None
            } else {
                Some(config.openrouter_api_key.clone())
            }
        })
        .unwrap_or_default();

    let cerebras_api_key = crate::APP
        .lock()
        .ok()
        .and_then(|app| {
            let config = app.config.clone();
            if config.cerebras_api_key.is_empty() {
                None
            } else {
                Some(config.cerebras_api_key.clone())
            }
        })
        .unwrap_or_default();

    let final_prompt = format!(
        "Content:\n{}\n\nInstruction:\n{}\n\nOutput ONLY the result.",
        previous_text, user_prompt
    );

    let (mut target_id_or_name, mut target_provider) = match context {
        RefineContext::Image(_) => (original_model_id.to_string(), original_provider.to_string()),
        _ => {
            if !original_model_id.trim().is_empty() && original_model_id != "scout" {
                (original_model_id.to_string(), original_provider.to_string())
            } else {
                if !gemini_api_key.trim().is_empty() {
                    ("gemini-flash-lite".to_string(), "google".to_string())
                } else if !cerebras_api_key.trim().is_empty() {
                    (
                        "qwen-3-235b-a22b-instruct-2507".to_string(),
                        "cerebras".to_string(),
                    )
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
            if gemini_api_key.trim().is_empty() {
                return Err(anyhow::anyhow!("NO_API_KEY:gemini"));
            }

            let method = if streaming_enabled {
                "streamGenerateContent"
            } else {
                "generateContent"
            };
            let url = if streaming_enabled {
                format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?alt=sse",
                    p_model, method
                )
            } else {
                format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                    p_model, method
                )
            };

            let mut payload = serde_json::json!({
                "contents": [{ "role": "user", "parts": [{ "text": final_prompt }] }]
            });

            // Enable thinking for Gemini 2.5+ models
            let supports_thinking = (p_model.contains("gemini-2.5-flash")
                && !p_model.contains("lite"))
                || p_model.contains("gemini-3-flash-preview")
                || p_model.contains("gemini-robotics");
            if supports_thinking {
                payload["generationConfig"] = serde_json::json!({
                    "thinkingConfig": {
                        "includeThoughts": true
                    }
                });
            }

            if crate::model_config::model_supports_search_by_name(&p_model) {
                payload["tools"] = serde_json::json!([
                    { "url_context": {} },
                    { "google_search": {} }
                ]);
            }

            let resp = UREQ_AGENT
                .post(&url)
                .header("x-goog-api-key", gemini_api_key)
                .send_json(payload)
                .map_err(|e| anyhow::anyhow!("Gemini Refine Error: {}", e))?;

            if streaming_enabled {
                let reader = BufReader::new(resp.into_body().into_reader());
                let mut thinking_shown = false;
                let mut content_started = false;
                let locale = LocaleText::get(ui_language);

                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("data: ") {
                        let json_str = &line["data: ".len()..];
                        if json_str.trim() == "[DONE]" {
                            break;
                        }
                        if let Ok(chunk_resp) = serde_json::from_str::<serde_json::Value>(json_str)
                        {
                            if let Some(candidates) =
                                chunk_resp.get("candidates").and_then(|c| c.as_array())
                            {
                                if let Some(first) = candidates.first() {
                                    if let Some(parts) = first
                                        .get("content")
                                        .and_then(|c| c.get("parts"))
                                        .and_then(|p| p.as_array())
                                    {
                                        for part in parts {
                                            let is_thought = part
                                                .get("thought")
                                                .and_then(|t| t.as_bool())
                                                .unwrap_or(false);

                                            if let Some(t) =
                                                part.get("text").and_then(|v| v.as_str())
                                            {
                                                if is_thought {
                                                    if !thinking_shown && !content_started {
                                                        on_chunk(locale.model_thinking);
                                                        thinking_shown = true;
                                                    }
                                                } else {
                                                    if !content_started && thinking_shown {
                                                        content_started = true;
                                                        full_content.push_str(t);
                                                        let wipe_content = format!(
                                                            "{}{}",
                                                            crate::api::WIPE_SIGNAL,
                                                            full_content
                                                        );
                                                        on_chunk(&wipe_content);
                                                    } else {
                                                        content_started = true;
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
                    }
                }
            } else {
                let json: serde_json::Value = resp.into_body().read_json()?;
                if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
                    if let Some(first) = candidates.first() {
                        if let Some(parts) = first
                            .get("content")
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.as_array())
                        {
                            // Filter out thought parts
                            full_content = parts
                                .iter()
                                .filter(|p| {
                                    !p.get("thought").and_then(|t| t.as_bool()).unwrap_or(false)
                                })
                                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                .collect::<String>();
                            on_chunk(&full_content);
                        }
                    }
                }
            }
        } else if p_provider == "gemini-live" {
            // --- GEMINI LIVE REFINE ---
            return super::gemini_live::gemini_live_generate(
                final_prompt.clone(),
                String::new(), // instruction part of prompt for now
                None,
                None,
                streaming_enabled,
                ui_language,
                &mut on_chunk,
            );
        } else if p_provider == "cerebras" {
            if cerebras_api_key.trim().is_empty() {
                return Err(anyhow::anyhow!("NO_API_KEY:cerebras"));
            }

            let payload = serde_json::json!({
                "model": p_model,
                "messages": [
                    { "role": "user", "content": final_prompt }
                ],
                "stream": streaming_enabled
            });

            let resp = UREQ_AGENT
                .post("https://api.cerebras.ai/v1/chat/completions")
                .header("Authorization", &format!("Bearer {}", cerebras_api_key))
                .header("Content-Type", "application/json")
                .send_json(payload)
                .map_err(|e| anyhow::anyhow!("Cerebras Refine Error: {}", e))?;

            // Extract rate limit info
            // Extract rate limit info
            let remaining = resp
                .headers()
                .get("x-ratelimit-remaining-requests-day")
                .or_else(|| resp.headers().get("x-ratelimit-remaining-requests"))
                .and_then(|v| v.to_str().ok())
                .unwrap_or("?");

            let mut limit = resp
                .headers()
                .get("x-ratelimit-limit-requests-day")
                .or_else(|| resp.headers().get("x-ratelimit-limit-requests"))
                .and_then(|v| v.to_str().ok())
                .unwrap_or("?")
                .to_string();

            if limit == "?" {
                if let Some(conf) = crate::model_config::get_model_by_id(&p_model) {
                    if let Some(val) = conf.quota_limit_en.split_whitespace().next() {
                        limit = val.to_string();
                    }
                }
            }

            if remaining != "?" || limit != "?" {
                let usage_str = format!("{} / {}", remaining, limit);
                if let Ok(mut app) = APP.lock() {
                    app.model_usage_stats.insert(p_model.clone(), usage_str);
                }
            }

            if streaming_enabled {
                let reader = BufReader::new(resp.into_body().into_reader());
                let mut thinking_shown = false;
                let mut content_started = false;
                let locale = LocaleText::get(ui_language);

                let is_reasoning_model = p_model.contains("gpt-oss") || p_model.contains("zai-glm");

                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" {
                            break;
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                // Check for reasoning tokens (thinking phase)
                                if let Some(reasoning) = chunk
                                    .choices
                                    .get(0)
                                    .and_then(|c| c.delta.reasoning.as_ref())
                                    .filter(|s| !s.is_empty())
                                {
                                    if !thinking_shown && !content_started {
                                        on_chunk(locale.model_thinking);
                                        thinking_shown = true;
                                    }
                                    let _ = reasoning;
                                } else if is_reasoning_model && !content_started && !thinking_shown
                                {
                                    on_chunk(locale.model_thinking);
                                    thinking_shown = true;
                                }

                                // Check for content tokens (final result)
                                if let Some(content) = chunk
                                    .choices
                                    .get(0)
                                    .and_then(|c| c.delta.content.as_ref())
                                    .filter(|s| !s.is_empty())
                                {
                                    if !content_started && thinking_shown {
                                        content_started = true;
                                        full_content.push_str(content);
                                        let wipe_content =
                                            format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                                        on_chunk(&wipe_content);
                                    } else {
                                        content_started = true;
                                        full_content.push_str(content);
                                        on_chunk(content);
                                    }
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
            } else {
                let json: ChatCompletionResponse = resp.into_body().read_json()?;
                if let Some(choice) = json.choices.first() {
                    full_content = choice.message.content.clone();
                    on_chunk(&full_content);
                }
            }
        } else if p_provider == "openrouter" {
            if openrouter_api_key.trim().is_empty() {
                return Err(anyhow::anyhow!("NO_API_KEY:openrouter"));
            }

            let payload = serde_json::json!({
                "model": p_model,
                "messages": [
                    { "role": "user", "content": final_prompt }
                ],
                "stream": streaming_enabled
            });

            let resp = UREQ_AGENT
                .post("https://openrouter.ai/api/v1/chat/completions")
                .header("Authorization", &format!("Bearer {}", openrouter_api_key))
                .header("Content-Type", "application/json")
                .send_json(payload)
                .map_err(|e| anyhow::anyhow!("OpenRouter Refine Error: {}", e))?;

            if streaming_enabled {
                let reader = BufReader::new(resp.into_body().into_reader());
                let mut thinking_shown = false;
                let mut content_started = false;
                let locale = LocaleText::get(ui_language);

                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" {
                            break;
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                // Check for reasoning tokens (thinking phase)
                                if let Some(reasoning) = chunk
                                    .choices
                                    .get(0)
                                    .and_then(|c| c.delta.reasoning.as_ref())
                                    .filter(|s| !s.is_empty())
                                {
                                    if !thinking_shown && !content_started {
                                        on_chunk(locale.model_thinking);
                                        thinking_shown = true;
                                    }
                                    let _ = reasoning;
                                }

                                // Check for content tokens (final result)
                                if let Some(content) = chunk
                                    .choices
                                    .get(0)
                                    .and_then(|c| c.delta.content.as_ref())
                                    .filter(|s| !s.is_empty())
                                {
                                    if !content_started && thinking_shown {
                                        content_started = true;
                                        full_content.push_str(content);
                                        let wipe_content =
                                            format!("{}{}", crate::api::WIPE_SIGNAL, full_content);
                                        on_chunk(&wipe_content);
                                    } else {
                                        content_started = true;
                                        full_content.push_str(content);
                                        on_chunk(content);
                                    }
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
            } else {
                let json: ChatCompletionResponse = resp.into_body().read_json()?;
                if let Some(choice) = json.choices.first() {
                    full_content = choice.message.content.clone();
                    on_chunk(&full_content);
                }
            }
        } else {
            if groq_api_key.trim().is_empty() {
                return Err(anyhow::anyhow!("NO_API_KEY:groq"));
            }

            let is_compound = p_model.starts_with("groq/compound");

            if is_compound {
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
                on_chunk(&format!(
                    "{}\n\n🔍 {} {}...",
                    context_quote, locale.search_doing, locale.search_searching
                ));

                let resp = UREQ_AGENT
                    .post("https://api.groq.com/openai/v1/chat/completions")
                    .header("Authorization", &format!("Bearer {}", groq_api_key))
                    .send_json(payload)
                    .map_err(|e| anyhow::anyhow!("Groq Compound Refine Error: {}", e))?;

                if let Some(remaining) = resp
                    .headers()
                    .get("x-ratelimit-remaining-requests")
                    .and_then(|v| v.to_str().ok())
                {
                    let limit = resp
                        .headers()
                        .get("x-ratelimit-limit-requests")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("?");
                    let usage_str = format!("{} / {}", remaining, limit);
                    if let Ok(mut app) = APP.lock() {
                        app.model_usage_stats.insert(p_model.clone(), usage_str);
                    }
                }

                let json: serde_json::Value = resp.into_body().read_json()?;

                if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                    if let Some(first_choice) = choices.first() {
                        if let Some(message) = first_choice.get("message") {
                            if let Some(executed_tools) =
                                message.get("executed_tools").and_then(|t| t.as_array())
                            {
                                let mut search_queries = Vec::new();
                                for tool in executed_tools {
                                    if tool.get("type").and_then(|t| t.as_str()) == Some("search") {
                                        if let Some(args) =
                                            tool.get("arguments").and_then(|a| a.as_str())
                                        {
                                            if let Ok(args_json) =
                                                serde_json::from_str::<serde_json::Value>(args)
                                            {
                                                if let Some(query) =
                                                    args_json.get("query").and_then(|q| q.as_str())
                                                {
                                                    search_queries.push(query.to_string());
                                                }
                                            }
                                        }
                                    }
                                }

                                if !search_queries.is_empty() {
                                    let context_quote = get_context_quote(&final_prompt);
                                    let mut phase1 = format!(
                                        "{}\n\n🔍 {} {}...\n\n{}\n",
                                        context_quote,
                                        locale.search_doing.to_uppercase(),
                                        locale.search_searching.to_uppercase(),
                                        locale.search_query_label
                                    );
                                    for (i, q) in search_queries.iter().enumerate() {
                                        phase1.push_str(&format!("  {}. \"{}\"\n", i + 1, q));
                                    }
                                    on_chunk(&phase1);
                                    std::thread::sleep(std::time::Duration::from_millis(600));
                                }

                                let mut all_sources = Vec::new();
                                for tool in executed_tools {
                                    if let Some(results) = tool
                                        .get("search_results")
                                        .and_then(|s| s.get("results"))
                                        .and_then(|r| r.as_array())
                                    {
                                        for r in results {
                                            let title = r
                                                .get("title")
                                                .and_then(|t| t.as_str())
                                                .unwrap_or(locale.search_no_title);
                                            let url =
                                                r.get("url").and_then(|u| u.as_str()).unwrap_or("");
                                            let score = r
                                                .get("score")
                                                .and_then(|s| s.as_f64())
                                                .unwrap_or(0.0);
                                            all_sources.push((
                                                title.to_string(),
                                                url.to_string(),
                                                score,
                                            ));
                                        }
                                    }
                                }

                                if !all_sources.is_empty() {
                                    all_sources.sort_by(|a, b| {
                                        b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
                                    });
                                    let context_quote = get_context_quote(&final_prompt);
                                    let mut phase2 = format!(
                                        "{}\n\n{}\n\n",
                                        context_quote,
                                        locale
                                            .search_found_sources
                                            .replace("{}", &all_sources.len().to_string())
                                    );
                                    for (i, (title, url, score)) in
                                        all_sources.iter().take(5).enumerate()
                                    {
                                        let t = if title.chars().count() > 50 {
                                            format!(
                                                "{}...",
                                                title.chars().take(47).collect::<String>()
                                            )
                                        } else {
                                            title.clone()
                                        };
                                        let domain = url.split('/').nth(2).unwrap_or("");
                                        phase2.push_str(&format!(
                                            "{}. {} [{}%]\n   🔗 {}\n",
                                            i + 1,
                                            t,
                                            (score * 100.0) as i32,
                                            domain
                                        ));
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
                let payload = serde_json::json!({
                    "model": p_model,
                    "messages": [{ "role": "user", "content": final_prompt }],
                    "stream": streaming_enabled
                });

                let resp = UREQ_AGENT
                    .post("https://api.groq.com/openai/v1/chat/completions")
                    .header("Authorization", &format!("Bearer {}", groq_api_key))
                    .send_json(payload)
                    .map_err(|e| anyhow::anyhow!("Groq Refine Error: {}", e))?;

                if let Some(remaining) = resp
                    .headers()
                    .get("x-ratelimit-remaining-requests")
                    .and_then(|v| v.to_str().ok())
                {
                    let limit = resp
                        .headers()
                        .get("x-ratelimit-limit-requests")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("?");
                    let usage_str = format!("{} / {}", remaining, limit);
                    if let Ok(mut app) = APP.lock() {
                        app.model_usage_stats.insert(p_model.clone(), usage_str);
                    }
                }

                if streaming_enabled {
                    let reader = BufReader::new(resp.into_body().into_reader());
                    for line in reader.lines() {
                        let line = line?;
                        if line.starts_with("data: ") {
                            let data = &line[6..];
                            if data == "[DONE]" {
                                break;
                            }
                            if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                                if let Some(content) =
                                    chunk.choices.get(0).and_then(|c| c.delta.content.as_ref())
                                {
                                    full_content.push_str(content);
                                    on_chunk(content);
                                }
                            }
                        }
                    }
                } else {
                    let json: ChatCompletionResponse = resp.into_body().read_json()?;
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
                if gemini_api_key.trim().is_empty() {
                    return Err(anyhow::anyhow!("NO_API_KEY:gemini"));
                }
                let img = image::load_from_memory(&img_bytes)?.to_rgba8();
                vision_translate_image_streaming(
                    groq_api_key,
                    gemini_api_key,
                    final_prompt,
                    target_id_or_name,
                    target_provider,
                    img,
                    Some(img_bytes.clone()),
                    streaming_enabled,
                    false,
                    on_chunk,
                )
            } else if target_provider == "gemini-live" {
                // Determine mime type (default to jpeg as per common usage)
                let mime = "image/jpeg".to_string();
                super::gemini_live::gemini_live_generate(
                    final_prompt,
                    String::new(),
                    Some((img_bytes.clone(), mime)),
                    None,
                    streaming_enabled,
                    ui_language,
                    &mut on_chunk,
                )
            } else {
                if groq_api_key.trim().is_empty() {
                    return Err(anyhow::anyhow!("NO_API_KEY:groq"));
                }
                let img = image::load_from_memory(&img_bytes)?.to_rgba8();
                vision_translate_image_streaming(
                    groq_api_key,
                    gemini_api_key,
                    final_prompt,
                    target_id_or_name,
                    target_provider,
                    img,
                    Some(img_bytes.clone()),
                    streaming_enabled,
                    false,
                    on_chunk,
                )
            }
        }
        RefineContext::Audio(_) => {
            // Audio refinement uses text-only processing (transcription already done)
            exec_text_only(target_id_or_name, target_provider)
        }
        RefineContext::None => exec_text_only(target_id_or_name, target_provider),
    }
}
