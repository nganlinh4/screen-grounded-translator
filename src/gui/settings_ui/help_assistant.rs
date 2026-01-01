//! Help Assistant - Ask questions about SGT and get AI-powered answers
//! Uses the TextInput overlay for input and markdown_view for displaying responses

use crate::api::client::UREQ_AGENT;
use std::sync::atomic::{AtomicBool, Ordering};

/// Static flag to track if help input is active
static HELP_INPUT_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Check if the help assistant input is currently active
pub fn is_modal_open() -> bool {
    HELP_INPUT_ACTIVE.load(Ordering::SeqCst)
}

/// Fetch the repomix XML from GitHub
fn fetch_repomix_xml() -> Result<String, String> {
    let url =
        "https://raw.githubusercontent.com/nganlinh4/screen-goated-toolbox/main/repomix-output.xml";

    match UREQ_AGENT.get(url).call() {
        Ok(response) => response
            .into_string()
            .map_err(|e| format!("Failed to read response: {}", e)),
        Err(e) => Err(format!("Failed to fetch XML: {}", e)),
    }
}

/// Ask Gemini a question about SGT
fn ask_gemini(gemini_api_key: &str, question: &str, context_xml: &str) -> Result<String, String> {
    if gemini_api_key.trim().is_empty() {
        return Err("Gemini API key not configured. Please set it in Global Settings.".to_string());
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-robotics-er-1.5-preview:generateContent?key={}",
        gemini_api_key
    );

    let system_prompt = r#"
Answer the user in a helpful, concise and easy to understand way in the question's language, no made up infomation, only the true infomation. Go straight to the point, dont mention thing like "Based on the source code", if answer needs to mention the UI, be sure to use correct i18n locale terms matching the question's language. Format your response in Markdown."#;

    let user_message = format!(
        "{}\n\n---\nSource Code Context:\n{}\n---\n\nUser Question: {}",
        system_prompt, context_xml, question
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{
                "text": user_message
            }]
        }],
        "generationConfig": {
            "maxOutputTokens": 2048,
            "temperature": 0.7
        }
    });

    let response = UREQ_AGENT
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("API request failed: {}", e))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Extract text from response
    json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to extract response text".to_string())
}

/// Show the help assistant input using TextInput overlay
/// When user submits, query Gemini and show result in markdown_view
pub fn show_help_input() {
    HELP_INPUT_ACTIVE.store(true, Ordering::SeqCst);

    // Get config and API key
    let (gemini_api_key, ui_language) = {
        let app = crate::APP.lock().unwrap();
        (
            app.config.gemini_api_key.clone(),
            app.config.ui_language.clone(),
        )
    };

    // Get localized placeholder text
    let placeholder = match ui_language.as_str() {
        "vi" => "Hỏi gì về SGT? (VD: Làm sao để dịch vùng màn hình?)",
        "ko" => "SGT에 대해 무엇을 물어볼까요?",
        _ => "Ask anything about SGT (e.g., How do I translate a screen region?)",
    };

    // Show the text input overlay
    crate::overlay::text_input::show(
        placeholder.to_string(),
        ui_language.clone(),
        String::new(), // No cancel hotkey
        false,         // Not continuous mode
        move |question, _hwnd| {
            // User submitted a question
            let question = question.trim().to_string();
            if question.is_empty() {
                HELP_INPUT_ACTIVE.store(false, Ordering::SeqCst);
                return;
            }

            let gemini_key = gemini_api_key.clone();
            let lang = ui_language.clone();

            // Process in a dedicated thread (runs message loop for results)
            std::thread::spawn(move || {
                // Show loading state
                let loading_msg = match lang.as_str() {
                    "vi" => "⏳ Đang gọi cho tác giả nganlinh4 ... Kkk đùa thôi, đợi tí nha",
                    "ko" => "⏳ 작가 nganlinh4에게 전화 중... ㅋㅋ 농담이고, 잠깐만 기다려",
                    _ => "⏳ Calling author nganlinh4 ... Kkk joke, wait a bit",
                };

                // Initialize COM for WebView2
                unsafe {
                    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }

                // Get screen center for result display
                use windows::Win32::UI::WindowsAndMessaging::{
                    DispatchMessageW, GetMessageW, GetSystemMetrics, SetForegroundWindow,
                    ShowWindow, TranslateMessage, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW,
                };

                let (screen_w, screen_h) =
                    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };

                let center_rect = windows::Win32::Foundation::RECT {
                    left: screen_w / 2 - 300,
                    top: screen_h / 2 - 200,
                    right: screen_w / 2 + 300,
                    bottom: screen_h / 2 + 200,
                };

                // 1. Create the Result Window (Loading State)
                // This must happen on the thread that runs the message loop
                let result_hwnd = crate::overlay::result::create_result_window(
                    center_rect,
                    crate::overlay::result::WindowType::Primary,
                    crate::overlay::result::RefineContext::None,
                    "gemini-2.0-flash-exp".to_string(),
                    "gemini".to_string(),
                    false,
                    false,
                    "Ask SGT".to_string(),
                    0,
                    "markdown",
                    loading_msg.to_string(),
                );

                // Show the window (create_result_window creates it hidden by default)
                unsafe {
                    let _ = ShowWindow(result_hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(result_hwnd);
                }

                // 2. Spawn a background worker for the API call (so we don't block the message loop)
                // HWND is not Send, so cast to isize to pass across threads
                let api_hwnd_val = result_hwnd.0 as isize;
                std::thread::spawn(move || {
                    let api_hwnd =
                        windows::Win32::Foundation::HWND(api_hwnd_val as *mut std::ffi::c_void);
                    // Fetch context and ask Gemini
                    let result = match fetch_repomix_xml() {
                        Ok(xml) => ask_gemini(&gemini_key, &question, &xml),
                        Err(e) => Err(format!("Failed to fetch context: {}", e)),
                    };

                    // Format the response
                    let response = match result {
                        Ok(answer) => format!("## ❓ {}\n\n{}", question, answer),
                        Err(e) => format!("## ❌ Error\n\n{}", e),
                    };

                    // Update the window text (thead-safe update via global state + InvalidateRect)
                    crate::overlay::result::update_window_text(api_hwnd, &response);
                });

                // 3. Run the Message Loop for the Result Window
                // This keeps the window alive and handling events (paint, mouse, webview, etc.)
                unsafe {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                // Loop ends when PostQuitMessage is called (e.g. on window close)
                HELP_INPUT_ACTIVE.store(false, Ordering::SeqCst);
            });
        },
    );
}
