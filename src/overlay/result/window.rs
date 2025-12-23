use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::core::*;
use std::mem::size_of;
use std::sync::Once;

use super::state::{WINDOW_STATES, WindowState, CursorPhysics, InteractionMode, ResizeEdge, RefineContext, WindowType};
use super::event_handler::result_wnd_proc;

// Palette for chain windows
// 0: Dark (Primary)
// 1: Green (Secondary)
// 2: Blue
// 3: Purple
// 4: Orange
// 5+: Random/Cyclic
pub const CHAIN_PALETTE: [u32; 5] = [
    0x00222222, // Dark Gray
    0x002d4a22, // Forest Green
    0x00223355, // Deep Blue
    0x00332244, // Muted Purple
    0x00443322, // Brown/Orange
];

pub fn get_chain_color(visible_index: usize) -> u32 {
    if visible_index == 0 {
        CHAIN_PALETTE[0]
    } else {
        let cycle_idx = (visible_index - 1) % (CHAIN_PALETTE.len() - 1);
        CHAIN_PALETTE[cycle_idx + 1]
    }
}

static REGISTER_RESULT_CLASS: Once = Once::new();

// Helper to apply rounded corners to the edit control
unsafe fn set_rounded_edit_region(h_edit: HWND, w: i32, h: i32) {
    // radius (12, 12) matches the overlay style
    let rgn = CreateRoundRectRgn(0, 0, w, h, 12, 12);
    let _ = SetWindowRgn(h_edit, Some(rgn), true);
}

pub fn create_result_window(
    target_rect: RECT,
    _win_type: WindowType,
    context: RefineContext,
    model_id: String,
    provider: String,
    streaming_enabled: bool,
    start_editing: bool,
    preset_prompt: String,
    custom_bg_color: u32,
    render_mode: &str,
) -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        REGISTER_RESULT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance.into();
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap(); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS; 
            wc.hbrBackground = HBRUSH::default();
            let _ = RegisterClassW(&wc);
        });

        let width = (target_rect.right - target_rect.left).abs();
        let height = (target_rect.bottom - target_rect.top).abs();
        
        // WindowType logic essentially just sets color now, but we override it via custom_bg_color usually
        let (x, y) = (target_rect.left, target_rect.top);

        // WS_CLIPCHILDREN prevents parent from drawing over child (Fixes Blinking)
        // WS_EX_NOACTIVATE prevents stealing focus when window appears
        // NOTE: For markdown mode, we match text_input's working configuration exactly
        let (ex_style, base_style) = if render_mode == "markdown" {
            // Markdown mode: match text_input (no WS_CLIPCHILDREN, no WS_EX_NOACTIVATE)
            (
                WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                WS_POPUP
            )
        } else {
            // Plain text mode: prevent focus stealing, use clip children
            (
                WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                WS_POPUP | WS_CLIPCHILDREN
            )
        };
        
        let hwnd = CreateWindowExW(
            ex_style,
            class_name,
            w!(""),
            base_style, 
            x, y, width, height,
            None, None, Some(instance.into()), None
        ).unwrap_or_default();
        
        // FOR MARKDOWN MODE: Create WebView IMMEDIATELY after window creation
        // See docs/WEBVIEW2_INITIALIZATION.md for why this is necessary
        if render_mode == "markdown" {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
            let _ = super::markdown_view::create_markdown_webview(hwnd, "", false);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA);
        }

        let edit_style = WINDOW_STYLE(
            WS_CHILD.0 | 
            WS_BORDER.0 | 
            WS_CLIPSIBLINGS.0 |
            (ES_MULTILINE as u32) |
            (ES_AUTOVSCROLL as u32)
        );
        
        let h_edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            edit_style,
            0, 0, 0, 0, // Sized dynamically
            Some(hwnd),
            Some(HMENU(101 as *mut core::ffi::c_void)),
            Some(instance.into()),
            None
        ).unwrap_or_default();
        
        let hfont = CreateFontW(14, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        SendMessageW(h_edit, WM_SETFONT, Some(WPARAM(hfont.0 as usize)), Some(LPARAM(1)));

        let mut physics = CursorPhysics::default();
        physics.initialized = true;

        // Get graphics mode from config
        let graphics_mode = {
            let app = crate::APP.lock().unwrap();
            app.config.graphics_mode.clone()
        };

        {
            let mut states = WINDOW_STATES.lock().unwrap();
            states.insert(hwnd.0 as isize, WindowState {

                is_hovered: false,
                on_copy_btn: false,
                copy_success: false,
                on_edit_btn: false,
                on_undo_btn: false,
                on_redo_btn: false,
                is_editing: start_editing,
                edit_hwnd: h_edit,
                context_data: context,
                full_text: String::new(),
                text_history: Vec::new(),
                redo_history: Vec::new(),
                is_refining: false,
                animation_offset: 0.0,
                is_streaming_active: false,
                model_id,
                provider,
                streaming_enabled,
                bg_color: custom_bg_color,
                linked_window: None,
                physics,
                interaction_mode: InteractionMode::None,
                current_resize_edge: ResizeEdge::None,
                drag_start_mouse: POINT { x: 0, y: 0 },
                drag_start_window_rect: RECT::default(),
                has_moved_significantly: false,
                font_cache_dirty: true,
                cached_font_size: 72,
                content_bitmap: HBITMAP::default(),
                last_w: 0,
                last_h: 0,
                pending_text: None,
                last_text_update_time: 0,
                bg_bitmap: HBITMAP::default(),
                bg_w: 0,
                bg_h: 0,
                edit_font: hfont,
                preset_prompt, 
                input_text: String::new(),
                graphics_mode,
                cancellation_token: None,
                // Markdown mode state
                is_markdown_mode: render_mode == "markdown",
                on_markdown_btn: false,
                is_browsing: false,
                navigation_depth: 0,
                max_navigation_depth: 0,
                on_back_btn: false,
                on_forward_btn: false,
                on_download_btn: false,
                on_speaker_btn: false,
                tts_request_id: 0,
                tts_loading: false,
            });
        }

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA);
        
        let corner_preference = 2u32; 
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &corner_preference as *const _ as *const _,
            size_of::<u32>() as u32
        );

        if start_editing {
            let width = (target_rect.right - target_rect.left).abs();
            // Initial positioning for the edit box
            let edit_w = width - 20;
            let edit_h = 40;
            SetWindowPos(h_edit, Some(HWND_TOP), 10, 10, edit_w, edit_h, SWP_SHOWWINDOW);
            set_rounded_edit_region(h_edit, edit_w, edit_h);
            
            // FIX: Activate window so Edit control can receive focus immediately
            // WS_EX_NOACTIVATE prevents click-activation, so we must force it here.
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(h_edit));
        }
        
        SetTimer(Some(hwnd), 3, 16, None);
        if render_mode == "markdown" {
            SetTimer(Some(hwnd), 2, 30, None);
            // WebView was already created immediately after window creation (see above)
        }
        
        InvalidateRect(Some(hwnd), None, false);
        UpdateWindow(hwnd);
        
        hwnd
    }
}

pub fn update_window_text(hwnd: HWND, text: &str) {
    if !unsafe { IsWindow(Some(hwnd)).as_bool() } { return; }
    
    let mut states = WINDOW_STATES.lock().unwrap();
    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
        state.pending_text = Some(text.to_string());
        state.full_text = text.to_string();
    }
}
