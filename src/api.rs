use anyhow::Result;
use serde::{Deserialize, Serialize};
use image::{ImageBuffer, Rgba, ImageFormat};
use base64::{Engine as _, engine::general_purpose};
use std::io::Cursor;

#[derive(Serialize, Deserialize)]
struct GroqResponse {
    translation: String,
}

pub fn translate_image(
    api_key: String,
    target_lang: String,
    model: String,
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> Result<String> {
    let mut png_data = Vec::new();
    image.write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)?;
    let b64_image = general_purpose::STANDARD.encode(&png_data);

    let prompt = format!(
        "Extract text from this image and translate it to {}. \
        You must output valid JSON containing ONLY the key 'translation'. \
        Example: {{ \"translation\": \"Hello world\" }}",
        target_lang
    );

    let payload = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64_image) } }
                ]
            }
        ],
        "temperature": 0.1,
        "max_completion_tokens": 1024,
        "response_format": { "type": "json_object" }
    });

    let resp = ureq::post("https://api.groq.com/openai/v1/chat/completions")
        .set("Authorization", &format!("Bearer {}", api_key))
        .send_json(payload)?;

    let text_resp: String = resp.into_string()?;

    let json_resp: serde_json::Value = serde_json::from_str(&text_resp)
        .map_err(|e| anyhow::anyhow!("Invalid API JSON: {}. Body: {}", e, text_resp))?;
        
    let content_str = json_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in response"))?;

    let groq_resp: GroqResponse = serde_json::from_str(content_str)
        .map_err(|e| anyhow::anyhow!("LLM invalid JSON: {}. content: {}", e, content_str))?;
    
    Ok(groq_resp.translation)
}
