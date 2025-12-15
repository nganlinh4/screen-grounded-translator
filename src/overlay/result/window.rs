use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::core::*;
use std::mem::size_of;
use std::sync::Once;

use super::state::{WINDOW_STATES, WindowState, CursorPhysics, InteractionMode, ResizeEdge, RefineContext, WindowType, RetranslationConfig};
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
    SetWindowRgn(h_edit, rgn, true);
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
    _unused_retrans: Option<RetranslationConfig>,
    custom_bg_color: u32,
) -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("TranslationResult");
        
        REGISTER_RESULT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap(); 
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS; 
            wc.hbrBackground = HBRUSH(0);
            let _ = RegisterClassW(&wc);
        });

        let width = (target_rect.right - target_rect.left).abs();
        let height = (target_rect.bottom - target_rect.top).abs();
        
        // WindowType logic essentially just sets color now, but we override it via custom_bg_color usually
        let (x, y) = (target_rect.left, target_rect.top);

        // WS_CLIPCHILDREN prevents parent from drawing over child (Fixes Blinking)
        // WS_EX_NOACTIVATE prevents stealing focus when window appears
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN, 
            x, y, width, height,
            None, None, instance, None
        );

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
            hwnd,
            HMENU(101),
            instance,
            None
        );
        
        let hfont = CreateFontW(14, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        SendMessageW(h_edit, WM_SETFONT, WPARAM(hfont.0 as usize), LPARAM(1));

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
                alpha: 220,
                is_hovered: false,
                on_copy_btn: false,
                copy_success: false,
                on_edit_btn: false,
                on_undo_btn: false,
                is_editing: start_editing,
                edit_hwnd: h_edit,
                context_data: context,
                full_text: String::new(),
                text_history: Vec::new(),
                is_refining: false,
                animation_offset: 0.0,
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
                content_bitmap: HBITMAP(0),
                last_w: 0,
                last_h: 0,
                pending_text: None,
                last_text_update_time: 0,
                bg_bitmap: HBITMAP(0),
                bg_w: 0,
                bg_h: 0,
                edit_font: hfont,
                preset_prompt, 
                retrans_config: None, // Deprecated in favor of chain logic
                graphics_mode,
                cancellation_token: None,
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
            SetWindowPos(h_edit, HWND_TOP, 10, 10, edit_w, edit_h, SWP_SHOWWINDOW);
            set_rounded_edit_region(h_edit, edit_w, edit_h);
            
            // FIX: Activate window so Edit control can receive focus immediately
            // WS_EX_NOACTIVATE prevents click-activation, so we must force it here.
            SetForegroundWindow(hwnd);
            SetFocus(h_edit);
        }
        
        SetTimer(hwnd, 3, 16, None);
        
        InvalidateRect(hwnd, None, false);
        UpdateWindow(hwnd);
        
        hwnd
    }
}

pub fn update_window_text(hwnd: HWND, text: &str) {
    if !unsafe { IsWindow(hwnd).as_bool() } { return; }
    
    let mut states = WINDOW_STATES.lock().unwrap();
    if let Some(state) = states.get_mut(&(hwnd.0 as isize)) {
        state.pending_text = Some(text.to_string());
        state.full_text = text.to_string();
    }
}
