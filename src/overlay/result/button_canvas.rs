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

/// Warmup the button canvas - creates hidden fullscreen transparent window
pub fn warmup() {
    if IS_WARMED_UP.load(Ordering::SeqCst) || CANVAS_HWND.load(Ordering::SeqCst) != 0 {
        return;
    }

    std::thread::spawn(|| {
        create_canvas_window();
    });
}

/// Check if canvas is ready
pub fn is_ready() -> bool {
    IS_WARMED_UP.load(Ordering::SeqCst) && CANVAS_HWND.load(Ordering::SeqCst) != 0
}

/// Register a markdown window for button overlay
pub fn register_markdown_window(hwnd: HWND) {
    let hwnd_key = hwnd.0 as isize;

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

    update_canvas();
}

/// Update canvas with current window positions
fn update_canvas() {
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

/// Generate the canvas HTML with buttons
fn generate_canvas_html() -> String {
    let font_css = crate::overlay::html_components::font_manager::get_font_css();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<link href="https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@20..48,100..700,0..1,-50..200" rel="stylesheet" />
<style>
{font_css}

.icons {{
    font-family: 'Material Symbols Rounded';
    font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24;
    font-size: 18px;
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
    gap: 6px;
    padding: 4px;
    pointer-events: auto; /* Buttons accept clicks */
    transition: opacity 0.15s ease-out;
}}

.btn {{
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: rgba(30, 30, 30, 0.85);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    cursor: pointer;
    transition: opacity 0.15s ease-out, background-color 0.15s ease-out, color 0.15s ease-out;
    color: rgba(255, 255, 255, 0.8);
}}

.button-group.vertical {{
    flex-direction: column;
    padding: 8px 4px;
    height: auto;
    width: 48px;
}}
.button-group.vertical .btn {{
    margin: 4px 0;
}}

.btn:hover {{
    background: rgba(60, 60, 60, 0.95);
    color: #4fc3f7;
    transform: scale(1.1);
    box-shadow: 0 0 12px rgba(79, 195, 247, 0.4);
}}

.btn:active {{
    transform: scale(0.95);
}}

.btn.disabled {{
    opacity: 0.3;
    pointer-events: none;
}}

.btn.active {{
    background: rgba(79, 195, 247, 0.3);
    color: #4fc3f7;
}}

.btn.success {{
    background: rgba(76, 175, 80, 0.3);
    color: #81c784;
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
</style>
</head>
<body>
<div id="button-container"></div>
<script>
// Track registered windows: {{ hwnd: {{ x, y, w, h }} }}
window.registeredWindows = {{}};

// Track visibility state to minimize IPC calls
// Key: hwnd string, Value: boolean (isVisible)
let lastVisibleState = new Map();

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
    }});
    
    if (needsUpdate) {{
        // Send updated clickable regions to Rust
        // Only include regions that are currently visible
        const regions = [];
        const padding = 20; // Padding for glow effect and easier clicking
        
        groups.forEach(group => {{
            if (lastVisibleState.get(group.dataset.hwnd)) {{
                const rect = group.getBoundingClientRect();
                regions.push({{
                    x: rect.left - padding,
                    y: rect.top - padding,
                    w: rect.width + (padding * 2),
                    h: rect.height + (padding * 2)
                }});
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
    const longDim = 400; // Length of the button group (Width in Horiz, Height in Vert)
    const shortDim = 50;  // Thickness of the button group (Height in Horiz, Width in Vert)
    const margin = 8;
    
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
function generateButtonsHTML(hwnd, state) {{
    const canGoBack = state.navDepth > 0;
    const canGoForward = state.navDepth < state.maxNavDepth;
    
    let buttons = '';
    
    // Back button (if browsable)
    buttons += `<div class="btn ${{canGoBack ? '' : 'disabled'}}" onclick="action('${{hwnd}}', 'back')" title="Back">
        <span class="icons">arrow_back</span>
    </div>`;
    
    // Forward button (if browsable)  
    buttons += `<div class="btn ${{canGoForward ? '' : 'disabled'}}" onclick="action('${{hwnd}}', 'forward')" title="Forward">
        <span class="icons">arrow_forward</span>
    </div>`;
    
    // Copy
    buttons += `<div class="btn ${{state.copySuccess ? 'success' : ''}}" onclick="action('${{hwnd}}', 'copy')" title="Copy">
        <span class="icons">${{state.copySuccess ? 'check' : 'content_copy'}}</span>
    </div>`;
    
    // Undo
    buttons += `<div class="btn ${{state.hasUndo ? '' : 'disabled'}}" onclick="action('${{hwnd}}', 'undo')" title="Undo">
        <span class="icons">undo</span>
    </div>`;
    
    // Redo
    buttons += `<div class="btn ${{state.hasRedo ? '' : 'disabled'}}" onclick="action('${{hwnd}}', 'redo')" title="Redo">
        <span class="icons">redo</span>
    </div>`;
    
    // Edit/Refine
    buttons += `<div class="btn" onclick="action('${{hwnd}}', 'edit')" title="Refine">
        <span class="icons">edit</span>
    </div>`;
    
    // Markdown toggle
    const mdClass = state.isMarkdown ? 'active' : '';
    buttons += `<div class="btn ${{mdClass}}" onclick="action('${{hwnd}}', 'markdown')" title="Toggle Markdown">
        <span class="icons">article</span>
    </div>`;
    
    // Download
    buttons += `<div class="btn" onclick="action('${{hwnd}}', 'download')" title="Save HTML">
        <span class="icons">download</span>
    </div>`;
    
    // Speaker/TTS
    const speakerIcon = state.ttsLoading ? 'hourglass_empty' : (state.ttsSpeaking ? 'stop' : 'volume_up');
    const speakerClass = state.ttsLoading ? 'loading' : (state.ttsSpeaking ? 'active' : '');
    buttons += `<div class="btn ${{speakerClass}}" onclick="action('${{hwnd}}', 'speaker')" title="Text to Speech">
        <span class="icons">${{speakerIcon}}</span>
    </div>`;
    
    // Broom (close/drag)
    buttons += `<div class="btn broom" 
        onclick="action('${{hwnd}}', 'broom_click')"
        oncontextmenu="action('${{hwnd}}', 'broom_right'); return false;"
        onmousedown="handleBroomDrag(event, '${{hwnd}}')"
        onauxclick="if(event.button===1) action('${{hwnd}}', 'broom_middle')"
        title="Close (drag to move)">
        <span class="icons">cleaning_services</span>
    </div>`;
    
    return buttons;
}}

// Handle broom drag
function handleBroomDrag(e, hwnd) {{
    if (e.button !== 0) return; // Only left click
    broomDragData = {{ hwnd, startX: e.clientX, startY: e.clientY, moved: false }};
    
    const onMove = (ev) => {{
        if (!broomDragData) return;
        const deltaX = ev.clientX - broomDragData.startX;
        const deltaY = ev.clientY - broomDragData.startY;
        
        // Threshold check: waiting for initial move > 4px, then process all
        if (broomDragData.moved || Math.abs(deltaX) > 4 || Math.abs(deltaY) > 4) {{
            broomDragData.moved = true;

            // 1. Immediate Visual Update to prevent lag
            const group = document.querySelector('.button-group[data-hwnd="' + broomDragData.hwnd + '"]');
            if (group) {{
                const curL = parseFloat(group.style.left || 0);
                const curT = parseFloat(group.style.top || 0);
                group.style.left = (curL + deltaX) + 'px';
                group.style.top = (curT + deltaY) + 'px';
            }}

            // 2. Send drag delta to Rust
            window.ipc.postMessage(JSON.stringify({{
                action: 'broom_drag',
                hwnd: broomDragData.hwnd,
                dx: Math.round(deltaX),
                dy: Math.round(deltaY)
            }}));
            
            broomDragData.startX = ev.clientX;
            broomDragData.startY = ev.clientY;
        }}
    }};
    
    const onUp = () => {{
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
        
        if (broomDragData && broomDragData.moved) {{
            // Prevent accidental click triggering after drag
            window.ignoreNextBroomClick = true;
            setTimeout(() => {{ window.ignoreNextBroomClick = false; }}, 100);
        }}
        broomDragData = null;
    }};
    
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
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
            finalY = data.rect.y + data.rect.h + 8; // margin
        }} else if (pos.direction === 'top') {{
            finalX = data.rect.x + (data.rect.w - actualW) / 2;
            finalY = data.rect.y - actualH - 8;
        }} else if (pos.direction === 'right') {{
            finalX = data.rect.x + data.rect.w + 8;
            finalY = data.rect.y + (data.rect.h - actualH) / 2;
        }} else if (pos.direction === 'left') {{
            finalX = data.rect.x - actualW - 8;
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
        
        group.style.left = finalX + 'px';
        group.style.top = finalY + 'px';
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
            screen_h,
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
                let mut data_dir = crate::overlay::get_shared_webview_data_dir();
                // WebView2 on different threads with different Environments MUST use different user data folders
                data_dir.push("button_canvas_thread");
                *ctx.borrow_mut() = Some(WebContext::new(Some(data_dir)));
            }
        });

        let html = generate_canvas_html();
        let wrapper = HwndWrapper(hwnd);

        let webview = CANVAS_WEB_CONTEXT.with(|ctx| {
            let mut ctx_ref = ctx.borrow_mut();
            let builder = if let Some(web_ctx) = ctx_ref.as_mut() {
                WebViewBuilder::new_with_web_context(web_ctx)
            } else {
                WebViewBuilder::new()
            };

            let builder = crate::overlay::html_components::font_manager::configure_webview(builder);

            // Store HTML in font server
            let page_url =
                crate::overlay::html_components::font_manager::store_html_page(html.clone())
                    .unwrap_or_else(|| format!("data:text/html,{}", urlencoding::encode(&html)));

            builder
                .with_bounds(Rect {
                    position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                    size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                        screen_w as u32,
                        screen_h as u32,
                    )),
                })
                .with_transparent(true)
                .with_url(&page_url)
                .with_ipc_handler(move |msg: wry::http::Request<String>| {
                    handle_ipc_message(msg.body());
                })
                .build(&wrapper)
        });

        match webview {
            Ok(wv) => {
                eprintln!("[ButtonCanvas] WebView created successfully!");
                CANVAS_WEBVIEW.with(|cell| {
                    *cell.borrow_mut() = Some(wv);
                });
                IS_WARMED_UP.store(true, Ordering::SeqCst);
                eprintln!("[ButtonCanvas] Canvas is now warmed up and ready");
            }
            Err(e) => {
                eprintln!("[ButtonCanvas] Failed to create WebView: {:?}", e);
                // CRITICAL: Destroy the window so it doesn't block the screen invisibly
                eprintln!("[ButtonCanvas] Destroying canvas window due to WebView failure");
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
            "broom_drag" => {
                // Drag window group - JavaScript sends logical (CSS) pixels, scale to physical
                // User reports raw JS delta is too slow (100 log < 150 phys), so scaling is required.
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
    let windows_data = {
        let states = WINDOW_STATES.lock().unwrap();
        let windows = MARKDOWN_WINDOWS.lock().unwrap();

        let mut data = serde_json::Map::new();

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
            });

            // Scale physical coordinates to logical coordinates for WebView
            // GetWindowRect returns physical pixels, but WebView uses logical (CSS) pixels
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

unsafe extern "system" fn canvas_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_UPDATE_WINDOWS => {
            send_windows_update();
            LRESULT(0)
        }

        WM_APP_SHOW_CANVAS => {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            // Start cursor polling timer (100ms interval - balance between smoothness and performance)
            let _ = SetTimer(Some(hwnd), CURSOR_POLL_TIMER_ID, 100, None);
            LRESULT(0)
        }

        WM_APP_HIDE_CANVAS => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            // Stop cursor polling timer
            let _ = KillTimer(Some(hwnd), CURSOR_POLL_TIMER_ID);
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == CURSOR_POLL_TIMER_ID {
                // Poll cursor position and send to WebView
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    // Scale physical cursor coordinates to logical (CSS) coordinates
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
            LRESULT(0)
        }

        WM_DISPLAYCHANGE => {
            // Screen resolution changed - resize canvas
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                screen_w,
                screen_h,
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
