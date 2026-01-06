use crate::api::{translate_image_streaming, translate_text_streaming};
use crate::config::{Config, Preset, ProcessingBlock};
use crate::gui::settings_ui::get_localized_preset_name;
use crate::overlay::result::{
    create_result_window, get_chain_color, link_windows, update_window_text, RefineContext,
    WindowType, WINDOW_STATES,
};
use crate::overlay::text_input;
use crate::win_types::SendHwnd;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::types::{get_next_window_position, reset_window_position_queue};
use super::window::create_processing_window;

// --- CORE PIPELINE LOGIC ---

pub fn execute_chain_pipeline(
    initial_input: String,
    rect: RECT,
    config: Config,
    preset: Preset,
    context: RefineContext,
) {
    // 1. Create Processing Window (Gradient Glow)
    // This window stays on the current thread (UI thread context for this operation)
    let graphics_mode = config.graphics_mode.clone();
    let processing_hwnd = unsafe { create_processing_window(rect, graphics_mode) };
    unsafe {
        let _ = SendMessageW(processing_hwnd, WM_TIMER, Some(WPARAM(1)), Some(LPARAM(0)));
    }

    // 2. Start the chain execution on a BACKGROUND thread
    // We pass the processing_hwnd so the background thread can close it when appropriate
    let conf_clone = config.clone();
    let blocks = preset.blocks.clone();
    let connections = preset.block_connections.clone();
    let preset_id = preset.id.clone();

    let processing_hwnd_send = SendHwnd(processing_hwnd);
    std::thread::spawn(move || {
        // Reset position queue for new chain
        reset_window_position_queue();

        run_chain_step(
            0,
            initial_input,
            rect,
            blocks,
            connections, // Graph connections
            conf_clone,
            Arc::new(Mutex::new(None)),
            context,
            false,
            Some(processing_hwnd_send), // Pass the handle to be closed later
            Arc::new(AtomicBool::new(false)), // New chains start with cancellation = false
            preset_id,
        );
    });

    // 3. Keep the Processing Window alive on this thread until it is destroyed by the worker
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(Some(processing_hwnd)).as_bool() {
                break;
            }
        }
    }
}

/// Execute chain pipeline with a pre-created cancellation token
/// Used for continuous input mode to track and close previous chain windows
/// NOTE: For text presets, we don't create a processing window (gradient glow).
/// Instead, we rely on the refining animation baked into the result window.
pub fn execute_chain_pipeline_with_token(
    initial_input: String,
    rect: RECT,
    config: Config,
    preset: Preset,
    context: RefineContext,
    cancel_token: Arc<AtomicBool>,
) {
    // For text presets: NO processing window (gradient glow).
    // The result window itself shows the refining animation.

    let blocks = preset.blocks.clone();
    let connections = preset.block_connections.clone();

    // Reset position queue for new chain
    reset_window_position_queue();

    run_chain_step(
        0,
        initial_input,
        rect,
        blocks,
        connections,
        config,
        Arc::new(Mutex::new(None)),
        context,
        false,
        None, // No processing window for text presets
        cancel_token,
        preset.id.clone(),
    );
}

/// Recursive step to run a block in the chain (now supports graph with connections)
pub fn run_chain_step(
    block_idx: usize,
    input_text: String,
    current_rect: RECT,
    blocks: Vec<ProcessingBlock>,
    connections: Vec<(usize, usize)>, // Graph edges: (from_idx, to_idx)
    config: Config,
    parent_hwnd: Arc<Mutex<Option<SendHwnd>>>,
    context: RefineContext, // Passed to Block 0 (Image context)
    skip_execution: bool,   // If true, we just display result
    mut processing_indicator_hwnd: Option<SendHwnd>, // Handle to the "Processing..." overlay
    cancel_token: Arc<AtomicBool>, // Cancellation flag - if true, stop processing
    preset_id: String,
) {
    // Check if cancelled before starting
    if cancel_token.load(Ordering::Relaxed) {
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    if block_idx >= blocks.len() {
        // End of chain. If processing overlay is still active (e.g., all blocks were hidden), close it now.
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    let block = &blocks[block_idx];

    // 1. Resolve Model & Prompt
    let model_id = block.model.clone();
    let model_conf = crate::model_config::get_model_by_id(&model_id);
    let provider = model_conf
        .clone()
        .map(|m| m.provider)
        .unwrap_or("groq".to_string());
    let model_full_name = model_conf.map(|m| m.full_name).unwrap_or(model_id.clone());

    let mut final_prompt = block.prompt.clone();
    for (key, value) in &block.language_vars {
        final_prompt = final_prompt.replace(&format!("{{{}}}", key), value);
    }
    // Fallback: if {language1} is still in prompt but not in language_vars, use selected_language
    if final_prompt.contains("{language1}") && !block.language_vars.contains_key("language1") {
        final_prompt = final_prompt.replace("{language1}", &block.selected_language);
    }
    final_prompt = final_prompt.replace("{language}", &block.selected_language);

    // 2. Determine Visibility & Position
    let visible_count_before = blocks
        .iter()
        .take(block_idx)
        .filter(|b| b.show_overlay)
        .count();
    let bg_color = get_chain_color(visible_count_before);

    // For visible windows: use global queue for sequential snake positioning (first-come-first-serve)
    let my_rect = if block.show_overlay {
        get_next_window_position(current_rect)
    } else {
        current_rect // Hidden blocks don't consume a position
    };

    let mut my_hwnd: Option<HWND> = None;

    // 3. Create Window (if visible)
    // All blocks (including input_adapter) can show overlay if show_overlay is enabled
    let should_create_window = block.show_overlay;

    if block.block_type == "input_adapter" && !block.show_overlay {
        // Input adapter without overlay - invisible and instant pass-through
        // Do nothing here, skipping window creation
    } else if should_create_window {
        // For input_adapter with show_overlay: use the input context for display
        let ctx_clone = if block.block_type == "input_adapter" || block_idx == 0 {
            context.clone()
        } else {
            RefineContext::None
        };
        let m_id = model_id.clone();
        let prov = provider.clone();
        let prompt_c = final_prompt.clone();
        // CRITICAL: Override streaming to false if render_mode is markdown
        // Markdown + streaming doesn't work properly (causes missing content)
        let stream_en = if block.render_mode == "markdown" {
            false
        } else {
            block.streaming_enabled
        };
        let render_md = block.render_mode.clone();

        let parent_clone = parent_hwnd.clone();
        let (tx_hwnd, rx_hwnd) = std::sync::mpsc::channel();
        // For image blocks (processing), we defer showing until data arrives.
        // For input_adapter (display), we show immediately (handled by initial_content).
        let is_image_block = block.block_type == "image";

        // Check if we need to set full opacity (input adapter with image context)
        let is_input_adapter_image =
            block.block_type == "input_adapter" && matches!(context, RefineContext::Image(_));

        let locale = crate::gui::locale::LocaleText::get(&config.ui_language);

        // Generate initial content (HTML/Text) for the window immediately
        // This decouples content generation from window display loop
        let initial_content = if block.block_type == "input_adapter" {
            match &context {
                RefineContext::Image(img_data) => {
                    use base64::Engine;
                    let base64_img = base64::engine::general_purpose::STANDARD.encode(img_data);

                    // Simple magic byte detection for MIME type
                    let mime_type = if img_data.starts_with(&[0xff, 0xd8, 0xff]) {
                        "image/jpeg"
                    } else if img_data.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
                        "image/png"
                    } else {
                        "image/png" // Fallback
                    };

                    format!(
                        r#"<!DOCTYPE html>
<html>
<head>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:wght@400;500&display=swap">
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ 
    display: flex; 
    justify-content: center; 
    align-items: center; 
    min-height: 100vh;
    background: transparent;
    font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
}}
::-webkit-scrollbar {{ display: none; }}
.container {{
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    justify-content: center;
    align-items: center;
}}
.image {{
    width: 100%;
    height: auto;
    object-fit: contain;
    border-radius: 8px;
    transition: opacity 0.15s ease;
}}
.slider-container {{
    position: absolute;
    top: 16px;
    left: 50%;
    transform: translateX(-50%);
    background: rgba(20, 20, 30, 0.95);
    padding: 10px 16px;
    border-radius: 12px;
    display: flex;
    align-items: center;
    gap: 12px;
    opacity: 0;
    transition: opacity 0.2s ease;
    box-shadow: 0 4px 20px rgba(0,0,0,0.3);
    border: 1px solid rgba(255,255,255,0.08);
}}
.container:hover .slider-container {{
    opacity: 1;
}}
.slider-label {{
    color: #e0e0e0;
    font-size: 12px;
    white-space: nowrap;
}}
.slider {{
    -webkit-appearance: none;
    width: 120px;
    height: 4px;
    background: rgba(255,255,255,0.2);
    border-radius: 2px;
    outline: none;
    cursor: pointer;
}}
.slider::-webkit-slider-thumb {{
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    background: #8ab4f8;
    border-radius: 50%;
    cursor: pointer;
    box-shadow: 0 2px 5px rgba(0,0,0,0.2);
    transition: transform 0.15s ease;
}}
.slider::-webkit-slider-thumb:hover {{
    transform: scale(1.15);
}}
.value {{
    color: #fff;
    font-size: 12px;
    font-weight: 500;
    min-width: 36px;
    text-align: right;
}}
</style>
</head>
<body>
<div class="container">
    <img class="image" id="img" src="data:{};base64,{}" />
    <div class="slider-container">
        <span class="slider-label">{}</span>
        <input type="range" class="slider" id="opacity" min="0" max="100" value="100" />
        <span class="value" id="val">100%</span>
    </div>
</div>
<script>
const slider = document.getElementById('opacity');
const val = document.getElementById('val');
slider.oninput = function() {{
    val.textContent = this.value + '%';
    if (window.ipc) {{
        window.ipc.postMessage('opacity:' + this.value);
    }}
}};
</script>
</body>
</html>"#,
                        mime_type, base64_img, locale.opacity_label
                    )
                }
                RefineContext::Audio(wav_data) => {
                    use base64::Engine;
                    let base64_audio = base64::engine::general_purpose::STANDARD.encode(wav_data);
                    format!(
                        r#"<!DOCTYPE html>
<html>
<head>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:wght@400;500&display=swap">
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ 
    display: flex; 
    justify-content: center; 
    align-items: center; 
    min-height: 100vh; 
    background: transparent;
    font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
}}
::-webkit-scrollbar {{ display: none; }}
.audio-player {{
    background: #1e1e1e;
    border-radius: 12px;
    padding: 20px 24px;
    width: 100%;
    max-width: 400px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.3);
    border: 1px solid rgba(255, 255, 255, 0.08);
    position: relative;
}}
.waveform {{
    display: flex;
    align-items: center;
    gap: 2px;
    height: 60px;
    margin-bottom: 16px;
    justify-content: center;
}}
.wave-bar {{
    width: 3px;
    min-height: 4px;
    background: #8ab4f8;
    border-radius: 2px;
    transition: height 0.05s ease-out;
}}
.controls {{
    display: flex;
    align-items: center;
    gap: 14px;
}}
.play-btn {{
    width: 44px;
    height: 44px;
    background: #8ab4f8;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: transform 0.2s, background-color 0.2s;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
    flex-shrink: 0;
}}
.play-btn:hover {{
    transform: scale(1.05);
    background: #aecbfa;
}}
.play-btn svg {{
    fill: #1e1e1e;
    width: 18px;
    height: 18px;
    margin-left: 2px;
}}
.play-btn.playing svg {{
    margin-left: 0;
}}
.download-btn {{
    width: 36px;
    height: 36px;
    background: transparent;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.2s;
    flex-shrink: 0;
    margin-left: 4px;
}}
.download-btn:hover {{
    background: rgba(255, 255, 255, 0.1);
}}
.download-btn svg {{
    fill: #9aa0a6;
    width: 20px;
    height: 20px;
    transition: fill 0.2s;
}}
.download-btn:hover svg {{
    fill: #fff;
}}
.download-btn.success svg {{
    fill: #4CAF50;
}}
.download-btn.success:hover {{
    background: rgba(76, 175, 80, 0.15);
}}
.progress-container {{
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
}}
.progress-bar {{
    height: 4px;
    background: rgba(255,255,255,0.1);
    border-radius: 2px;
    overflow: hidden;
    cursor: pointer;
}}
.progress-fill {{
    height: 100%;
    background: #8ab4f8;
    border-radius: 2px;
    width: 0%;
    transition: width 0.1s;
}}
.time-display {{
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: #9aa0a6;
}}
.toast {{
    position: absolute;
    bottom: 74px;
    left: 50%;
    transform: translateX(-50%);
    background: rgba(30, 30, 35, 0.95);
    color: #fff;
    padding: 8px 16px;
    border-radius: 20px;
    font-size: 13px;
    font-weight: 500;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.3s ease;
    box-shadow: 0 4px 12px rgba(0,0,0,0.3);
    border: 1px solid rgba(255,255,255,0.1);
    white-space: nowrap;
    z-index: 100;
    backdrop-filter: blur(4px);
}}
.toast.show {{
    opacity: 1;
}}
audio {{ display: none; }}
</style>
</head>
<body>
<div class="audio-player">
    <div class="toast" id="toast">{}</div>
    <div class="waveform" id="waveform"></div>
    <div class="controls">
        <button class="play-btn" id="playBtn">
            <svg id="playIcon" viewBox="0 0 24 24"><path d="M8 5v14l11-7z"/></svg>
        </button>
        <div class="progress-container">
            <div class="progress-bar" id="progressBar">
                <div class="progress-fill" id="progress"></div>
            </div>
            <div class="time-display">
                <span id="current">0:00</span>
                <span id="duration">0:00</span>
            </div>
        </div>
        <button class="download-btn" id="downloadBtn" title="{}">
            <svg viewBox="0 0 24 24"><path d="M5 20h14v-2H5v2zM19 9h-4V3H9v6H5l7 7 7-7z"/></svg>
        </button>
    </div>
</div>
<audio id="audio">
    <source src="data:audio/wav;base64,{}" type="audio/wav">
</audio>
<script>
const audio = document.getElementById('audio');
const progress = document.getElementById('progress');
const playIcon = document.getElementById('playIcon');
const playBtn = document.getElementById('playBtn');
const downloadBtn = document.getElementById('downloadBtn');
const toast = document.getElementById('toast');
const currentTimeEl = document.getElementById('current');
const durationEl = document.getElementById('duration');
const waveformEl = document.getElementById('waveform');
const progressBar = document.getElementById('progressBar');

// Create waveform bars
const BAR_COUNT = 32;
for (let i = 0; i < BAR_COUNT; i++) {{
    const bar = document.createElement('div');
    bar.className = 'wave-bar';
    bar.style.height = '4px';
    waveformEl.appendChild(bar);
}}
const bars = waveformEl.querySelectorAll('.wave-bar');

// Web Audio API setup
let audioContext, analyser, source, dataArray;
let isSetup = false;

function setupAudio() {{
    if (isSetup) return;
    audioContext = new (window.AudioContext || window.webkitAudioContext)();
    analyser = audioContext.createAnalyser();
    analyser.fftSize = 64;
    source = audioContext.createMediaElementSource(audio);
    source.connect(analyser);
    analyser.connect(audioContext.destination);
    dataArray = new Uint8Array(analyser.frequencyBinCount);
    isSetup = true;
}}

function formatTime(s) {{
    if (isNaN(s)) return '0:00';
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return m + ':' + (sec < 10 ? '0' : '') + sec;
}}

function visualize() {{
    if (!analyser || audio.paused) return;
    analyser.getByteFrequencyData(dataArray);
    for (let i = 0; i < BAR_COUNT; i++) {{
        const idx = Math.floor(i * dataArray.length / BAR_COUNT);
        const value = dataArray[idx];
        const height = Math.max(4, (value / 255) * 56);
        bars[i].style.height = height + 'px';
    }}
    requestAnimationFrame(visualize);
}}

audio.onloadedmetadata = () => {{
    durationEl.textContent = formatTime(audio.duration);
}};

audio.ontimeupdate = () => {{
    const pct = (audio.currentTime / audio.duration) * 100;
    progress.style.width = pct + '%';
    currentTimeEl.textContent = formatTime(audio.currentTime);
}};

audio.onended = () => {{
    playIcon.innerHTML = '<path d="M8 5v14l11-7z"/>';
    playBtn.classList.remove('playing');
    bars.forEach(b => b.style.height = '4px');
}};

playBtn.onclick = () => {{
    setupAudio();
    if (audio.paused) {{
        audio.play();
        playIcon.innerHTML = '<path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/>';
        playBtn.classList.add('playing');
        visualize();
    }} else {{
        audio.pause();
        playIcon.innerHTML = '<path d="M8 5v14l11-7z"/>';
        playBtn.classList.remove('playing');
    }}
}};

downloadBtn.onclick = () => {{
    const link = document.createElement('a');
    link.href = audio.querySelector('source').src;
    const date = new Date();
    const ts = date.getFullYear() + '-' + (date.getMonth()+1) + '-' + date.getDate() + '_' + date.getHours() + '-' + date.getMinutes() + '-' + date.getSeconds();
    link.download = 'recording_' + ts + '.wav';
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);

    // Visual Feedback
    const originalIcon = downloadBtn.innerHTML;
    // Checkmark
    downloadBtn.innerHTML = '<svg viewBox="0 0 24 24"><path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/></svg>';
    downloadBtn.classList.add('success');
    toast.classList.add('show');

    setTimeout(() => {{
        downloadBtn.innerHTML = originalIcon;
        downloadBtn.classList.remove('success');
        toast.classList.remove('show');
    }}, 2500);
}};

progressBar.onclick = (e) => {{
    const rect = progressBar.getBoundingClientRect();
    const pct = (e.clientX - rect.left) / rect.width;
    audio.currentTime = pct * audio.duration;
}};
</script>
</body>
</html>"#,
                        locale.downloaded_successfully,
                        locale.download_recording_tooltip,
                        base64_audio
                    )
                }
                RefineContext::None => input_text.clone(),
            }
        } else {
            String::new()
        };
        let initial_content_clone = initial_content.clone();

        let cancel_token_thread = cancel_token.clone();
        std::thread::spawn(move || {
            // NOTE: wry handles COM internally, explicit initialization may interfere

            let hwnd = create_result_window(
                my_rect,
                WindowType::Primary,
                ctx_clone,
                m_id,
                prov,
                stream_en,
                false,
                prompt_c,
                bg_color,
                &render_md,
                initial_content_clone,
            );

            // Assign cancellation token immediately for linking/grouping
            // This is critical for input adapters since we don't wait for them in main thread
            {
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(hwnd.0 as isize)) {
                    st.cancellation_token = Some(cancel_token_thread.clone());
                }
            }

            if let Ok(p_guard) = parent_clone.lock() {
                if let Some(ph) = *p_guard {
                    link_windows(ph.0, hwnd);
                }
            }

            // For image blocks: DON'T show window yet - keep it hidden
            // It will be shown when first data arrives (in the streaming callback)
            // For text blocks: show immediately with refining animation
            if !is_image_block {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                }
            }
            let _ = tx_hwnd.send(SendHwnd(hwnd));

            unsafe {
                // If it's an image input adapter, set opacity to 255 (full opaque)
                // This allows the image itself to be fully visible, while the slider controls the image opacity
                if is_input_adapter_image {
                    // Import SetLayeredWindowAttributes locally if needed, or assume it's available via windows crate
                    use windows::Win32::Foundation::COLORREF;
                    use windows::Win32::UI::WindowsAndMessaging::{
                        SetLayeredWindowAttributes, LWA_ALPHA,
                    };
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                }

                let mut m = MSG::default();
                while GetMessageW(&mut m, None, 0, 0).into() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                    if !IsWindow(Some(hwnd)).as_bool() {
                        break;
                    }
                }
            }
        });

        if block.block_type == "input_adapter" {
            // Decoupled: don't wait for input adapter window
            my_hwnd = None;
        } else {
            my_hwnd = rx_hwnd.recv().ok().map(|h| h.0);
        }

        // Associate cancellation token with this window so destruction stops the chain
        if let Some(h) = my_hwnd {
            let mut s = WINDOW_STATES.lock().unwrap();
            if let Some(st) = s.get_mut(&(h.0 as isize)) {
                st.cancellation_token = Some(cancel_token.clone());
            }
        }

        // Show loading state in the new window
        // For TEXT blocks: use the refining rainbow edge animation
        // For IMAGE blocks: keep using the gradient glow/laser processing window
        // For input_adapter: show the input content immediately (no refining animation)
        if !skip_execution && my_hwnd.is_some() {
            if block.block_type == "input_adapter" {
                // Input adapter: show input content immediately, no refining animation
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(my_hwnd.unwrap().0 as isize)) {
                    st.is_refining = false;
                    st.is_streaming_active = false; // Show buttons immediately
                    st.font_cache_dirty = true;
                }
            } else if block.block_type != "image" {
                // Text block: use rainbow edge refining animation
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(my_hwnd.unwrap().0 as isize)) {
                    st.input_text = input_text.clone();
                    st.is_refining = true;
                    st.is_streaming_active = true; // Hide buttons during streaming
                    st.was_streaming_active = true; // Track for end-of-stream flush
                    st.font_cache_dirty = true;
                }
            } else {
                // Image block: also set streaming active to hide buttons
                let mut s = WINDOW_STATES.lock().unwrap();
                if let Some(st) = s.get_mut(&(my_hwnd.unwrap().0 as isize)) {
                    st.is_streaming_active = true; // Hide buttons during streaming
                    st.was_streaming_active = true; // Track for end-of-stream flush
                }
            }
        }

        // CRITICAL: Close the old "Processing..." overlay ONLY for text blocks (not input_adapter)
        // For image blocks, we want to keep the beautiful gradient glow animation alive
        if block.block_type != "image" && block.block_type != "input_adapter" {
            if let Some(h) = processing_indicator_hwnd {
                unsafe {
                    let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                // Consumed. Don't pass it to next steps.
                processing_indicator_hwnd = None;
            }
        }
    } else {
        // HIDDEN BLOCK:
        // We do NOT close processing_indicator_hwnd.
        // It keeps spinning/glowing while we execute this hidden block.
        // It will be passed to the next block.
    }

    // 4. Execution (API Call)
    // 4. Execution (API Call)
    let input_text_for_history = input_text.clone();
    let result_text = if block.block_type == "input_adapter" {
        // Pass-through: return input as-is immediately
        input_text.clone()
    } else if skip_execution {
        if let Some(h) = my_hwnd {
            update_window_text(h, &input_text);
        }
        input_text
    } else {
        let groq_key = config.api_key.clone();
        let gemini_key = config.gemini_api_key.clone();
        // Use JSON format for single-block image extraction (helps with structured output)
        let use_json = block_idx == 0 && blocks.len() == 1 && blocks[0].block_type == "image";

        // CRITICAL: Override streaming to false if render_mode is markdown
        // Markdown + streaming doesn't work properly (causes missing content)
        let actual_streaming_enabled = if block.render_mode == "markdown" {
            false
        } else {
            block.streaming_enabled
        };

        let accumulated = Arc::new(Mutex::new(String::new()));
        let acc_clone = accumulated.clone();

        // Identify if this is the first block in the chain that actually processes input (skipping adapters)
        let is_first_processing_block = blocks
            .iter()
            .position(|b| b.block_type != "input_adapter")
            .map(|pos| pos == block_idx)
            .unwrap_or(false);

        // SETUP RETRY VARIABLES
        let mut current_model_id = model_id.clone();
        let mut current_provider = provider.clone();
        let mut current_model_full_name = model_full_name.clone();
        // Variable to hold the model name for error reporting (must survive loop)
        let mut model_name_for_error = model_full_name.clone();

        let mut failed_model_ids: Vec<String> = Vec::new();
        let mut retry_count = 0;
        const MAX_RETRIES: usize = 2;

        // For image blocks: track if window has been shown and share processing_hwnd
        let window_shown = Arc::new(Mutex::new(block.block_type != "image")); // true for text, false for image
        let window_shown_clone = window_shown.clone();
        let processing_hwnd_shared = Arc::new(Mutex::new(processing_indicator_hwnd));
        let processing_hwnd_clone = processing_hwnd_shared.clone();

        // RETRY LOOP
        let res = loop {
            // Update model_name_for_error to current attempt
            model_name_for_error = current_model_full_name.clone();

            let res_inner = if is_first_processing_block
                && block.block_type == "image"
                && matches!(context, RefineContext::Image(_))
            {
                // Image Block (first processing block in chain)
                if let RefineContext::Image(img_data) = context.clone() {
                    let img = image::load_from_memory(&img_data)
                        .expect("Failed to load png")
                        .to_rgba8();

                    let acc_clone_inner = acc_clone.clone();
                    let my_hwnd_inner = my_hwnd;
                    let window_shown_inner = window_shown_clone.clone();
                    let proc_hwnd_inner = processing_hwnd_clone.clone();

                    // CLEAR ACCUMULATOR ON RETRY
                    if retry_count > 0 {
                        if let Ok(mut lock) = acc_clone.lock() {
                            lock.clear();
                        }
                    }

                    translate_image_streaming(
                        &groq_key,
                        &gemini_key,
                        final_prompt.clone(),
                        current_model_full_name.clone(),
                        current_provider.clone(),
                        img,
                        Some(img_data),
                        actual_streaming_enabled,
                        use_json,
                        move |chunk| {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u32)
                                .unwrap_or(0);

                            let mut t = acc_clone_inner.lock().unwrap();
                            // Handle WIPE_SIGNAL - clear accumulator and use content after signal
                            if chunk.starts_with(crate::api::WIPE_SIGNAL) {
                                t.clear();
                                t.push_str(&chunk[crate::api::WIPE_SIGNAL.len()..]);
                            } else {
                                t.push_str(chunk);
                            }

                            if let Some(h) = my_hwnd_inner {
                                // On first chunk for image blocks: show window and close processing indicator
                                {
                                    let mut shown = window_shown_inner.lock().unwrap();
                                    if !*shown {
                                        *shown = true;
                                        unsafe {
                                            let _ = ShowWindow(h, SW_SHOW);
                                        }
                                        // Close processing indicator
                                        let mut proc_hwnd = proc_hwnd_inner.lock().unwrap();
                                        if let Some(ph) = proc_hwnd.take() {
                                            unsafe {
                                                let _ = PostMessageW(
                                                    Some(ph.0),
                                                    WM_CLOSE,
                                                    WPARAM(0),
                                                    LPARAM(0),
                                                );
                                            }
                                        }
                                    }
                                }
                                {
                                    let mut s = WINDOW_STATES.lock().unwrap();
                                    if let Some(st) = s.get_mut(&(h.0 as isize)) {
                                        st.is_refining = false;

                                        st.font_cache_dirty = true;
                                    }
                                }
                                update_window_text(h, &t);
                            }
                        },
                    )
                } else {
                    Err(anyhow::anyhow!("Missing image context"))
                }
            } else {
                // Text Block
                // Compute search label for compound models
                let search_label = Some(get_localized_preset_name(&preset_id, &config.ui_language));

                // CLEAR ACCUMULATOR ON RETRY
                if retry_count > 0 {
                    if let Ok(mut lock) = acc_clone.lock() {
                        lock.clear();
                    }
                }

                let acc_clone_inner = acc_clone.clone();
                translate_text_streaming(
                    &groq_key,
                    &gemini_key,
                    input_text.clone(),
                    final_prompt.clone(),
                    current_model_full_name.clone(),
                    current_provider.clone(),
                    actual_streaming_enabled,
                    false,
                    search_label,
                    &config.ui_language,
                    move |chunk| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u32)
                            .unwrap_or(0);

                        let mut t = acc_clone_inner.lock().unwrap();
                        // Handle WIPE_SIGNAL - clear accumulator and use content after signal
                        if chunk.starts_with(crate::api::WIPE_SIGNAL) {
                            t.clear();
                            t.push_str(&chunk[crate::api::WIPE_SIGNAL.len()..]);
                        } else {
                            t.push_str(chunk);
                        }

                        if let Some(h) = my_hwnd {
                            {
                                let mut s = WINDOW_STATES.lock().unwrap();
                                if let Some(st) = s.get_mut(&(h.0 as isize)) {
                                    st.is_refining = false;

                                    st.font_cache_dirty = true;
                                }
                            }
                            update_window_text(h, &t);
                        }
                    },
                )
            };

            // CHECK RESULT AND RETRY IF NEEDED
            match res_inner {
                Ok(val) => break Ok(val),
                Err(e) => {
                    // Check if retryable
                    if retry_count < MAX_RETRIES
                        && crate::overlay::utils::is_retryable_error(&e.to_string())
                    {
                        retry_count += 1;
                        failed_model_ids.push(current_model_id.clone());

                        // Determine fallback
                        let current_type = if block.block_type == "image" {
                            crate::model_config::ModelType::Vision
                        } else {
                            crate::model_config::ModelType::Text
                        };

                        // Try to get next model
                        if let Some(next_model) = crate::model_config::resolve_fallback_model(
                            &current_model_id,
                            &failed_model_ids,
                            &current_type,
                            &config,
                        ) {
                            current_model_id = next_model.id;
                            current_provider = next_model.provider;
                            current_model_full_name = next_model.full_name;

                            // Notify via Window Text
                            if let Some(h) = my_hwnd {
                                let lang = config.ui_language.clone();
                                let retry_msg = match lang.as_str() {
                                    "vi" => {
                                        format!("(Đang thử lại {}...)", current_model_full_name)
                                    }
                                    "ko" => format!("({} 재시도 중...)", current_model_full_name),
                                    "ja" => format!("({} 再試行中...)", current_model_full_name),
                                    "zh" => format!("(正在重试 {}...)", current_model_full_name),
                                    _ => format!("(Retrying {}...)", current_model_full_name),
                                };
                                update_window_text(h, &retry_msg);
                            }

                            continue; // Retry Loop
                        }
                    }
                    // Not retryable or max retries exceeded
                    break Err(e);
                }
            }
        };

        // CRITICAL: Set is_streaming_active = false AND pending_text atomically in the same lock
        // to prevent race condition where the timer detects streaming_just_ended but pending_text
        // hasn't been set yet (causing the final text to be throttled and not rendered)
        match res {
            Ok(txt) => {
                if let Some(h) = my_hwnd {
                    let mut s = WINDOW_STATES.lock().unwrap();
                    if let Some(st) = s.get_mut(&(h.0 as isize)) {
                        st.is_refining = false;
                        st.is_streaming_active = false; // Streaming complete, show buttons
                        st.font_cache_dirty = true;
                        // Set pending_text in same lock to avoid race condition
                        st.pending_text = Some(txt.clone());
                        st.full_text = txt.clone();
                    }
                }
                txt
            }
            Err(e) => {
                let lang = config.ui_language.clone();
                let err = crate::overlay::utils::get_error_message(
                    &e.to_string(),
                    &lang,
                    Some(&model_name_for_error),
                );
                if let Some(h) = my_hwnd {
                    // CRITICAL: For image blocks, the window may still be hidden if on_chunk was never called
                    // We must show it now to display the error message
                    {
                        let mut shown = window_shown.lock().unwrap();
                        if !*shown {
                            *shown = true;
                            unsafe {
                                let _ = ShowWindow(h, SW_SHOW);
                            }
                            // Also close the processing indicator
                            let mut proc_hwnd = processing_hwnd_shared.lock().unwrap();
                            if let Some(ph) = proc_hwnd.take() {
                                unsafe {
                                    let _ =
                                        PostMessageW(Some(ph.0), WM_CLOSE, WPARAM(0), LPARAM(0));
                                }
                            }
                        }
                    }
                    // Set is_streaming_active = false AND pending_text atomically
                    let mut s = WINDOW_STATES.lock().unwrap();
                    if let Some(st) = s.get_mut(&(h.0 as isize)) {
                        st.is_refining = false;
                        st.is_streaming_active = false;
                        st.font_cache_dirty = true;
                        st.pending_text = Some(err.clone());
                        st.full_text = err.clone();
                    }
                }
                String::new()
            }
        }
    };

    // 5. Post-Processing (Copy)
    // 5. Post-Processing (Copy)
    // Handle Auto-Copy for both Text and Image inputs
    // For input_adapter, we must check if we should copy the SOURCE (Image or Text)
    // result_text is input_text for adapters
    let is_input_adapter = block.block_type == "input_adapter";
    let has_content = !result_text.trim().is_empty();

    if block.auto_copy {
        // CASE 1: Image Input Adapter (Source Copy)
        // If this is an input adapter AND we have image context, copy the image.
        // We do this even if result_text (input_text) is empty, because image source has no text.
        if is_input_adapter {
            if let RefineContext::Image(img_data) = context.clone() {
                let img_data_clone = img_data.clone();
                std::thread::spawn(move || {
                    crate::overlay::utils::copy_image_to_clipboard(&img_data_clone);
                });
            }
        }

        // CASE 2: Text Content (Result or Source Text) OR Image Content (Source Copy)
        // Only copy text if it is NOT empty.
        // For paste logic: we proceed if EITHER we have text content OR we just copied an image (is_input_adapter && image context).
        let image_copied = is_input_adapter && matches!(context, RefineContext::Image(_));

        if has_content {
            let txt_c = result_text.clone();
            let txt_for_badge = result_text.clone();
            // Only show badge for actual processed results, NOT for input_adapter blocks
            // because input_adapter just passes through text that was already copied to clipboard
            // by text_selection.rs (the "b?? ??? d?" copy for processing)
            let should_show_badge = !is_input_adapter;
            std::thread::spawn(move || {
                crate::overlay::utils::copy_to_clipboard(&txt_c, HWND::default());
                // Show auto-copy badge notification with text snippet (skip for input_adapter)
                if should_show_badge {
                    crate::overlay::auto_copy_badge::show_auto_copy_badge_text(&txt_for_badge);
                }
            });
        } else if image_copied {
            // For image-only copy, show the badge with image message
            // (this is intentional - image wasn't in clipboard before)
            crate::overlay::auto_copy_badge::show_auto_copy_badge_image();
        }

        // Only trigger paste for:
        // 1. Non-input_adapter blocks with text content (actual processed results)
        // 2. Image copies from input_adapter (intentional image copy)
        // This prevents double-paste when input_adapter has auto_copy enabled alongside a processing block
        let should_trigger_paste = (has_content && !is_input_adapter) || image_copied;

        if should_trigger_paste {
            // Re-clone for the paste thread
            let txt_c = result_text.clone();
            let preset_id_clone = preset_id.clone();

            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Get auto_paste settings from the RUNNING preset (by ID), not active_preset_idx
                let (should_add_newline, should_paste, target_window) = {
                    let app = crate::APP.lock().unwrap();
                    // Find the preset that's actually running this chain
                    if let Some(preset) =
                        app.config.presets.iter().find(|p| p.id == preset_id_clone)
                    {
                        (
                            preset.auto_paste_newline,
                            preset.auto_paste,
                            app.last_active_window,
                        )
                    } else {
                        // Fallback to active preset if not found (shouldn't happen)
                        let active_idx = app.config.active_preset_idx;
                        if active_idx < app.config.presets.len() {
                            let preset = &app.config.presets[active_idx];
                            (
                                preset.auto_paste_newline,
                                preset.auto_paste,
                                app.last_active_window,
                            )
                        } else {
                            (false, false, app.last_active_window)
                        }
                    }
                };

                // If strictly image copied (no text content), we ignore newline logic and just paste (Ctrl+V)
                // If text content exists, we do the full text logic.
                let final_text = if !txt_c.trim().is_empty() {
                    if should_add_newline {
                        format!("{}\n", txt_c)
                    } else {
                        txt_c.clone()
                    }
                } else {
                    String::new() // No text to modify/inject
                };

                // NOTE: We ALREADY copied to clipboard above (Text or Image).
                // Now we just handle the PASTE action.

                if should_paste {
                    // Special Case: If it's pure image copy (no text), we MUST use generic Ctrl+V paste.
                    // We cannot use text injection or set_editor_text.
                    if txt_c.trim().is_empty() {
                        // Image-only paste path
                        if let Some(target) = target_window {
                            crate::overlay::utils::force_focus_and_paste(target.0);
                        }
                    } else {
                        // Text paste path (supports injection)
                        // Check if text input window is active - if so, set text directly
                        if text_input::is_active() {
                            // Use set_editor_text to inject text into the webview editor
                            text_input::set_editor_text(&final_text);
                            text_input::refocus_editor();
                        }
                        // Check if refine input is active - if so, set text there
                        else if crate::overlay::result::refine_input::is_any_refine_active() {
                            if let Some(parent) =
                                crate::overlay::result::refine_input::get_active_refine_parent()
                            {
                                crate::overlay::result::refine_input::set_refine_text(
                                    parent,
                                    &final_text,
                                );
                            }
                        } else if let Some(target) = target_window {
                            // Normal paste to last active window
                            crate::overlay::utils::force_focus_and_paste(target.0);
                        }
                    }
                }
            });
        }
    }

    // Auto-Speak
    if block.auto_speak && !result_text.trim().is_empty() {
        let txt_s = result_text.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            crate::api::tts::TTS_MANAGER.speak(&txt_s, 0);
        });
    }

    // SAVE TO HISTORY: Handle both Text and Image blocks
    if block.show_overlay && !result_text.trim().is_empty() {
        let text_for_history = result_text.clone();

        if block.block_type == "text" {
            let input_text_clone = input_text_for_history.clone();
            std::thread::spawn(move || {
                if let Ok(app) = crate::APP.lock() {
                    app.history.save_text(text_for_history, input_text_clone);
                }
            });
        } else if block.block_type == "image" {
            // For image blocks, we need to grab the image data from the context
            // context is RefineContext::Image(Vec<u8>) for the first block
            if let RefineContext::Image(img_bytes) = context.clone() {
                std::thread::spawn(move || {
                    // Decode PNG bytes back to ImageBuffer for the history saver
                    // (HistoryManager::save_image expects ImageBuffer<Rgba<u8>, ...>)
                    if let Ok(img_dynamic) = image::load_from_memory(&img_bytes) {
                        let img_buffer = img_dynamic.to_rgba8();
                        if let Ok(app) = crate::APP.lock() {
                            app.history.save_image(img_buffer, text_for_history);
                        }
                    }
                });
            }
        }
    }

    // 6. Chain Next Steps (Graph-based: find all downstream blocks)
    // Check cancellation before continuing
    if cancel_token.load(Ordering::Relaxed) {
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return;
    }

    // For input_adapter blocks, ALWAYS continue to downstream blocks even if result_text is empty
    // This is critical for image presets where the image data is in context, not input_text
    let should_continue = !result_text.trim().is_empty() || block.block_type == "input_adapter";

    if should_continue {
        // Find all downstream blocks from connections
        let downstream_indices: Vec<usize> = connections
            .iter()
            .filter(|(from, _)| *from == block_idx)
            .map(|(_, to)| *to)
            .collect();

        // Determine next blocks:
        // - If connections vec is completely empty (legacy linear chain), use block_idx + 1 fallback
        // - If connections vec has entries (graph mode), use ONLY explicit connections
        let next_blocks: Vec<usize> = if connections.is_empty() {
            // Legacy mode: no graph connections defined, use linear chain
            if block_idx + 1 < blocks.len() {
                vec![block_idx + 1]
            } else {
                vec![]
            }
        } else {
            // Graph mode: use only explicit connections (no fallback)
            downstream_indices
        };

        if next_blocks.is_empty() {
            // End of chain
            if let Some(h) = processing_indicator_hwnd {
                unsafe {
                    let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            return;
        }

        let next_parent = if my_hwnd.is_some() {
            Arc::new(Mutex::new(my_hwnd.map(|h| SendHwnd(h))))
        } else {
            parent_hwnd
        };

        let base_rect = if my_hwnd.is_some() {
            my_rect
        } else {
            current_rect
        };

        // For the first downstream block, pass the processing indicator (if any)
        // For additional parallel branches, spawn new threads without the indicator
        let first_next = next_blocks[0];
        let parallel_branches: Vec<usize> = next_blocks.into_iter().skip(1).collect();

        // Spawn parallel threads for additional branches FIRST
        let next_context = if block.block_type == "input_adapter" {
            context.clone()
        } else {
            RefineContext::None
        };

        let next_skip_execution = if skip_execution {
            // Continue skipping if current block didn't "consume" the skipped output
            // Input adapter never consumes/displays, so we keep skipping until we hit the actual source block
            block.block_type == "input_adapter"
        } else {
            false
        };

        let _s_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let _s_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

        for (branch_index, next_idx) in parallel_branches.iter().enumerate() {
            let result_clone = result_text.clone();
            let blocks_clone = blocks.clone();
            let conns_clone = connections.clone();
            let config_clone = config.clone();
            let cancel_clone = cancel_token.clone();
            let parent_clone = next_parent.clone();
            let preset_id_clone = preset_id.clone();
            let next_idx_copy = *next_idx;

            // Capture next_context for parallel branches
            let branch_context = next_context.clone();

            // Position will be determined individually by get_next_window_position inside run_chain_step
            // We just pass the base_rect as a reference point
            let branch_rect = base_rect;

            // Incremental delay for each branch (300ms, 600ms, 900ms, ...)
            // This naturally staggers WebView2 creation without blocking mutexes
            let delay_ms = (branch_index as u64 + 1) * 300;

            std::thread::spawn(move || {
                // CRITICAL: Initialize COM on this thread - required for WebView2
                unsafe {
                    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }

                // Stagger WebView2 creation across parallel branches
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));

                run_chain_step(
                    next_idx_copy,
                    result_clone,
                    branch_rect,
                    blocks_clone,
                    conns_clone,
                    config_clone,
                    parent_clone,
                    branch_context, // Pass the captured context
                    next_skip_execution,
                    None, // No processing indicator for parallel branches
                    cancel_clone,
                    preset_id_clone,
                );
            });
        }

        // Continue with the first downstream block on current thread
        run_chain_step(
            first_next,
            result_text,
            base_rect,
            blocks.clone(),
            connections,
            config,
            next_parent,
            next_context, // Pass the context
            next_skip_execution,
            processing_indicator_hwnd, // Pass it along (might be None or Some)
            cancel_token,              // Pass the same token through the chain
            preset_id,
        );
    } else {
        // Chain stopped unexpectedly (empty result or error)
        // Ensure processing overlay is closed
        if let Some(h) = processing_indicator_hwnd {
            unsafe {
                let _ = PostMessageW(Some(h.0), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}
