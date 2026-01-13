use crate::gui::locale::LocaleText;
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use std::cell::RefCell;
use std::num::NonZeroIsize;
use std::sync::{Mutex, Once};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebViewBuilder};

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

static REGISTER_INPUT_CLASS: Once = Once::new();
static INPUT_HWND: AtomicIsize = AtomicIsize::new(0);
static IS_WARMING_UP: AtomicBool = AtomicBool::new(false);
static IS_WARMED_UP: AtomicBool = AtomicBool::new(false);
static IS_SHOWING: AtomicBool = AtomicBool::new(false);

// COL_DARK_BG removed

// Global storage for submitted text (from webview IPC)
lazy_static::lazy_static! {
    static ref SUBMITTED_TEXT: Mutex<Option<String>> = Mutex::new(None);
    static ref SHOULD_CLOSE: Mutex<bool> = Mutex::new(false);
    static ref SHOULD_CLEAR_ONLY: Mutex<bool> = Mutex::new(false);

    // Config Storage (Thread-safe for persistent window)
    static ref CFG_TITLE: Mutex<String> = Mutex::new(String::new());
    static ref CFG_LANG: Mutex<String> = Mutex::new(String::new());
    static ref CFG_CANCEL: Mutex<String> = Mutex::new(String::new());
    static ref CFG_CALLBACK: Mutex<Option<Box<dyn Fn(String, HWND) + Send>>> = Mutex::new(None);
    static ref CFG_CONTINUOUS: Mutex<bool> = Mutex::new(false);

    // Cross-thread text injection (for auto-paste from transcription)
    static ref PENDING_TEXT: Mutex<Option<String>> = Mutex::new(None);
}

const WM_APP_SHOW: u32 = WM_USER + 99;
const WM_APP_SET_TEXT: u32 = WM_USER + 100; // New: trigger text injection from other threads
const WM_APP_HIDE: u32 = WM_USER + 101; // New: trigger fade out sequence

// Thread-local storage for WebView (not Send)
thread_local! {
    static TEXT_INPUT_WEBVIEW: RefCell<Option<wry::WebView>> = RefCell::new(None);
    // Shared WebContext for this thread using common data directory
    static TEXT_INPUT_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

/// Wrapper for HWND to implement HasWindowHandle
struct HwndWrapper(HWND);

impl HasWindowHandle for HwndWrapper {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let hwnd = self.0 .0 as isize;
        if let Some(non_zero) = NonZeroIsize::new(hwnd) {
            let mut handle = Win32WindowHandle::new(non_zero);
            handle.hinstance = None;
            let raw = RawWindowHandle::Win32(handle);
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        } else {
            Err(HandleError::Unavailable)
        }
    }
}

/// CSS for the modern text input editor
fn get_editor_css(is_dark: bool) -> String {
    let vars = if is_dark {
        r#"
        :root {
            /* Premium Dark Mode (Google UI Inspired) */
            --bg-color: rgba(32, 33, 36, 0.8);
            --text-color: #e8eaed;
            --header-text: #9aa0a6;
            --footer-text: #9aa0a6;
            --placeholder-color: #9aa0a6;
            --scrollbar-thumb: #5f6368;
            --scrollbar-thumb-hover: #80868b;
            --btn-bg: #3c4043; /* Elevated surface */
            --btn-border: rgba(255, 255, 255, 0.1);
            --mic-fill: #8ab4f8; 
            --mic-border: transparent;
            --mic-hover-bg: rgba(138, 180, 248, 0.12);
            --send-fill: #8ab4f8;
            --send-border: transparent;
            --send-hover-bg: rgba(138, 180, 248, 0.12);
            --hint-color: #9aa0a6;
            --close-hover-bg: rgba(232, 234, 237, 0.08);
            --container-border: 1px solid #3c4043;
            --container-shadow: 0 0px 16px rgba(0,0,0,0.25);
            --input-bg: #303134; /* Base surface */
            --input-border: 1px solid transparent;
            --wave-color: #8ab4f8; 
        }
        "#
    } else {
        r#"
        :root {
            /* Premium Light Mode (Google UI Inspired) */
            --bg-color: rgba(255, 255, 255, 0.75);
            --text-color: #202124;
            --header-text: #5f6368;
            --footer-text: #5f6368;
            --wave-color: #1a73e8;
            --placeholder-color: #5f6368;
            --scrollbar-thumb: #dadce0;
            --scrollbar-thumb-hover: #bdc1c6;
            --btn-bg: #ffffff; /* Elevated action button */
            --btn-border: #dadce0;
            --mic-fill: #1a73e8;
            --mic-border: transparent;
            --mic-hover-bg: rgba(26, 115, 232, 0.06);
            --send-fill: #1a73e8;
            --send-border: transparent;
            --send-hover-bg: rgba(26, 115, 232, 0.06);
            --hint-color: #5f6368;
            --close-hover-bg: rgba(32, 33, 36, 0.04);
            --container-border: 1px solid #dadce0;
            --container-shadow: 0 0px 16px rgba(0,0,0,0.25);
            --input-bg: #f1f3f4; /* Base surface */
            --input-border: 1px solid transparent;
        }
        "#
    };

    format!(
        r#"
    {vars}

    html, body {{
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: transparent;
        padding: 10px; /* Reduced to fit calc(100% - 20px) better */
        font-family: 'Google Sans Flex', sans-serif;
        font-variation-settings: 'ROND' 100;
    }}
    
    * {{ 
        box-sizing: border-box; 
        margin: 0; 
        padding: 0; 
        user-select: none; 
        font-variation-settings: 'ROND' 100; 
    }}
    
    *::-webkit-scrollbar {{
        width: 10px;
        height: 10px;
        background: transparent;
    }}
    *::-webkit-scrollbar-thumb {{
        background: var(--scrollbar-thumb);
        border-radius: 5px;
        border: 2px solid transparent;
        background-clip: content-box;
    }}
    *::-webkit-scrollbar-thumb:hover {{
        background: var(--scrollbar-thumb-hover);
        border: 2px solid transparent;
        background-clip: content-box;
    }}
    
    .editor-container {{
        width: calc(100% - 20px);
        height: calc(100% - 20px);
        margin: 10px;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        background: var(--bg-color);
        position: relative;
        border-radius: 20px;
        border: var(--container-border);
        box-shadow: var(--container-shadow);
        
        /* Initial State for Animation */
        opacity: 0;
        transform: scale(0.95);
        transition: background 0.2s, border-color 0.2s;
    }}

    .editor-container.entering {{
        animation: inputFadeIn 0.2s cubic-bezier(0.2, 0, 0, 1) forwards;
    }}

    .editor-container.exiting {{
        animation: inputFadeOut 0.15s cubic-bezier(0.2, 0, 0, 1) forwards;
    }}

    @keyframes inputFadeIn {{
        to {{ opacity: 1; transform: scale(1); }}
    }}

    @keyframes inputFadeOut {{
        from {{ opacity: 1; transform: scale(1); }}
        to {{ opacity: 0; transform: scale(0.95); }}
    }}
    
    /* Header (Draggable) */
    .header {{
        height: 32px;
        background: transparent;
        display: flex;
        align-items: center;
        padding: 0 10px;
        cursor: default;
        /* No border for header to seamless blend */
    }}
    
    .header-title {{
        flex: 1;
        font-size: 14px;
        font-weight: 600;
        text-transform: uppercase;
        font-stretch: 151%;
        letter-spacing: 0.15em;
        line-height: 24px;
        padding-top: 4px; /* Visual centering */
        color: var(--header-text);
        padding-left: 14px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        font-family: 'Google Sans Flex', sans-serif;
    }}
    
    .header-title span {{
        display: inline-block;
        transition: color 0.2s;
    }}

    @keyframes waveColor {{
        0%, 100% {{
            color: var(--header-text);
            font-variation-settings: 'GRAD' 0, 'wght' 600, 'ROND' 100;
        }}
        50% {{
            color: var(--wave-color);
            font-variation-settings: 'GRAD' 200, 'wght' 1000, 'ROND' 100;
        }}
    }}
    
    .close-btn {{
        width: 32px;
        height: 32px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 50%;
        cursor: pointer;
        color: var(--header-text);
        transition: background 0.1s;
        margin-right: 6px;
    }}

    .close-btn svg {{
        width: 20px;
        height: 20px;
        fill: currentColor;
    }}
    
    .mic-btn svg, .send-btn svg {{
        width: 22px;
        height: 22px;
    }}
    
    .mic-btn svg {{ fill: var(--mic-fill); }}
    .send-btn svg {{ fill: var(--send-fill); }}
    
    .close-btn:hover {{
        background: var(--close-hover-bg);
    }}

    #editor {{
        flex: 1;
        width: 100%;
        margin: 0px 8px;
        background: var(--input-bg);
        border-radius: 22px; /* Ultra rounded pill look */
        padding: 12px 14px;
        padding-right: 68px; /* Space for mic + send buttons to prevent overlap */
        border: var(--input-border);
        outline: none;
        resize: none;
        font-family: 'Google Sans Flex', sans-serif;
        font-size: 15px;
        line-height: 1.55;
        color: var(--text-color);
        overflow-y: auto;
        user-select: text;
        width: calc(100% - 16px);
    }}
    
    #editor::placeholder {{
        color: var(--placeholder-color);
        opacity: 1;
    }}
    
    /* Footer */
    .footer {{
        height: 28px;
        background: transparent;
        /* No border for seamless blend */
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 11px;
        color: var(--footer-text);
        font-variation-settings: 'ROND' 100, 'slnt' -10;
        cursor: default;
    }}

    /* Floating Buttons */
    /* Floating Buttons - Vertical Stack */
    .btn-container {{
        position: absolute;
        bottom: 40px; /* Above footer */
        right: 20px;
        display: flex;
        flex-direction: column;
        gap: 12px;
        z-index: 100;
    }}

    .mic-btn, .send-btn {{
        width: 48px;
        height: 48px; /* Big buttons */
        border-radius: 50%;
        display: flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;
        background: var(--btn-bg);
        border: 1px solid var(--btn-border);
        box-shadow: 0 2px 8px rgba(0,0,0,0.1);
        transition: all 0.2s cubic-bezier(0.2, 0.0, 0.2, 1);
        backdrop-filter: blur(8px);
        -webkit-backdrop-filter: blur(8px);
    }}
    
    .mic-btn svg, .send-btn svg {{
        width: 28px; /* Bigger icons */
        height: 28px;
        transition: transform 0.2s, fill 0.2s;
    }}
    
    .mic-btn:active, .send-btn:active {{
        transform: scale(0.95);
    }}
    


    .mic-btn svg {{ fill: var(--mic-fill); }}
    .send-btn svg {{ fill: var(--send-fill); }}

    .mic-btn:hover {{
        background: var(--mic-hover-bg);
        border-color: var(--mic-fill);
    }}
    
    .send-btn:hover {{
        background: var(--send-hover-bg);
        border-color: var(--send-fill);
    }}
"#,
        vars = vars
    )
}

/// Generate HTML for the text input webview
fn get_editor_html(placeholder: &str, is_dark: bool) -> String {
    let css = get_editor_css(is_dark);
    let theme_attr = if is_dark {
        "data-theme=\"dark\""
    } else {
        "data-theme=\"light\""
    };
    let font_css = crate::overlay::html_components::font_manager::get_font_css();
    let escaped_placeholder = placeholder
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    // Locale text
    let (submit_txt, newline_txt, cancel_txt) = {
        let lang = crate::overlay::text_input::CFG_LANG.lock().unwrap().clone();
        let locale = crate::gui::locale::LocaleText::get(&lang);
        (
            locale.text_input_footer_submit.to_string(),
            locale.text_input_footer_newline.to_string(),
            locale.text_input_footer_cancel.to_string(),
        )
    };
    let cancel_hint = {
        let sub = crate::overlay::text_input::CFG_CANCEL.lock().unwrap();
        if sub.is_empty() {
            "Esc".to_string()
        } else {
            format!("Esc / {}", sub)
        }
    };
    let title_text = {
        let t = crate::overlay::text_input::CFG_TITLE.lock().unwrap();
        if t.is_empty() {
            let lang = crate::overlay::text_input::CFG_LANG.lock().unwrap().clone();
            let locale = crate::gui::locale::LocaleText::get(&lang);
            locale.text_input_placeholder.to_string()
        } else {
            t.clone()
        }
    };

    format!(
        r#"<!DOCTYPE html>
<html {theme_attr}>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>{font_css}</style>
    <style id="theme-style">{css}</style>
</head>
<body>
    <div class="editor-container">
        <div class="header" id="headerRegion">
            <span class="header-title" id="headerTitle">{title_text}</span>
            <div class="close-btn" id="closeBtn" title="Close">
                {close_svg}
            </div>
        </div>
        
        <textarea id="editor" placeholder="{placeholder}" autofocus></textarea>
        
        <div class="btn-container">
            <button class="mic-btn" id="micBtn" title="Speech to text">
                {mic_svg}
            </button>
            <button class="send-btn" id="sendBtn" title="Send">
                {send_svg}
            </button>
        </div>
        
        <div class="footer" id="footerRegion">
            {submit_txt}  |  {newline_txt}  |  {cancel_hint} {cancel_txt}
        </div>
    </div>
    <script>
        const container = document.querySelector('.editor-container');
        const editor = document.getElementById('editor');
        const closeBtn = document.getElementById('closeBtn');
        const micBtn = document.getElementById('micBtn');
        const sendBtn = document.getElementById('sendBtn');
        
        // Drag window logic - Entire container except interactive elements
        container.addEventListener('mousedown', (e) => {{
            const isInteractive = e.target.closest('#editor') || 
                                e.target.closest('.close-btn') || 
                                e.target.closest('.mic-btn') || 
                                e.target.closest('.send-btn');
            if (isInteractive) return;
            
            // Only left click
            if (e.button === 0) {{
                window.ipc.postMessage('drag_window');
            }}
        }});
        
        // Close button
        closeBtn.addEventListener('click', (e) => {{
            window.ipc.postMessage('close_window');
        }});
        
        window.onload = () => {{
            setTimeout(() => editor.focus(), 50);
        }};
        
        // ... keydown handles ...
        editor.addEventListener('keydown', (e) => {{
            if (e.key === 'Enter' && !e.shiftKey) {{
                e.preventDefault();
                const text = editor.value.trim();
                if (text) {{
                    window.ipc.postMessage('submit:' + text);
                }}
            }}
            
            if (e.key === 'Escape') {{
                e.preventDefault();
                window.ipc.postMessage('cancel');
            }}
            
            if (e.key === 'ArrowUp') {{
                const isSingleLine = !editor.value.includes('\n');
                if ((isSingleLine || editor.selectionStart === 0) && !e.shiftKey) {{
                    e.preventDefault();
                    window.ipc.postMessage('history_up:' + editor.value);
                }}
            }}

            if (e.key === 'ArrowDown') {{
                const isSingleLine = !editor.value.includes('\n');
                if ((isSingleLine || editor.selectionStart === editor.value.length) && !e.shiftKey) {{
                    e.preventDefault();
                    window.ipc.postMessage('history_down:' + editor.value);
                }}
            }}
        }});
        
        micBtn.addEventListener('click', (e) => {{
            e.preventDefault();
            window.ipc.postMessage('mic');
        }});
        
        sendBtn.addEventListener('click', (e) => {{
            e.preventDefault();
            const text = editor.value.trim();
            if (text) {{
                window.ipc.postMessage('submit:' + text);
            }}
        }});
        
        document.addEventListener('contextmenu', e => e.preventDefault());
        
        window.setEditorText = (text) => {{
            editor.value = text;
            editor.selectionStart = editor.selectionEnd = text.length;
            editor.focus();
        }};

        window.updateTheme = (isDark) => {{
            document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
        }};

        window.playEntry = () => {{
            const el = document.querySelector('.editor-container');
            if(el) {{
                el.classList.remove('exiting');
                el.classList.add('entering');
                
                // Trigger wave animation on title characters
                const title = document.getElementById('headerTitle');
                if (title && !title.hasAttribute('data-wrapped')) {{
                    const text = title.innerText;
                    title.innerHTML = text.split('').map((char, i) => 
                        `<span style="animation: waveColor 0.6s ease forwards ${{0.2 + i * 0.05}}s">${{char === ' ' ? '&nbsp;' : char}}</span>`
                    ).join('');
                    title.setAttribute('data-wrapped', 'true');
                }} else if (title) {{
                    // Re-trigger animation by removing/adding spans or class
                     const text = title.innerText; // Get raw text back from spans
                     title.innerHTML = text.split('').map((char, i) => 
                        `<span style="animation: waveColor 0.6s ease forwards ${{0.2 + i * 0.05}}s">${{char === ' ' ? '&nbsp;' : char}}</span>`
                    ).join('');
                }}
            }}
        }};

        window.playExit = () => {{
            const el = document.querySelector('.editor-container');
            if(el) {{
                el.classList.remove('entering');
                el.classList.add('exiting');
            }}
        }};

    </script>
</body>
</html>"#,
        theme_attr = theme_attr,
        font_css = font_css,
        css = css,
        title_text = title_text,
        placeholder = escaped_placeholder,
        submit_txt = submit_txt,
        newline_txt = newline_txt,
        cancel_hint = cancel_hint,
        cancel_txt = cancel_txt,
        close_svg = crate::overlay::html_components::icons::get_icon_svg("close"),
        mic_svg = crate::overlay::html_components::icons::get_icon_svg("mic"),
        send_svg = crate::overlay::html_components::icons::get_icon_svg("send")
    )
}

pub fn is_active() -> bool {
    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val == 0 {
        return false;
    }
    unsafe { IsWindowVisible(HWND(hwnd_val as *mut std::ffi::c_void)).as_bool() }
}

pub fn cancel_input() {
    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        unsafe {
            let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
            let _ = PostMessageW(Some(hwnd), WM_APP_HIDE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Set text content in the webview editor (for paste operations)
/// This is thread-safe and can be called from any thread
pub fn set_editor_text(text: &str) {
    // Store the text in the mutex
    *PENDING_TEXT.lock().unwrap() = Some(text.to_string());

    // Post message to the text input window to trigger the injection
    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd_val as *mut std::ffi::c_void)),
                WM_APP_SET_TEXT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Internal function to apply pending text (called on the window's thread)
/// Inserts text at the current cursor position instead of replacing all content
fn apply_pending_text() {
    let text = PENDING_TEXT.lock().unwrap().take();
    if let Some(text) = text {
        // Check if this is a history replacement (replace all) or insertion
        let (is_replace_all, actual_text) =
            if let Some(stripped) = text.strip_prefix("__REPLACE_ALL__") {
                (true, stripped.to_string())
            } else {
                (false, text)
            };

        let escaped = actual_text
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace("${", "\\${")
            .replace('\n', "\\n")
            .replace('\r', "");

        TEXT_INPUT_WEBVIEW.with(|webview| {
            if let Some(wv) = webview.borrow().as_ref() {
                let script = if is_replace_all {
                    // Replace all text (for history navigation)
                    format!(
                        r#"(function() {{
                            const editor = document.getElementById('editor');
                            const text = `{}`;
                            editor.value = text;
                            editor.selectionStart = editor.selectionEnd = text.length;
                            editor.focus();
                        }})();"#,
                        escaped
                    )
                } else {
                    // Insert at cursor position (for paste/transcription)
                    format!(
                        r#"(function() {{
                            const editor = document.getElementById('editor');
                            const start = editor.selectionStart;
                            const end = editor.selectionEnd;
                            const text = `{}`;
                            editor.value = editor.value.substring(0, start) + text + editor.value.substring(end);
                            editor.selectionStart = editor.selectionEnd = start + text.length;
                            editor.focus();
                        }})();"#,
                        escaped
                    )
                };
                let _ = wv.evaluate_script(&script);
            }
        });
    }
}

/// Clear the webview editor content and refocus (for continuous input mode)
pub fn clear_editor_text() {
    TEXT_INPUT_WEBVIEW.with(|webview| {
        if let Some(wv) = webview.borrow().as_ref() {
            let script = r#"document.getElementById('editor').value = ''; document.getElementById('editor').focus();"#;
            let _ = wv.evaluate_script(script);
        }
    });
}

/// Update the UI text (header) and trigger a repaint
pub fn update_ui_text(header_text: String) {
    *CFG_TITLE.lock().unwrap() = header_text.clone();
    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            let _ = SetWindowTextW(hwnd, &HSTRING::from(header_text));
            let _ = PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(1), LPARAM(0));
        }
    }
}

/// Bring the text input window to foreground and focus the editor
/// Call this after closing modal windows like the preset wheel
pub fn refocus_editor() {
    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
            use windows::Win32::UI::WindowsAndMessaging::{
                BringWindowToTop, SetForegroundWindow, SetTimer,
            };

            // Aggressive focus: try multiple methods
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));

            // Focus the webview editor immediately
            TEXT_INPUT_WEBVIEW.with(|webview| {
                if let Some(wv) = webview.borrow().as_ref() {
                    // First focus the WebView itself (native focus)
                    let _ = wv.focus();
                    // Then focus the textarea inside via JavaScript
                    let _ = wv.evaluate_script("document.getElementById('editor').focus();");
                }
            });

            // Schedule another focus attempt after 200ms via timer ID 3
            // This will be handled in WM_TIMER in the same thread
            let _ = SetTimer(Some(hwnd), 3, 200, None);
        }
    }
}

/// Get the current window rect of the text input window (if active)
pub fn get_window_rect() -> Option<RECT> {
    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(HWND(hwnd_val as *mut std::ffi::c_void), &mut rect).is_ok() {
                return Some(rect);
            }
        }
    }
    None
}

/// Start the persistent hidden window (called from main)
pub fn warmup() {
    // Thread-safe atomic check-and-set to prevent multiple warmup threads
    if IS_WARMED_UP.load(Ordering::SeqCst) {
        return;
    }
    if IS_WARMING_UP
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        internal_create_window_loop();
    });
}

pub fn show(
    prompt_guide: String,
    ui_language: String,
    cancel_hotkey_name: String,
    continuous_mode: bool,
    on_submit: impl Fn(String, HWND) + Send + 'static,
) {
    // Re-entrancy guard: if we are already in the process of showing/waiting, ignore subsequent calls
    // This prevents key-mashing from spawning multiple wait loops or confused states
    if IS_SHOWING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    // Ensure we clear the flag when we return
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            IS_SHOWING.store(false, Ordering::SeqCst);
        }
    }
    let _guard = Guard;

    // Clone lang for locale notification before moving/consuming it
    let lang_for_locale = ui_language.clone();

    // Update shared state FIRST so it's ready when window shows up
    *CFG_TITLE.lock().unwrap() = prompt_guide;
    *CFG_LANG.lock().unwrap() = ui_language;
    *CFG_CANCEL.lock().unwrap() = cancel_hotkey_name;
    *CFG_CONTINUOUS.lock().unwrap() = continuous_mode;
    *CFG_CALLBACK.lock().unwrap() = Some(Box::new(on_submit));

    *SUBMITTED_TEXT.lock().unwrap() = None;
    *SHOULD_CLOSE.lock().unwrap() = false;
    *SHOULD_CLEAR_ONLY.lock().unwrap() = false;

    // Check if warmed up
    if !IS_WARMED_UP.load(Ordering::SeqCst) {
        // Trigger warmup for recovery
        warmup();

        // Show localized message that feature is not ready yet
        let locale = LocaleText::get(&lang_for_locale);
        crate::overlay::auto_copy_badge::show_notification(locale.text_input_loading);

        // Blocking wait with message pump
        // We wait up to 5 seconds. If it fails, we simply return (preventing premature broken window)
        for _ in 0..500 {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));

            if IS_WARMED_UP.load(Ordering::SeqCst) {
                break;
            }
        }

        // If still not warmed up after wait, give up
        if !IS_WARMED_UP.load(Ordering::SeqCst) {
            return;
        }
    }

    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
        unsafe {
            // ALWAYS show logic (Toggle logic handled by caller if needed)
            // Fixes issue where dynamic prompt mode fails to appear if window state is desync
            let _ = PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}

fn internal_create_window_loop() {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextInputWry");

        REGISTER_INPUT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(input_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            // Use NULL brush to prevent white flashes/stripes on resize
            wc.hbrBackground = HBRUSH(GetStockObject(NULL_BRUSH).0);
            let _ = RegisterClassW(&wc);
        });

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let scale = {
            let dpi = unsafe { GetDpiForSystem() };
            dpi as f64 / 96.0
        };
        // Width scaling: matches 800px physical at 1.25 scale (Laptop preferred),
        // but creates ~580px physical at 1.0 scale (PC preferred, narrower than 640).
        let win_w = ((880.0 * scale) - 300.0).round() as i32;
        let win_h = (253.0 * scale).round() as i32;

        eprintln!(
            "[TextInput] Creating window: scale={:.2}, width={}, height={}",
            scale, win_w, win_h
        );

        let x = (screen_w - win_w) / 2;
        let y = (screen_h - win_h) / 2;

        // Start HIDDEN logic
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Text Input"),
            WS_POPUP, // Start invisible (not WS_VISIBLE)
            x,
            y,
            win_w,
            win_h,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        INPUT_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // Initialize use simple DwmExtendFrameIntoClientArea for full transparency
        // NO SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY) as it conflicts with Dwm
        // Use margins -1 to extend glass effect to entire window (fully transparent client area)
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // REMOVED GDI REGION CLIPPING
        // We now rely on HTML/CSS border-radius and transparent background

        // Create webview
        init_webview(hwnd, win_w, win_h);

        // Mark as warmed up and ready
        IS_WARMED_UP.store(true, Ordering::SeqCst);
        IS_WARMING_UP.store(false, Ordering::SeqCst); // Done warming up

        // Message Loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }

        // Cleanup on exit
        TEXT_INPUT_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = None;
        });
        INPUT_HWND.store(0, Ordering::SeqCst);
        IS_WARMED_UP.store(false, Ordering::SeqCst);
        IS_WARMING_UP.store(false, Ordering::SeqCst);
    }
}

unsafe fn init_webview(hwnd: HWND, w: i32, h: i32) {
    // Use exact window dimensions for the webview, no insets.
    // The CSS .editor-container handles the padding/border-radius/shadow.
    let webview_x = 0;
    let webview_y = 0;
    let webview_w = w;
    let webview_h = h;

    let is_dark = if let Ok(app) = crate::APP.lock() {
        match app.config.theme_mode {
            crate::config::ThemeMode::Dark => true,
            crate::config::ThemeMode::Light => false,
            crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
        }
    } else {
        true
    };

    let placeholder = "Ready...";
    let html = get_editor_html(placeholder, is_dark);
    let wrapper = HwndWrapper(hwnd);

    // Initialize shared WebContext if needed (uses same data dir as other modules)
    TEXT_INPUT_WEB_CONTEXT.with(|ctx| {
        if ctx.borrow().is_none() {
            let shared_data_dir = crate::overlay::get_shared_webview_data_dir();
            *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
        }
    });

    let result = TEXT_INPUT_WEB_CONTEXT.with(|ctx| {
        let mut ctx_ref = ctx.borrow_mut();
        let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
            WebViewBuilder::new_with_web_context(web_ctx)
        } else {
            WebViewBuilder::new()
        };
        let builder = builder.with_transparent(true);
        let builder = crate::overlay::html_components::font_manager::configure_webview(builder);

        // Store HTML in font server and get URL for same-origin font loading
        let page_url = crate::overlay::html_components::font_manager::store_html_page(html.clone())
            .unwrap_or_else(|| format!("data:text/html,{}", urlencoding::encode(&html)));

        builder
            .with_bounds(Rect {
                position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
                    webview_x, webview_y,
                )),
                size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                    webview_w as u32,
                    webview_h as u32,
                )),
            })
            .with_url(&page_url)
            .with_transparent(true)
            .with_ipc_handler(move |msg: wry::http::Request<String>| {
                let body = msg.body();
                if body.starts_with("submit:") {
                    let text = body.strip_prefix("submit:").unwrap_or("").to_string();
                    if !text.trim().is_empty() {
                        // Save to history before submitting
                        crate::overlay::input_history::add_to_history(&text);
                        *SUBMITTED_TEXT.lock().unwrap() = Some(text);
                        *SHOULD_CLOSE.lock().unwrap() = true;
                    }
                } else if body == "cancel" {
                    crate::overlay::input_history::reset_history_navigation();
                    *SHOULD_CLOSE.lock().unwrap() = true;
                } else if body.starts_with("history_up:") {
                    let current = body.strip_prefix("history_up:").unwrap_or("");
                    if let Some(text) = crate::overlay::input_history::navigate_history_up(current)
                    {
                        *PENDING_TEXT.lock().unwrap() = Some(format!("__REPLACE_ALL__{}", text));
                        let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
                        if hwnd_val != 0 {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd_val as *mut std::ffi::c_void)),
                                    WM_APP_SET_TEXT,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        }
                    }
                } else if body.starts_with("history_down:") {
                    let current = body.strip_prefix("history_down:").unwrap_or("");
                    if let Some(text) =
                        crate::overlay::input_history::navigate_history_down(current)
                    {
                        *PENDING_TEXT.lock().unwrap() = Some(format!("__REPLACE_ALL__{}", text));
                        let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
                        if hwnd_val != 0 {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd_val as *mut std::ffi::c_void)),
                                    WM_APP_SET_TEXT,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        }
                    }
                } else if body == "mic" {
                    // Trigger transcription preset
                    let transcribe_idx = {
                        let app = crate::APP.lock().unwrap();
                        app.config
                            .presets
                            .iter()
                            .position(|p| p.id == "preset_transcribe")
                    };

                    if let Some(preset_idx) = transcribe_idx {
                        std::thread::spawn(move || {
                            crate::overlay::recording::show_recording_overlay(preset_idx);
                        });
                    }
                } else if body == "drag_window" {
                    let hwnd_val = INPUT_HWND.load(Ordering::SeqCst);
                    if hwnd_val != 0 {
                        unsafe {
                            let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);
                            let _ = ReleaseCapture();
                            let _ = SendMessageW(
                                hwnd,
                                WM_NCLBUTTONDOWN,
                                Some(WPARAM(HTCAPTION as usize)),
                                Some(LPARAM(0)),
                            );
                        }
                    }
                } else if body == "close_window" {
                    cancel_input();
                }
            })
            .build_as_child(&wrapper)
    });

    if let Ok(webview) = result {
        TEXT_INPUT_WEBVIEW.with(|wv| {
            *wv.borrow_mut() = Some(webview);
        });
    }
}

unsafe extern "system" fn input_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // State variables for this window instance
    static mut FADE_ALPHA: i32 = 0;
    // IS_DRAGGING is no longer needed with native drag

    match msg {
        WM_APP_SHOW => {
            // Restore History Navigation State
            crate::overlay::input_history::reset_history_navigation();

            // Moved playEntry to end of block to run AFTER text updates

            // 1. Position Logic - Center on the monitor where the cursor is
            if wparam.0 != 1 {
                let mut cursor = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut cursor);
                    let hmonitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let _ = GetMonitorInfoW(hmonitor, &mut mi);

                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;

                    let monitor_w = mi.rcWork.right - mi.rcWork.left;
                    let monitor_h = mi.rcWork.bottom - mi.rcWork.top;

                    let x = mi.rcWork.left + (monitor_w - w) / 2;
                    let y = mi.rcWork.top + (monitor_h - h) / 2;

                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOP),
                        x,
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                }
            }

            // 2. Focus - Force window to foreground
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            // Force Webview focus immediately
            TEXT_INPUT_WEBVIEW.with(|webview| {
                if let Some(wv) = webview.borrow().as_ref() {
                    let _ = wv.focus();
                }
            });

            // 3. Dynamic Update (Theme + Locales)
            let is_dark = if let Ok(app) = crate::APP.lock() {
                match app.config.theme_mode {
                    crate::config::ThemeMode::Dark => true,
                    crate::config::ThemeMode::Light => false,
                    crate::config::ThemeMode::System => crate::gui::utils::is_system_in_dark_mode(),
                }
            } else {
                true
            };

            // Re-fetch locales to ensure they are current
            let (title, submit, newline, cancel, cancel_hint, placeholder) = {
                let lang = crate::overlay::text_input::CFG_LANG.lock().unwrap().clone();
                let locale = crate::gui::locale::LocaleText::get(&lang);
                let t = crate::overlay::text_input::CFG_TITLE
                    .lock()
                    .unwrap()
                    .clone();
                let title = if t.is_empty() {
                    let lang = crate::overlay::text_input::CFG_LANG.lock().unwrap().clone();
                    let locale = crate::gui::locale::LocaleText::get(&lang);
                    locale.text_input_placeholder.to_string()
                } else {
                    t
                };
                let hotkey = crate::overlay::text_input::CFG_CANCEL.lock().unwrap();
                let ch = if hotkey.is_empty() {
                    "Esc".to_string()
                } else {
                    format!("Esc / {}", hotkey)
                };
                (
                    title,
                    locale.text_input_footer_submit.to_string(),
                    locale.text_input_footer_newline.to_string(),
                    locale.text_input_footer_cancel.to_string(),
                    ch,
                    locale.text_input_placeholder.to_string(),
                )
            };

            // Update window title
            let _ = SetWindowTextW(hwnd, &HSTRING::from(&title));

            let css = get_editor_css(is_dark);
            let css_escaped = css.replace("`", "\\`");

            // Construct footer HTML
            let footer_html = format!("{}  |  {}  |  {} {}", submit, newline, cancel_hint, cancel);
            let placeholder_escaped = placeholder.replace("'", "\\'"); // rudimentary escape

            let script = format!(
                r#"
                if (document.getElementById('theme-style')) {{
                   document.getElementById('theme-style').innerHTML = `{}`;
                }}
                if (document.getElementById('headerTitle')) {{
                   document.getElementById('headerTitle').innerText = `{}`;
                }}
                if (document.getElementById('footerRegion')) {{
                   document.getElementById('footerRegion').innerHTML = `{}`;
                }}
                if (document.getElementById('editor')) {{
                   document.getElementById('editor').placeholder = '{}';
                }}
                document.documentElement.setAttribute('data-theme', '{}');
                // Force focus on editor 
                setTimeout(() => {{
                    const el = document.getElementById('editor');
                    if (el) {{
                        el.focus();     
                        el.select(); 
                        el.selectionStart = el.selectionEnd = el.value.length; 
                    }}
                }}, 10);
                "#,
                css_escaped,
                title,
                footer_html,
                placeholder_escaped,
                if is_dark { "dark" } else { "light" }
            );

            TEXT_INPUT_WEBVIEW.with(|webview| {
                if let Some(wv) = webview.borrow().as_ref() {
                    let _ = wv.evaluate_script(&script);
                    // NOW trigger animation, after text has been updated
                    let _ = wv.evaluate_script("playEntry();");
                }
            });

            // Reset state
            FADE_ALPHA = 0;

            // IPC check timer
            SetTimer(Some(hwnd), 2, 50, None);

            LRESULT(0)
        }

        WM_APP_SET_TEXT => {
            // Apply pending text from cross-thread call
            apply_pending_text();
            LRESULT(0)
        }

        WM_APP_HIDE => {
            // Trigger Fade Out Script & Delay Hide
            TEXT_INPUT_WEBVIEW.with(|webview| {
                if let Some(wv) = webview.borrow().as_ref() {
                    let _ = wv.evaluate_script("playExit();");
                }
            });
            // 150ms delay for animation (Timer ID 4)
            SetTimer(Some(hwnd), 4, 150, None);
            LRESULT(0)
        }

        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = KillTimer(Some(hwnd), 1);
            let _ = KillTimer(Some(hwnd), 2);
            let _ = KillTimer(Some(hwnd), 3);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        WM_ERASEBKGND => LRESULT(1),

        WM_SETFOCUS => {
            TEXT_INPUT_WEBVIEW.with(|webview| {
                if let Some(wv) = webview.borrow().as_ref() {
                    let _ = wv.focus();
                }
            });
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == 1 {
                // Fade Timer Logic removed
                let _ = KillTimer(Some(hwnd), 1);
            }

            if wparam.0 == 2 {
                // IPC messages
                let should_close = *SHOULD_CLOSE.lock().unwrap();
                if should_close {
                    *SHOULD_CLOSE.lock().unwrap() = false;
                    let submitted = SUBMITTED_TEXT.lock().unwrap().take();
                    if let Some(text) = submitted {
                        let continuous = *CFG_CONTINUOUS.lock().unwrap();
                        if continuous {
                            let cb_lock = CFG_CALLBACK.lock().unwrap();
                            if let Some(cb) = cb_lock.as_ref() {
                                cb(text, hwnd);
                            }
                            clear_editor_text();
                            refocus_editor();
                        } else {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            let cb_lock = CFG_CALLBACK.lock().unwrap();
                            if let Some(cb) = cb_lock.as_ref() {
                                cb(text, hwnd);
                            }
                        }
                    } else {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
            // Timer 3: focus logic (used by refocus_editor after preset wheel)
            if wparam.0 == 3 {
                let _ = KillTimer(Some(hwnd), 3);
                TEXT_INPUT_WEBVIEW.with(|webview| {
                    if let Some(wv) = webview.borrow().as_ref() {
                        let _ = wv.focus();
                        let _ = wv.evaluate_script("document.getElementById('editor').focus();");
                    }
                });
            }
            // Timer 4: Hide window after fade-out
            if wparam.0 == 4 {
                let _ = KillTimer(Some(hwnd), 4);
                let _ = ShowWindow(hwnd, SW_HIDE);

                // Signal closure if needed
                let mut should_close = SHOULD_CLOSE.lock().unwrap();
                if *should_close {
                    *should_close = false;
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let w = rect.right;

            // Close Button
            let close_x = w - 30;
            let close_y = 20;
            if (x - close_x).abs() < 15 && (y - close_y).abs() < 15 {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }

            // Title Bar Drag - Use Native Drag (Fix drifting issues)
            if y < 50 {
                let _ = ReleaseCapture();
                SendMessageW(hwnd, WM_SYSCOMMAND, Some(WPARAM(0xF012)), Some(LPARAM(0)));
                return LRESULT(0);
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            // WM_MOUSEMOVE drag logic removed in favor of native drag
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            // No capture cleanup needed for native drag
            LRESULT(0)
        }

        WM_SIZE => {
            // Resize WebView to match the new client area
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;

            if width > 0 && height > 0 {
                TEXT_INPUT_WEBVIEW.with(|wv| {
                    if let Some(webview) = wv.borrow().as_ref() {
                        let _ = webview.set_bounds(Rect {
                            position: wry::dpi::Position::Physical(
                                wry::dpi::PhysicalPosition::new(0, 0),
                            ),
                            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                                width as u32,
                                height as u32,
                            )),
                        });
                    }
                });
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
