// Button Canvas - Floating transparent WebView overlay for markdown result buttons
// Single fullscreen canvas that serves buttons for ALL markdown result windows
// Click-through background with radius-based opacity buttons

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Mutex,
};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::*;
use wry::{Rect, WebContext, WebView, WebViewBuilder};

use super::state::WINDOW_STATES;

/// Get DPI scale factor (1.0 = 100%, 1.5 = 150%, 2.0 = 200%, etc.)
fn get_dpi_scale() -> f64 {
    let dpi = unsafe { GetDpiForSystem() };
    dpi as f64 / 96.0
}

// Singleton canvas state
static CANVAS_HWND: AtomicIsize = AtomicIsize::new(0);
static IS_WARMED_UP: AtomicBool = AtomicBool::new(false);
static IS_DRAGGING_EXTERNAL: AtomicBool = AtomicBool::new(false); // New flag
static REGISTER_CANVAS_CLASS: std::sync::Once = std::sync::Once::new();

// Custom messages
const WM_APP_UPDATE_WINDOWS: u32 = WM_APP + 50;
const WM_APP_SHOW_CANVAS: u32 = WM_APP + 51;
const WM_APP_HIDE_CANVAS: u32 = WM_APP + 52;

// Timer for cursor position polling (since WS_EX_TRANSPARENT prevents mouse events)
const CURSOR_POLL_TIMER_ID: usize = 1;

thread_local! {
    static CANVAS_WEBVIEW: RefCell<Option<WebView>> = RefCell::new(None);
    static CANVAS_WEB_CONTEXT: RefCell<Option<WebContext>> = RefCell::new(None);
}

lazy_static::lazy_static! {


    // Tracks which result windows are in markdown mode and their positions
    // Key: hwnd as isize, Value: (x, y, w, h)
    static ref MARKDOWN_WINDOWS: Mutex<HashMap<isize, (i32, i32, i32, i32)>> = Mutex::new(HashMap::new());
}

// HWND wrapper for wry
struct HwndWrapper(HWND);
unsafe impl Send for HwndWrapper {}
unsafe impl Sync for HwndWrapper {}
impl raw_window_handle::HasWindowHandle for HwndWrapper {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::Win32WindowHandle::new(
            std::num::NonZeroIsize::new(self.0 .0 as isize).expect("HWND cannot be null"),
        );
        let handle = raw_window_handle::RawWindowHandle::Win32(raw);
        unsafe { Ok(raw_window_handle::WindowHandle::borrow_raw(handle)) }
    }
}

static IS_INITIALIZING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Warmup removed as per user request
pub fn warmup() {}

/// Register a markdown window for button overlay
pub fn register_markdown_window(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    // Initialize on-demand if not warmed up
    if !IS_WARMED_UP.load(Ordering::SeqCst) && CANVAS_HWND.load(Ordering::SeqCst) == 0 {
        if !IS_INITIALIZING.swap(true, Ordering::SeqCst) {
            std::thread::spawn(|| {
                create_canvas_window();
            });
        }

        // Polling thread to auto-show once ready
        std::thread::spawn(move || {
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if IS_WARMED_UP.load(Ordering::SeqCst) && CANVAS_HWND.load(Ordering::SeqCst) != 0 {
                    update_canvas();
                    show_canvas();
                    return;
                }
            }
        });
    }

    // Get window rect
    let rect = unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        r
    };

    {
        let mut windows = MARKDOWN_WINDOWS.lock().unwrap();
        windows.insert(
            hwnd_key,
            (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ),
        );
    }

    // Trigger canvas update
    update_canvas();
    show_canvas();
}

/// Unregister a markdown window
pub fn unregister_markdown_window(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

    {
        let mut windows = MARKDOWN_WINDOWS.lock().unwrap();
        windows.remove(&hwnd_key);

        // If no more markdown windows, hide canvas
        if windows.is_empty() {
            hide_canvas();
        }
    }

    update_canvas();
}

/// Update window position (call when window moves/resizes)
pub fn update_window_position(hwnd: HWND) {
    update_window_position_internal(hwnd, true);
}

/// Update window position without triggering canvas repaint (for high-frequency drag)
pub fn update_window_position_silent(hwnd: HWND) {
    update_window_position_internal(hwnd, false);
}

fn update_window_position_internal(hwnd: HWND, notify: bool) {
    let hwnd_key = hwnd.0 as isize;

    let rect = unsafe {
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        r
    };

    {
        let mut windows = MARKDOWN_WINDOWS.lock().unwrap();
        if windows.contains_key(&hwnd_key) {
            windows.insert(
                hwnd_key,
                (
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                ),
            );
        }
    }

    if notify {
        update_canvas();
        update_canvas();
    }
}

/// Set drag mode (temporarily disable region clipping to prevent UI cutoff)
pub fn set_drag_mode(active: bool) {
    let canvas_hwnd = CANVAS_HWND.load(Ordering::SeqCst);
    if canvas_hwnd == 0 {
        return;
    }
    let hwnd = HWND(canvas_hwnd as *mut std::ffi::c_void);

    if active {
        // ENTER DRAG MODE: Remove region (full window visible/interactive)
        IS_DRAGGING_EXTERNAL.store(true, Ordering::SeqCst);
        unsafe {
            let _ = SetWindowRgn(hwnd, None, true);
        }
    } else {
        // EXIT DRAG MODE: Restore regions
        IS_DRAGGING_EXTERNAL.store(false, Ordering::SeqCst);
        update_canvas(); // Trigger recalculation of regions
    }
}

/// Update canvas with current window positions
pub fn update_canvas() {
    let canvas_hwnd = CANVAS_HWND.load(Ordering::SeqCst);
    if canvas_hwnd != 0 {
        let hwnd = HWND(canvas_hwnd as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_UPDATE_WINDOWS, WPARAM(0), LPARAM(0));
        }
    }
}

/// Show the canvas
fn show_canvas() {
    let canvas_hwnd = CANVAS_HWND.load(Ordering::SeqCst);
    if canvas_hwnd != 0 {
        let hwnd = HWND(canvas_hwnd as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_SHOW_CANVAS, WPARAM(0), LPARAM(0));
        }
    }
}

/// Hide the canvas
fn hide_canvas() {
    let canvas_hwnd = CANVAS_HWND.load(Ordering::SeqCst);
    if canvas_hwnd != 0 {
        let hwnd = HWND(canvas_hwnd as *mut std::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_HIDE_CANVAS, WPARAM(0), LPARAM(0));
        }
    }
}

// Track last applied theme to avoid redundant injections
static LAST_THEME_IS_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn get_canvas_theme_css(is_dark: bool) -> &'static str {
    if is_dark {
        r#"
        :root {
            --btn-bg: rgba(30, 30, 30, 0.85);
            --btn-border: rgba(255, 255, 255, 0.1);
            --btn-color: rgba(255, 255, 255, 0.8);
            --btn-hover-bg: rgba(60, 60, 60, 0.95);
            --btn-hover-color: #4fc3f7;
            --btn-active-bg: rgba(30, 30, 30, 0.95);
            --btn-active-color: #4fc3f7;
            --btn-success-color: #81c784;
            --shadow-color: rgba(79, 195, 247, 0.35);
        }
        "#
    } else {
        r#"
        :root {
            --btn-bg: rgba(255, 255, 255, 0.92);
            --btn-border: rgba(0, 0, 0, 0.08);
            --btn-color: rgba(0, 0, 0, 0.7);
            --btn-hover-bg: #ffffff;
            --btn-hover-color: #0277bd;
            --btn-active-bg: #ffffff;
            --btn-active-color: #0277bd;
            --btn-success-color: #43a047;
            --shadow-color: rgba(2, 119, 189, 0.25);
        }
        "#
    }
}

fn generate_canvas_html() -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    // Get localization
    let lang = crate::APP.lock().unwrap().config.ui_language.clone();
    let locale = crate::gui::locale::LocaleText::get(&lang);
    let l10n_json = serde_json::json!({
        "copy": locale.overlay_copy_tooltip,
        "undo": locale.overlay_undo_tooltip,
        "redo": locale.overlay_redo_tooltip,
        "edit": locale.overlay_edit_tooltip,
        "markdown": locale.overlay_markdown_tooltip,
        "download": locale.overlay_download_tooltip,
        "speaker": locale.overlay_speaker_tooltip,
        "broom": locale.overlay_broom_tooltip,
        "back": locale.overlay_back_tooltip,
        "forward": locale.overlay_forward_tooltip,
    })
    .to_string();

    let is_dark = crate::overlay::is_dark_mode();

    // Icon color based on theme - REMOVED to allow CSS currentColor to work
    // let icon_color = if is_dark {
    //     "rgba(255, 255, 255, 0.8)"
    // } else {
    //     "rgba(0, 0, 0, 0.7)"
    // };

    // Initialize state
    LAST_THEME_IS_DARK.store(is_dark, Ordering::SeqCst);
    let theme_css = get_canvas_theme_css(is_dark);

    // Get icon SVGs with theme-appropriate colors
    // CHANGED: Do NOT replace currentColor. This allows CSS to control icon color (hover, active, etc.)
    let get_colored_svg = |name: &str| -> String {
        crate::overlay::html_components::icons::get_icon_svg(name).to_string()
    };

    let icon_svgs_json = serde_json::json!({
        "arrow_back": get_colored_svg("arrow_back"),
        "arrow_forward": get_colored_svg("arrow_forward"),
        "undo": get_colored_svg("undo"),
        "redo": get_colored_svg("redo"),
        "newsmode": get_colored_svg("newsmode"),
        "notes": get_colored_svg("notes"),
        "hourglass_empty": get_colored_svg("hourglass_empty"),
        "stop": get_colored_svg("stop"),
        "cleaning_services": get_colored_svg("cleaning_services"),
        "content_copy": get_colored_svg("content_copy"),
        "check": get_colored_svg("check"),
        "download": get_colored_svg("download"),
        "volume_up": get_colored_svg("volume_up"),
    })
    .to_string();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<style>
{font_css}
</style>
<style id="theme-css">
{theme_css}
</style>
<style>

.icons {{
    font-family: 'Material Symbols Rounded';
    font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 20;
    font-size: 16px;
    line-height: 1;
}}

* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: transparent;
    pointer-events: none; /* Click-through by default */
    font-family: 'Google Sans Flex', 'Segoe UI', sans-serif;
    user-select: none;
}}

.button-group {{
    position: absolute;
    display: flex;
    gap: 4px;
    padding: 2px;
    pointer-events: auto; /* Buttons accept clicks */
    transition: opacity 0.15s ease-out;
}}

.btn {{
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: var(--btn-bg);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border: 1px solid var(--btn-border);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: opacity 0.15s ease-out, background-color 0.15s ease-out, color 0.15s ease-out;
    color: var(--btn-color);
}}

.button-group.vertical {{
    flex-direction: column;
    padding: 6px 3px;
    height: auto;
    width: 32px;
}}
.button-group.vertical .btn {{
    margin: 3px 0;
}}

.btn:hover {{
    background: var(--btn-hover-bg);
    color: var(--btn-hover-color);
    transform: scale(1.05);
    /* Directional glow: left, right, bottom only (no top to avoid resize area) */
    box-shadow: 
        -5px 0 6px -3px var(--shadow-color),  /* left */
        5px 0 6px -3px var(--shadow-color),   /* right */
        0 5px 6px -3px var(--shadow-color);    /* bottom */
}}

.btn:active {{
    transform: scale(0.95);
}}

.btn.disabled {{
    opacity: 0.3;
    pointer-events: none;
}}

.btn.active {{
    background: var(--btn-active-bg);
    border-color: var(--btn-active-color);
    color: var(--btn-active-color);
}}

.btn.success {{
    background: var(--btn-active-bg);
    border-color: var(--btn-success-color);
    color: var(--btn-success-color);
}}

.btn.loading {{
    animation: pulse 1s infinite;
}}

@keyframes pulse {{
    0%, 100% {{ opacity: 1; }}
    50% {{ opacity: 0.5; }}
}}

/* Broom button special styling */
.btn.broom {{
    cursor: grab;
}}
.btn.broom:active {{
    cursor: grabbing;
}}

/* Hidden button - invisible but preserves space */
.btn.hidden {{
    visibility: hidden;
    pointer-events: none;
}}
</style>
</head>
<body>
<div id="button-container"></div>
<script>
// Track registered windows: {{ hwnd: {{ x, y, w, h }} }}
window.registeredWindows = {{}};
window.L10N = #L10N_JSON#;
window.iconSvgs = #ICON_SVGS_JSON#;

// Track visibility state to minimize IPC calls
// Key: hwnd string, Value: boolean (isVisible)
let lastVisibleState = new Map();
// Key: hwnd string, Value: string (json of x,y,w,h)
let lastSentRegions = new Map();


// Track cursor position for radius-based opacity
// Note: Position is updated from Rust via updateCursorPosition() since WS_EX_TRANSPARENT
// prevents this window from receiving mouse events directly
let cursorX = 0, cursorY = 0;
// Track drag state globally so opacity updates can sync regions during drag
let broomDragData = null;

// Called from Rust every 50ms with current cursor position
window.updateCursorPosition = (x, y) => {{
    cursorX = x;
    cursorY = y;
    updateButtonOpacity();
}};

// Update button opacity based on distance from cursor to nearest edge of button group
function updateButtonOpacity() {{
    const groups = document.querySelectorAll('.button-group');
    // Force update during drag to ensure clipping region follows the buttons
    let needsUpdate = (broomDragData && broomDragData.moved) || false;
    
    groups.forEach(group => {{
        const rect = group.getBoundingClientRect();
        
        // Calculate distance to nearest edge of the rectangle (not center)
        let dx = 0, dy = 0;
        if (cursorX < rect.left) dx = rect.left - cursorX;
        else if (cursorX > rect.right) dx = cursorX - rect.right;
        
        if (cursorY < rect.top) dy = rect.top - cursorY;
        else if (cursorY > rect.bottom) dy = cursorY - rect.bottom;
        
        // If cursor is inside the rect, distance is 0
        const dist = Math.sqrt(dx * dx + dy * dy);
        
        // Radius-based opacity: full at 0, fade to 0 at 150px from edge
        const maxRadius = 150;
        let opacity = Math.max(0, Math.min(1, 1 - (dist / maxRadius)));
        
        // Force full opacity if dragging this specific window
        if (broomDragData && broomDragData.moved && broomDragData.hwnd === group.dataset.hwnd) {{
            opacity = 1.0;
        }}

        group.style.opacity = opacity;
        
        const isVisible = opacity > 0.1;
        group.style.pointerEvents = isVisible ? 'auto' : 'none';
        
        const hwnd = group.dataset.hwnd;
        if (lastVisibleState.get(hwnd) !== isVisible) {{
            lastVisibleState.set(hwnd, isVisible);
            needsUpdate = true;
        }}

        // Check if region dimensions changed (even if visibility didn't)
        if (isVisible) {{
            const currentRegion = {{
                x: Math.round(rect.left),
                y: Math.round(rect.top),
                w: Math.round(rect.width),
                h: Math.round(rect.height)
            }};
            const regionStr = JSON.stringify(currentRegion);
            if (lastSentRegions.get(hwnd) !== regionStr) {{
                // DON'T update map here, wait until we actually confirm sending
                needsUpdate = true;
            }}
        }}
    }});
    
    if (needsUpdate) {{
        // Send updated clickable regions to Rust
        // Only include regions that are currently visible
        const regions = [];
        const padding = 5; // Reduced 20 -> 5 to prevent resize handle overlap
        
        groups.forEach(group => {{
            if (lastVisibleState.get(group.dataset.hwnd)) {{
                const rect = group.getBoundingClientRect();
                const region = {{
                    x: rect.left - padding,
                    y: rect.top + 1, // Start slightly below top edge to ensure we don't cover window bottom border
                    w: rect.width + (padding * 2),
                    h: rect.height + padding // Only pad bottom
                }};
                regions.push(region);
                
                // Track what we sent (using the RAW rect, not the padded one, for change detection stability)
                // Actually, let's track the RAW rect matching the change detection above
                const rawRegion = {{
                    x: Math.round(rect.left),
                    y: Math.round(rect.top),
                    w: Math.round(rect.width),
                    h: Math.round(rect.height)
                }};
                lastSentRegions.set(group.dataset.hwnd, JSON.stringify(rawRegion));
            }}
        }});
        
        window.ipc.postMessage(JSON.stringify({{
            action: "update_clickable_regions",
            regions: regions
        }}));
    }}
}}

// Calculate best position for button group based on window position and screen bounds
// Calculate best position for button group based on window position and screen bounds
// Calculate best position for button group based on window position and screen bounds
function calculateButtonPosition(winRect) {{
    const screenW = window.innerWidth;
    const screenH = window.innerHeight;
    const longDim = 300; // Length of the button group (reduced for compact UI)
    const shortDim = 32;  // Thickness of the button group (24px btn + padding)
    const margin = 4; // Gap of 4px to match button spacing
    
    // Check available space on each side (thickness-wise)
    const spaceBottom = screenH - (winRect.y + winRect.h);
    const spaceTop = winRect.y;
    const spaceRight = screenW - (winRect.x + winRect.w);
    const spaceLeft = winRect.x;
    
    // Helper to clamp position to keep bar on screen
    const clamp = (val, max) => Math.max(0, Math.min(val, max));

    // 1. Bottom Horizontal (Preferred) - Check if we have vertical space below
    if (spaceBottom >= shortDim + margin) {{
        // Right align relative to window (flush right), clamp to screen width
        let x = winRect.x + winRect.w - longDim;
        x = clamp(x, screenW - longDim);
        return {{
            x: x,
            y: winRect.y + winRect.h + margin,
            direction: 'bottom'
        }};
    }}
    // 2. Right Vertical - Check if we have horizontal space to the right
    else if (spaceRight >= shortDim + margin) {{
        // Center vertically relative to window, clamp to screen height
        let y = winRect.y + (winRect.h - longDim) / 2;
        y = clamp(y, screenH - longDim);
        return {{
            x: winRect.x + winRect.w + margin,
            y: y,
            direction: 'right'
        }};
    }}
    // 3. Left Vertical
    else if (spaceLeft >= shortDim + margin) {{
        let y = winRect.y + (winRect.h - longDim) / 2;
        y = clamp(y, screenH - longDim);
        return {{
            x: winRect.x - shortDim - margin,
            y: y,
            direction: 'left'
        }};
    }}
    // 4. Top Horizontal
    else if (spaceTop >= shortDim + margin) {{
        let x = winRect.x + (winRect.w - longDim) / 2;
        x = clamp(x, screenW - longDim);
        return {{
            x: x,
            y: winRect.y - shortDim - margin,
            direction: 'top'
        }};
    }}
    // Fallback: overlay inside window at bottom (clamped)
    else {{
        let x = winRect.x + (winRect.w - longDim) / 2;
        x = clamp(x, screenW - longDim);
        
        let y = winRect.y + winRect.h - shortDim - margin;
        // Ensure it doesn't go off top if window is tiny
        y = Math.max(winRect.y, y); 
        
        return {{
            x: x,
            y: y,
            direction: 'inside'
        }};
    }}
}}

// Generate buttons HTML for a window
// All buttons are always rendered but hidden when not applicable to preserve consistent layout
function generateButtonsHTML(hwnd, state) {{
    const canGoBack = state.navDepth > 0;
    const canGoForward = state.navDepth < state.maxNavDepth;
    const isBrowsing = state.isBrowsing || false;
    const hideClass = isBrowsing ? 'hidden' : '';
    
    let buttons = '';
    
    // Back button - always rendered, hidden when not available
    const backHideClass = canGoBack ? '' : 'hidden';
    buttons += `<div class="btn ${{backHideClass}}" onclick="action('${{hwnd}}', 'back')" title="${{window.L10N.back}}">
        ${{window.iconSvgs.arrow_back}}
    </div>`;

    // Forward button - always rendered, hidden when not available
    const forwardHideClass = canGoForward ? '' : 'hidden';
    buttons += `<div class="btn ${{forwardHideClass}}" onclick="action('${{hwnd}}', 'forward')" title="${{window.L10N.forward}}">
        ${{window.iconSvgs.arrow_forward}}
    </div>`;
    
    // Copy - hidden when browsing but preserves space
    buttons += `<div class="btn ${{state.copySuccess ? 'success' : ''}} ${{hideClass}}" onclick="action('${{hwnd}}', 'copy')" title="${{window.L10N.copy}}">
        ${{window.iconSvgs[state.copySuccess ? 'check' : 'content_copy']}}
    </div>`;
    
    // Undo - only shown if there's history (hidden when browsing)
    if (state.hasUndo) {{
        buttons += `<div class="btn ${{hideClass}}" onclick="action('${{hwnd}}', 'undo')" title="${{window.L10N.undo}}">
            ${{window.iconSvgs.undo}}
        </div>`;
    }}

    // Redo - only shown if there's redo history (hidden when browsing)
    if (state.hasRedo) {{
        buttons += `<div class="btn ${{hideClass}}" onclick="action('${{hwnd}}', 'redo')" title="${{window.L10N.redo}}">
            ${{window.iconSvgs.redo}}
        </div>`;
    }}
    
    // Edit/Refine - hidden when browsing but preserves space
    buttons += `<div class="btn ${{hideClass}}" onclick="action('${{hwnd}}', 'edit')" title="${{window.L10N.edit}}">
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 258" width="14" height="14" style="fill: currentColor; stroke: currentColor; stroke-width: 20; stroke-linejoin: round; opacity: 0.9;">
            <path d="m122.062 172.77l-10.27 23.52c-3.947 9.042-16.459 9.042-20.406 0l-10.27-23.52c-9.14-20.933-25.59-37.595-46.108-46.703L6.74 113.52c-8.987-3.99-8.987-17.064 0-21.053l27.385-12.156C55.172 70.97 71.917 53.69 80.9 32.043L91.303 6.977c3.86-9.303 16.712-9.303 20.573 0l10.403 25.066c8.983 21.646 25.728 38.926 46.775 48.268l27.384 12.156c8.987 3.99 8.987 17.063 0 21.053l-28.267 12.547c-20.52 9.108-36.97 25.77-46.109 46.703"/>
            <path d="m217.5 246.937l-2.888 6.62c-2.114 4.845-8.824 4.845-10.937 0l-2.889-6.62c-5.148-11.803-14.42-21.2-25.992-26.34l-8.898-3.954c-4.811-2.137-4.811-9.131 0-11.269l8.4-3.733c11.87-5.273 21.308-15.017 26.368-27.22l2.966-7.154c2.067-4.985 8.96-4.985 11.027 0l2.966 7.153c5.06 12.204 14.499 21.948 26.368 27.221l8.4 3.733c4.812 2.138 4.812 9.132 0 11.27l-8.898 3.953c-11.571 5.14-20.844 14.537-25.992 26.34"/>
        </svg>
    </div>`;
    
    // Markdown toggle - hidden when browsing but preserves space
    const mdClass = state.isMarkdown ? 'active' : '';
    const mdIcon = state.isMarkdown ? 'newsmode' : 'notes';
    buttons += `<div class="btn ${{mdClass}} ${{hideClass}}" onclick="action('${{hwnd}}', 'markdown')" title="${{window.L10N.markdown}}">
        ${{window.iconSvgs[mdIcon]}}
    </div>`;
    
    // Download - hidden when browsing but preserves space
    buttons += `<div class="btn ${{hideClass}}" onclick="action('${{hwnd}}', 'download')" title="${{window.L10N.download}}">
        ${{window.iconSvgs.download}}
    </div>`;
    
    // Speaker/TTS - hidden when browsing but preserves space
    const speakerIcon = state.ttsLoading ? 'hourglass_empty' : (state.ttsSpeaking ? 'stop' : 'volume_up');
    const speakerClass = state.ttsLoading ? 'loading' : (state.ttsSpeaking ? 'active' : '');
    buttons += `<div class="btn ${{speakerClass}} ${{hideClass}}" onclick="action('${{hwnd}}', 'speaker')" title="${{window.L10N.speaker}}">
        ${{window.iconSvgs[speakerIcon]}}
    </div>`;
    
    // Broom (close/drag) - always visible
    buttons += `<div class="btn broom"
        onclick="action('${{hwnd}}', 'broom_click')"
        oncontextmenu="action('${{hwnd}}', 'broom_right'); return false;"
        onmousedown="handleBroomDrag(event, '${{hwnd}}')"
        onauxclick="if(event.button===1) action('${{hwnd}}', 'broom_middle')"
        title="${{window.L10N.broom}}">
        ${{window.iconSvgs.cleaning_services}}
    </div>`;
    
    return buttons;
}}

// Handle broom drag
// Handle broom drag - NATIVE IMPLEMENTATION
function handleBroomDrag(e, hwnd) {{
    if (e.button !== 0 && e.button !== 2) return; // Allow left (0) or right (2) click
    
    // 1. Hide the button group immediately (it will reappear after drag via updateWindows)
    const group = document.querySelector('.button-group[data-hwnd="' + hwnd + '"]');
    if (group) {{
        group.style.opacity = '0';
        group.style.pointerEvents = 'none';
        
        // Also update local state so it doesn't flicker back on if a cursor poll happens
        lastVisibleState.set(hwnd, false);
    }}

    // 2. Trigger native drag immediately
    const action = e.button === 2 ? 'broom_group_drag_start' : 'broom_drag_start';
    window.ipc.postMessage(JSON.stringify({{
        action: action,
        hwnd: hwnd
    }}));
    
    // No need for mousemove logic/throttling anymore - the OS handles it!
}}

// Send action to Rust
function action(hwnd, cmd) {{
    // If it's a broom click and we just dragged, ignore it
    if (cmd === 'broom_click' && window.ignoreNextBroomClick) return;
    window.ipc.postMessage(JSON.stringify({{ action: cmd, hwnd: hwnd }}));
}}

// Update all button groups
function updateWindows(windowsData) {{
    window.registeredWindows = windowsData;
    
    const container = document.getElementById('button-container');
    const screenW = window.innerWidth;
    const screenH = window.innerHeight;
    
    // Diffing logic
    const existingGroups = new Map();
    container.querySelectorAll('.button-group').forEach(el => {{
        existingGroups.set(el.dataset.hwnd, el);
    }});
    
    for (const [hwnd, data] of Object.entries(windowsData)) {{
        // Pass 1: Estimate position
        let pos = calculateButtonPosition(data.rect);
        let group = existingGroups.get(hwnd);
        
        if (!group) {{
            group = document.createElement('div');
            group.className = 'button-group';
            group.style.opacity = '0'; // Start hidden
            group.dataset.hwnd = hwnd;
            container.appendChild(group);
        }} else {{
            existingGroups.delete(hwnd); // Mark as kept
        }}
        
        // Update content first to ensure correct dimensions for Pass 2
        const newStateStr = JSON.stringify(data.state || {{}});
        if (group.dataset.lastState !== newStateStr) {{
            group.innerHTML = generateButtonsHTML(hwnd, data.state || {{}});
            group.dataset.lastState = newStateStr;
        }}

        // Apply estimated class to get approximate dimensions
        if (pos.direction === 'left' || pos.direction === 'right') {{
            group.classList.add('vertical');
        }} else {{
            group.classList.remove('vertical');
        }}

        // Pass 2: Measure and Correct
        // Now that content and class are set, read actual dimensions
        const actualW = group.offsetWidth || (pos.direction === 'left' || pos.direction === 'right' ? 50 : 400);
        const actualH = group.offsetHeight || (pos.direction === 'left' || pos.direction === 'right' ? 400 : 50);

        // Re-clamp position based on actual dimensions
        // calculateButtonPosition returns a centered position, but we need to ensure it's on screen
        // We can just clamp the estimated 'pos' using actual dimensions
        
        // Helper to clamp
        const clamp = (val, size, max) => Math.max(0, Math.min(val, max - size));

        let finalX = pos.x;
        let finalY = pos.y;

        // Recalculate centering if dimensions differ significantly? 
        // calculateButtonPosition used hardcoded 400/50. 
        // If actual is 600, centering based on 400 is wrong.
        // Let's re-run the relevant centering logic with actual dims
        
        if (pos.direction === 'bottom') {{
            // Right align relative to window
            finalX = data.rect.x + data.rect.w - actualW;
            finalY = data.rect.y + data.rect.h + 4; // margin 4
        }} else if (pos.direction === 'top') {{
            finalX = data.rect.x + (data.rect.w - actualW) / 2;
            finalY = data.rect.y - actualH - 4; // margin 4 (gap)
        }} else if (pos.direction === 'right') {{
            finalX = data.rect.x + data.rect.w + 4; // margin 4
            finalY = data.rect.y + (data.rect.h - actualH) / 2;
        }} else if (pos.direction === 'left') {{
            finalX = data.rect.x - actualW - 4; // margin 4
            finalY = data.rect.y + (data.rect.h - actualH) / 2;
        }} else {{ // inside
            finalX = data.rect.x + 8;
            finalY = data.rect.y + data.rect.h - actualH - 8;
             // Ensure it doesn't go off top if window is tiny
            finalY = Math.max(data.rect.y, finalY);
        }}

        // Final screen clamping
        finalX = clamp(finalX, actualW, screenW);
        finalY = clamp(finalY, actualH, screenH);
        
        // CRITICAL: Do NOT update position if we are currently dragging this window!
        // The local drag handler owns the position to ensure smoothness.
        // Overwriting it with potentially stale Rust data causes stutter/glitching.
        if (!broomDragData || broomDragData.hwnd !== hwnd) {{
            group.style.left = finalX + 'px';
            group.style.top = finalY + 'px';
        }}
    }}
    
    // Remove stale
    existingGroups.forEach((el, key) => {{
        el.remove();
        lastVisibleState.delete(key);
    }});

    // CRITICAL: Update regions immediately so clicks work at new position
    updateButtonOpacity();
}}

// Expose to Rust
window.updateWindows = updateWindows;
</script>
</body>
</html>"#,
        font_css = font_css
    )
    .replace("#L10N_JSON#", &l10n_json)
    .replace("#ICON_SVGS_JSON#", &icon_svgs_json)
}

/// Create the fullscreen transparent canvas window
fn create_canvas_window() {
    unsafe {
        // Initialize COM for WebView on this thread
        let _ = CoInitialize(None);

        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class_name = w!("SGTButtonCanvas");

        REGISTER_CANVAS_CLASS.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(canvas_wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            RegisterClassW(&wc);
        });

        // Get screen dimensions
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        // Create fullscreen transparent window
        // WS_EX_TOPMOST keeps it above result windows
        // WS_EX_TOOLWINDOW hides from taskbar
        // WS_EX_NOACTIVATE prevents focus stealing
        // WS_EX_TRANSPARENT removed to allow hit-testing (we handle passthrough via WM_NCHITTEST)
        // WS_EX_LAYERED removed - interfering with WebView2 creation when WS_EX_TRANSPARENT is missing?
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("ButtonCanvas"),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            screen_w,
            screen_h - 1,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();

        if hwnd.is_invalid() {
            return;
        }

        CANVAS_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // CRITICAL: DwmExtendFrameIntoClientArea with -1 margins enables
        // transparent background while keeping WebView content visible
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

        // Initialize window region to empty (fully click-through)
        let empty_rgn = CreateRectRgn(0, 0, 0, 0);
        let _ = SetWindowRgn(hwnd, Some(empty_rgn), true);

        // Initialize WebContext
        CANVAS_WEB_CONTEXT.with(|ctx| {
            if ctx.borrow().is_none() {
                // Consolidate all minor overlays to 'common' to share one browser process and keep RAM at ~80MB
                let shared_data_dir = crate::overlay::get_shared_webview_data_dir(Some("common"));
                *ctx.borrow_mut() = Some(WebContext::new(Some(shared_data_dir)));
            }
        });

        let html = generate_canvas_html();
        let wrapper = HwndWrapper(hwnd);

        let webview = {
            // LOCK SCOPE: Only one WebView builds at a time to prevent "Not enough quota"
            let _init_lock = crate::overlay::GLOBAL_WEBVIEW_MUTEX.lock().unwrap();
            crate::log_info!("[ButtonCanvas] Acquired init lock. Building...");

            let build_res = CANVAS_WEB_CONTEXT.with(|ctx| {
                let mut ctx_ref = ctx.borrow_mut();
                let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                    WebViewBuilder::new_with_web_context(web_ctx)
                } else {
                    WebViewBuilder::new()
                };

                let builder =
                    crate::overlay::html_components::font_manager::configure_webview(builder);

                // Store HTML in font server
                let page_url =
                    crate::overlay::html_components::font_manager::store_html_page(html.clone())
                        .unwrap_or_else(|| {
                            format!("data:text/html,{}", urlencoding::encode(&html))
                        });

                builder
                    .with_bounds(Rect {
                        position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(
                            0.0, 0.0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            screen_w as u32,
                            (screen_h - 1) as u32,
                        )),
                    })
                    .with_transparent(true)
                    .with_visible(false)
                    .with_focused(false)
                    .with_url(&page_url)
                    .with_ipc_handler(move |msg: wry::http::Request<String>| {
                        handle_ipc_message(msg.body());
                    })
                    .build_as_child(&wrapper)
            });
            crate::log_info!(
                "[ButtonCanvas] Build finished. Releasing lock. Status: {}",
                if build_res.is_ok() { "OK" } else { "ERR" }
            );
            build_res
        };

        match webview {
            Ok(wv) => {
                crate::log_info!("[ButtonCanvas] WebView created successfully!");
                CANVAS_WEBVIEW.with(|cell| {
                    *cell.borrow_mut() = Some(wv);
                });
                IS_WARMED_UP.store(true, Ordering::SeqCst);
                crate::log_info!("[ButtonCanvas] Canvas is now warmed up and ready");
            }
            Err(e) => {
                crate::log_info!("[ButtonCanvas] Failed to create WebView: {:?}", e);
                // CRITICAL: Destroy the window so it doesn't block the screen invisibly
                crate::log_info!("[ButtonCanvas] Destroying canvas window due to WebView failure");
                let _ = DestroyWindow(hwnd);
                CANVAS_HWND.store(0, Ordering::SeqCst);
                CoUninitialize();
                return;
            }
        }

        // Message loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup
        IS_WARMED_UP.store(false, Ordering::SeqCst);
        IS_INITIALIZING.store(false, Ordering::SeqCst);
        CANVAS_HWND.store(0, Ordering::SeqCst);
        CANVAS_WEBVIEW.with(|cell| {
            *cell.borrow_mut() = None;
        });

        CoUninitialize();
    }
}

/// Handle IPC messages from the canvas WebView
fn handle_ipc_message(body: &str) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let action = json.get("action").and_then(|v| v.as_str()).unwrap_or("");

        // Handle clickable regions update (global, not per-window)
        // Handle clickable regions update (global, not per-window)
        if action == "update_clickable_regions" {
            if let Some(regions) = json.get("regions").and_then(|v| v.as_array()) {
                let canvas_hwnd = HWND(CANVAS_HWND.load(Ordering::SeqCst) as *mut std::ffi::c_void);
                if canvas_hwnd.0.is_null() {
                    return;
                }

                // If currently dragging external window, IGNORE region updates
                // We want the window to remain unclipped (full screen) during drag for smoothness
                if IS_DRAGGING_EXTERNAL.load(Ordering::SeqCst) {
                    return;
                }

                unsafe {
                    let combined_rgn = CreateRectRgn(0, 0, 0, 0);

                    // JavaScript sends logical (CSS) coordinates, but SetWindowRgn expects physical
                    let scale = get_dpi_scale();

                    for r in regions {
                        // Parse logical coordinates from JavaScript
                        let logical_x = r.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let logical_y = r.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let logical_w = r.get("w").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let logical_h = r.get("h").and_then(|v| v.as_f64()).unwrap_or(0.0);

                        // Scale to physical coordinates
                        let x = (logical_x * scale) as i32;
                        let y = (logical_y * scale) as i32;
                        let w = (logical_w * scale) as i32;
                        let h = (logical_h * scale) as i32;

                        let rgn = CreateRectRgn(x, y, x + w, y + h);
                        let _ =
                            CombineRgn(Some(combined_rgn), Some(combined_rgn), Some(rgn), RGN_OR);
                        let _ = DeleteObject(rgn.into()); // Delete localized region after combining
                    }

                    // Apply the region to the window
                    // System owns combined_rgn after this call
                    let _ = SetWindowRgn(canvas_hwnd, Some(combined_rgn), true);
                }
            }
            return;
        }

        let hwnd_str = json.get("hwnd").and_then(|v| v.as_str()).unwrap_or("0");
        let hwnd_val: isize = hwnd_str.parse().unwrap_or(0);

        if hwnd_val == 0 {
            return;
        }

        let hwnd = HWND(hwnd_val as *mut std::ffi::c_void);

        match action {
            "copy" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_COPY_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "undo" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_UNDO_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "redo" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_REDO_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "edit" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_EDIT_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "markdown" => {
                crate::overlay::result::trigger_markdown_toggle(hwnd);
            }
            "download" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_DOWNLOAD_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "back" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_BACK_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "forward" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_FORWARD_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "speaker" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_SPEAKER_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "broom_click" => {
                // Close window
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            "broom_right" => unsafe {
                let _ = PostMessageW(
                    Some(hwnd),
                    super::event_handler::misc::WM_COPY_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            },
            "broom_middle" => {
                // Middle-click = close all
                crate::overlay::result::trigger_close_all();
            }
            "broom_drag_start" => {
                // Initiate manual Rust-side drag (more robust than system drag)
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::SetCapture;
                    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

                    let mut pt = windows::Win32::Foundation::POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        use std::sync::atomic::Ordering;
                        ACTIVE_DRAG_TARGET.store(hwnd.0 as isize, Ordering::SeqCst);
                        DRAG_IS_GROUP.store(false, Ordering::SeqCst);

                        let mut last = LAST_DRAG_POS.lock().unwrap();
                        last.x = pt.x;
                        last.y = pt.y;

                        let mut start = START_DRAG_POS.lock().unwrap();
                        start.x = pt.x;
                        start.y = pt.y;

                        let canvas_val = CANVAS_HWND.load(Ordering::SeqCst);
                        if canvas_val != 0 {
                            let canvas_hwnd = HWND(canvas_val as *mut std::ffi::c_void);
                            let _ = SetCapture(canvas_hwnd);
                            // Hide all buttons immediately
                            update_canvas();
                        }
                    }
                }
            }
            "broom_group_drag_start" => {
                // Initiate mass drag for linked windows (Right-click drag)
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::SetCapture;
                    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

                    let mut pt = windows::Win32::Foundation::POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        use std::sync::atomic::Ordering;
                        ACTIVE_DRAG_TARGET.store(hwnd.0 as isize, Ordering::SeqCst);
                        DRAG_IS_GROUP.store(true, Ordering::SeqCst);

                        // Collect the entire group once at start
                        let group = crate::overlay::result::state::get_window_group(hwnd);
                        let mut snapshot = ACTIVE_DRAG_SNAPSHOT.lock().unwrap();
                        *snapshot = group.into_iter().map(|(h, _)| h.0 as isize).collect();

                        let mut last = LAST_DRAG_POS.lock().unwrap();
                        last.x = pt.x;
                        last.y = pt.y;

                        let mut start = START_DRAG_POS.lock().unwrap();
                        start.x = pt.x;
                        start.y = pt.y;

                        let canvas_val = CANVAS_HWND.load(Ordering::SeqCst);
                        if canvas_val != 0 {
                            let canvas_hwnd = HWND(canvas_val as *mut std::ffi::c_void);
                            let _ = SetCapture(canvas_hwnd);
                            // Hide all buttons immediately
                            update_canvas();
                        }
                    }
                }
            }
            "broom_drag" => {
                // Legacy JS-driven drag (unused now but kept for compatibility)
                let scale = get_dpi_scale();
                let dx =
                    (json.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0) * scale).round() as i32;
                let dy =
                    (json.get("dy").and_then(|v| v.as_f64()).unwrap_or(0.0) * scale).round() as i32;
                crate::overlay::result::trigger_drag_window(hwnd, dx, dy);
            }
            _ => {}
        }
    }
}

/// Send updated window data to the canvas
fn send_windows_update() {
    // Check if theme has changed and inject new CSS if needed
    let is_dark = crate::overlay::is_dark_mode();
    let last_dark = LAST_THEME_IS_DARK.load(Ordering::SeqCst);
    if is_dark != last_dark {
        let new_css = get_canvas_theme_css(is_dark);
        // Escape content safely for JS string
        let content_escaped = new_css.replace('`', "\\`").replace('\\', "\\\\");
        let script = format!(
            "var s = document.getElementById('theme-css'); if(s) s.innerHTML = `{}`;",
            content_escaped
        );
        CANVAS_WEBVIEW.with(|cell| {
            if let Some(webview) = cell.borrow().as_ref() {
                let _ = webview.evaluate_script(&script);
            }
        });
        LAST_THEME_IS_DARK.store(is_dark, Ordering::SeqCst);
    }

    let windows_data = {
        let states = WINDOW_STATES.lock().unwrap();
        let windows = MARKDOWN_WINDOWS.lock().unwrap();

        let mut data = serde_json::Map::new();

        // Check for any active native interaction (Resizing/Moving)
        let any_interacting = states.values().any(|s| {
            matches!(
                s.interaction_mode,
                super::state::InteractionMode::Resizing(_)
                    | super::state::InteractionMode::DraggingWindow
                    | super::state::InteractionMode::DraggingGroup(_)
            )
        });

        // If ANY drag (custom or native) or resize is active, hide ALL buttons
        let dragging_target = ACTIVE_DRAG_TARGET.load(std::sync::atomic::Ordering::SeqCst);
        if dragging_target != 0 || any_interacting {
            // Send empty data -> Hides all
            let json = serde_json::to_string(&data).unwrap_or_default();
            CANVAS_WEBVIEW.with(|cell| {
                if let Some(webview) = cell.borrow().as_ref() {
                    let script = format!("window.updateWindows({});", json);
                    let _ = webview.evaluate_script(&script);
                }
            });
            return;
        }

        for (&hwnd_key, &(x, y, w, h)) in windows.iter() {
            let state = states.get(&hwnd_key);

            let state_obj = serde_json::json!({
                "copySuccess": state.map(|s| s.copy_success).unwrap_or(false),
                "hasUndo": state.map(|s| !s.text_history.is_empty()).unwrap_or(false),
                "hasRedo": state.map(|s| !s.redo_history.is_empty()).unwrap_or(false),
                "navDepth": state.map(|s| s.navigation_depth).unwrap_or(0),
                "maxNavDepth": state.map(|s| s.max_navigation_depth).unwrap_or(0),
                "ttsLoading": state.map(|s| s.tts_loading).unwrap_or(false),
                "ttsSpeaking": state.map(|s| s.tts_request_id != 0 && !s.tts_loading).unwrap_or(false),
                "isMarkdown": state.map(|s| s.is_markdown_mode).unwrap_or(false),
                "isBrowsing": state.map(|s| s.is_browsing).unwrap_or(false),
            });

            // Scale physical coordinates to logical coordinates for WebView
            let scale = get_dpi_scale();
            let logical_x = (x as f64 / scale) as i32;
            let logical_y = (y as f64 / scale) as i32;
            let logical_w = (w as f64 / scale) as i32;
            let logical_h = (h as f64 / scale) as i32;

            data.insert(
                hwnd_key.to_string(),
                serde_json::json!({
                    "rect": { "x": logical_x, "y": logical_y, "w": logical_w, "h": logical_h },
                    "state": state_obj
                }),
            );
        }

        serde_json::Value::Object(data)
    };

    CANVAS_WEBVIEW.with(|cell| {
        if let Some(webview) = cell.borrow().as_ref() {
            let script = format!("window.updateWindows({});", windows_data);
            let _ = webview.evaluate_script(&script);
        }
    });
}

// Global state for manual Rust-side dragging
static ACTIVE_DRAG_TARGET: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static DRAG_IS_GROUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ACTIVE_DRAG_SNAPSHOT: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());
static LAST_DRAG_POS: std::sync::Mutex<POINT> = std::sync::Mutex::new(POINT { x: 0, y: 0 });
static START_DRAG_POS: std::sync::Mutex<POINT> = std::sync::Mutex::new(POINT { x: 0, y: 0 });

unsafe extern "system" fn canvas_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_UPDATE_WINDOWS => {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            send_windows_update();
            LRESULT(0)
        }

        WM_APP_SHOW_CANVAS => {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetForegroundWindow, IsWindow, SetForegroundWindow,
                };
                // 1. Capture current focus BEFORE showing our window
                let foreground = GetForegroundWindow();

                // 2. Show the window (SW_SHOWNOACTIVATE should prevent focus stealing, but doesn't always work with WebView)
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );

                // 3. Show WebView (deferred visibility)
                CANVAS_WEBVIEW.with(|cell| {
                    if let Some(webview) = cell.borrow().as_ref() {
                        let _ = webview.set_visible(true);
                    }
                });

                // 4. Force restore focus if it was stolen
                if !foreground.0.is_null() && IsWindow(Some(foreground)).as_bool() {
                    let _ = SetForegroundWindow(foreground);
                }
            }

            let _ = SetTimer(Some(hwnd), CURSOR_POLL_TIMER_ID, 100, None);
            LRESULT(0)
        }

        WM_APP_HIDE_CANVAS => {
            CANVAS_WEBVIEW.with(|cell| {
                if let Some(webview) = cell.borrow().as_ref() {
                    let _ = webview.set_visible(false);
                }
            });
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = KillTimer(Some(hwnd), CURSOR_POLL_TIMER_ID);
            LRESULT(0)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEACTIVATE => {
            LRESULT(windows::Win32::UI::WindowsAndMessaging::MA_NOACTIVATE as isize)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE => {
            let target_val = ACTIVE_DRAG_TARGET.load(Ordering::SeqCst);
            if target_val != 0 {
                // Get current cursor pos
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let mut last = LAST_DRAG_POS.lock().unwrap();
                    let dx = pt.x - last.x;
                    let dy = pt.y - last.y;

                    if dx != 0 || dy != 0 {
                        if DRAG_IS_GROUP.load(Ordering::SeqCst) {
                            let snapshot = ACTIVE_DRAG_SNAPSHOT.lock().unwrap();
                            for &h_val in snapshot.iter() {
                                let h = HWND(h_val as *mut std::ffi::c_void);
                                crate::overlay::result::trigger_drag_window(h, dx, dy);
                            }
                        } else {
                            let target_hwnd = HWND(target_val as *mut std::ffi::c_void);
                            crate::overlay::result::trigger_drag_window(target_hwnd, dx, dy);
                        }

                        // Update last pos
                        last.x = pt.x;
                        last.y = pt.y;
                    }
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP
        | windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP => {
            let target_val = ACTIVE_DRAG_TARGET.load(Ordering::SeqCst);
            if target_val != 0 {
                use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
                // End drag
                ACTIVE_DRAG_TARGET.store(0, Ordering::SeqCst);
                DRAG_IS_GROUP.store(false, Ordering::SeqCst);
                let _ = ReleaseCapture();
                update_canvas(); // Restore buttons after drag

                // Click vs Drag Check
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);

                let start = START_DRAG_POS.lock().unwrap();
                let dist_sq = ((pt.x - start.x).pow(2) + (pt.y - start.y).pow(2)) as f64;

                if dist_sq.sqrt() < 5.0 {
                    // Treating as CLICK
                    let is_right_click =
                        msg == windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP;
                    let target_hwnd = HWND(target_val as *mut std::ffi::c_void);

                    if is_right_click {
                        // Right-click broom click: perform copy (standard broom right-click behavior)
                        crate::overlay::result::trigger_copy(target_hwnd);
                    } else {
                        // Left-click broom click: perform close
                        let _ = PostMessageW(Some(target_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }

                // Force Immediate Cursor Update to JS (so buttons reappear correctly under mouse)
                let scale = get_dpi_scale();
                let logical_x = (pt.x as f64 / scale) as i32;
                let logical_y = (pt.y as f64 / scale) as i32;
                CANVAS_WEBVIEW.with(|cell| {
                    if let Some(webview) = cell.borrow().as_ref() {
                        let script =
                            format!("window.updateCursorPosition({}, {});", logical_x, logical_y);
                        let _ = webview.evaluate_script(&script);
                    }
                });

                // Final update to sync everything (and show buttons again)
                send_windows_update();

                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        WM_TIMER => {
            if wparam.0 == CURSOR_POLL_TIMER_ID {
                // Skip polling if we are dragging (to keep buttons hidden)
                if ACTIVE_DRAG_TARGET.load(Ordering::SeqCst) == 0 {
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let scale = get_dpi_scale();
                        let logical_x = (pt.x as f64 / scale) as i32;
                        let logical_y = (pt.y as f64 / scale) as i32;

                        CANVAS_WEBVIEW.with(|cell| {
                            if let Some(webview) = cell.borrow().as_ref() {
                                let script = format!(
                                    "window.updateCursorPosition({}, {});",
                                    logical_x, logical_y
                                );
                                let _ = webview.evaluate_script(&script);
                            }
                        });
                    }
                }
            }
            LRESULT(0)
        }

        WM_DISPLAYCHANGE => {
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                screen_w,
                screen_h - 1,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            CANVAS_WEBVIEW.with(|cell| {
                if let Some(webview) = cell.borrow().as_ref() {
                    let _ = webview.set_bounds(Rect {
                        position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(
                            0.0, 0.0,
                        )),
                        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                            screen_w as u32,
                            screen_h as u32,
                        )),
                    });
                }
            });
            LRESULT(0)
        }

        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
