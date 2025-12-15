use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::*;
use std::sync::Once;
use crate::gui::locale::LocaleText;

const EM_SETSEL: u32 = 0x00B1;
const EM_GETSEL: u32 = 0x00B0;

static REGISTER_INPUT_CLASS: Once = Once::new();
static mut INPUT_HWND: HWND = HWND(0);
static mut ORIGINAL_EDIT_PROC: Option<WNDPROC> = None;

// Static storage for i18n and display state
static mut CURRENT_UI_LANG: String = String::new();
static mut CURRENT_CANCEL_KEY: String = String::new();
static mut CURRENT_TITLE_OVERRIDE: String = String::new();
static mut FADE_ALPHA: i32 = 0;

// Dragging State (Screen Coordinates)
static mut IS_DRAGGING: bool = false;
static mut DRAG_START_MOUSE: POINT = POINT { x: 0, y: 0 };
static mut DRAG_START_WIN_POS: POINT = POINT { x: 0, y: 0 };

// Callback storage
type SubmitCallback = Box<dyn Fn(String, HWND) + Send>;
static mut ON_SUBMIT: Option<SubmitCallback> = None;

// Colors
const COL_DARK_BG: u32 = 0x202020; // RGB(32, 32, 32)
const COL_OFF_WHITE: u32 = 0xF2F2F2; // RGB(242, 242, 242)

pub fn is_active() -> bool {
    unsafe { INPUT_HWND.0 != 0 }
}

pub fn cancel_input() {
    unsafe {
        if INPUT_HWND.0 != 0 {
            PostMessageW(INPUT_HWND, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn show(
    prompt_guide: String,
    ui_language: String,
    cancel_hotkey_name: String,
    on_submit: impl Fn(String, HWND) + Send + 'static
) {
    unsafe {
        if INPUT_HWND.0 != 0 {
            SetForegroundWindow(INPUT_HWND);
            return;
        }

        ON_SUBMIT = Some(Box::new(on_submit));
        
        // Store strings
        CURRENT_TITLE_OVERRIDE = prompt_guide;
        CURRENT_UI_LANG = ui_language;
        CURRENT_CANCEL_KEY = cancel_hotkey_name;
        FADE_ALPHA = 0;
        IS_DRAGGING = false;

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("SGT_TextInput");

        REGISTER_INPUT_CLASS.call_once(|| {
            let mut wc = WNDCLASSW::default();
            wc.lpfnWndProc = Some(input_wnd_proc);
            wc.hInstance = instance;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap();
            wc.lpszClassName = class_name;
            wc.style = CS_HREDRAW | CS_VREDRAW;
            // Use a null brush to prevent flickering if we paint background manually
            wc.hbrBackground = HBRUSH(0); 
            let _ = RegisterClassW(&wc);
        });

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let win_w = 600;
        let win_h = 250;
        let x = (screen_w - win_w) / 2;
        let y = (screen_h - win_h) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("Text Input"),
            WS_POPUP,
            x, y, win_w, win_h,
            None, None, instance, None
        );
        INPUT_HWND = hwnd;

        // Start invisible for fade-in
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

        // Window Region (Rounded)
        let rgn = CreateRoundRectRgn(0, 0, win_w, win_h, 16, 16);
        SetWindowRgn(hwnd, rgn, true);

        // Calculate Edit Control Rect (Relative)
        let edit_x = 20;
        let edit_y = 50;
        let edit_w = win_w - 40;
        let edit_h = win_h - 90;

        let edit_style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32);
        let h_edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            edit_style,
            edit_x + 5, edit_y + 5, edit_w - 10, edit_h - 10,
            hwnd,
            HMENU(101),
            instance,
            None
        );

        // Apply Font
        let h_font_edit = CreateFontW(16, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
        SendMessageW(h_edit, WM_SETFONT, WPARAM(h_font_edit.0 as usize), LPARAM(1));

        // Subclass Edit
        let old_proc = SetWindowLongPtrW(h_edit, GWLP_WNDPROC, edit_subclass_proc as *const () as isize);
        ORIGINAL_EDIT_PROC = Some(std::mem::transmute(old_proc));

        SetFocus(h_edit);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd); // Ensure focus stealing for both keyboard and mouse hotkeys
        UpdateWindow(hwnd);
        
        // Start Fade Timer (16ms = ~60fps)
        SetTimer(hwnd, 1, 16, None);

        // Message Loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if !IsWindow(hwnd).as_bool() { break; }
        }

        DeleteObject(h_font_edit);
        INPUT_HWND = HWND(0);
        ON_SUBMIT = None;
        ORIGINAL_EDIT_PROC = None;
    }
}

unsafe extern "system" fn edit_subclass_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            let vk = wparam.0 as i32;
            
            // ENTER: Submit
            if vk == VK_RETURN.0 as i32 {
                let shift = (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                if !shift {
                    let len = GetWindowTextLengthW(hwnd) + 1;
                    let mut buf = vec![0u16; len as usize];
                    GetWindowTextW(hwnd, &mut buf);
                    let text = String::from_utf16_lossy(&buf[..len as usize - 1]);
                    
                    if !text.trim().is_empty() {
                        if let Some(cb) = ON_SUBMIT.as_ref() {
                            let parent = GetParent(hwnd);
                            DestroyWindow(parent); 
                            cb(text, parent);
                        }
                    }
                    return LRESULT(0); 
                }
            }
            
            // ESCAPE: Cancel
            if vk == VK_ESCAPE.0 as i32 {
                let parent = GetParent(hwnd);
                DestroyWindow(parent);
                return LRESULT(0); 
            }

            // CTRL+A: Select All
            if vk == 0x41 { // 'A'
                if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
                    SendMessageW(hwnd, EM_SETSEL, WPARAM(0), LPARAM(-1));
                    return LRESULT(0);
                }
            }
            
            // LEFT ARROW: Collapse Selection
            if vk == VK_LEFT.0 as i32 {
                let mut start: u32 = 0;
                let mut end: u32 = 0;
                SendMessageW(hwnd, EM_GETSEL, WPARAM(&mut start as *mut _ as usize), LPARAM(&mut end as *mut _ as isize));
                if start != end {
                    SendMessageW(hwnd, EM_SETSEL, WPARAM(start as usize), LPARAM(start as isize));
                    return LRESULT(0);
                }
            }
        }
        WM_CHAR => {
            // Swallow Enter to prevent beep if no shift
            if wparam.0 == VK_RETURN.0 as usize && (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) == 0 {
                return LRESULT(0);
            }
        }
        _ => {}
    }

    if let Some(proc) = ORIGINAL_EDIT_PROC {
        CallWindowProcW(proc, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

unsafe extern "system" fn input_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Prevent flickering by handling erase background
        WM_ERASEBKGND => LRESULT(1),

        WM_TIMER => {
            if wparam.0 == 1 { 
                // Fade In Logic
                if FADE_ALPHA < 245 {
                    FADE_ALPHA += 25;
                    if FADE_ALPHA > 245 { FADE_ALPHA = 245; }
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), FADE_ALPHA as u8, LWA_ALPHA);
                } else {
                    // Stop timer once fade is complete to save CPU/battery
                    KillTimer(hwnd, 1);
                }
            }
            LRESULT(0)
        }

        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as isize);
            SetBkMode(hdc, OPAQUE);
            SetBkColor(hdc, COLORREF(COL_OFF_WHITE)); 
            SetTextColor(hdc, COLORREF(0x000000)); 
            let hbrush = GetStockObject(DC_BRUSH);
            SetDCBrushColor(hdc, COLORREF(COL_OFF_WHITE));
            LRESULT(hbrush.0 as isize)
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            
            // Check Close Button Click
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let w = rect.right;
            let close_x = w - 30;
            let close_y = 20;
            if (x - close_x).abs() < 15 && (y - close_y).abs() < 15 {
                 PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                 return LRESULT(0);
            }

            // Start Drag
            // FIX 1: Use Screen Coordinates for accurate delta calculation
            IS_DRAGGING = true;
            
            let mut pt_screen = POINT::default();
            GetCursorPos(&mut pt_screen);
            DRAG_START_MOUSE = pt_screen;
            
            let mut rect_win = RECT::default();
            GetWindowRect(hwnd, &mut rect_win);
            DRAG_START_WIN_POS = POINT { x: rect_win.left, y: rect_win.top };
            
            SetCapture(hwnd);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                // FIX 2: Calculate delta based on absolute screen coordinates
                let mut pt_screen = POINT::default();
                GetCursorPos(&mut pt_screen);
                
                let dx = pt_screen.x - DRAG_START_MOUSE.x;
                let dy = pt_screen.y - DRAG_START_MOUSE.y;
                
                let new_x = DRAG_START_WIN_POS.x + dx;
                let new_y = DRAG_START_WIN_POS.y + dy;
                
                SetWindowPos(hwnd, HWND(0), new_x, new_y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false;
                ReleaseCapture();
            }
            LRESULT(0)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            let w = rect.right;
            let h = rect.bottom;

            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bmp = CreateCompatibleBitmap(hdc, w, h);
            let old_bmp = SelectObject(mem_dc, mem_bmp);

            // 1. Draw Background (Dark)
            let brush_bg = CreateSolidBrush(COLORREF(COL_DARK_BG));
            FillRect(mem_dc, &rect, brush_bg);
            DeleteObject(brush_bg);

            // 2. Draw Rounded White Input Area
            let edit_x = 20;
            let edit_y = 50;
            let edit_w = w - 40;
            let edit_h = h - 90;
            
            let brush_white = CreateSolidBrush(COLORREF(COL_OFF_WHITE));
            let old_brush = SelectObject(mem_dc, brush_white);
            let pen_null = GetStockObject(NULL_PEN);
            let old_pen = SelectObject(mem_dc, pen_null);
            
            RoundRect(mem_dc, edit_x, edit_y, edit_x + edit_w, edit_y + edit_h, 12, 12);
            
            SelectObject(mem_dc, old_pen);
            SelectObject(mem_dc, old_brush);
            DeleteObject(brush_white);
            
            // 3. Draw Text Labels
            SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, COLORREF(0x00FFFFFF)); 
            
            let h_font = CreateFontW(19, 0, 0, 0, FW_SEMIBOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            let old_font = SelectObject(mem_dc, h_font);
            
            let locale = LocaleText::get(&CURRENT_UI_LANG);
            let title_str = if !CURRENT_TITLE_OVERRIDE.is_empty() { CURRENT_TITLE_OVERRIDE.clone() } else { locale.text_input_title_default.to_string() };
            let mut title_w = crate::overlay::utils::to_wstring(&title_str);
            let mut r_title = RECT { left: 20, top: 15, right: w - 20, bottom: 45 };
            DrawTextW(mem_dc, &mut title_w, &mut r_title, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS);
            
            let h_font_small = CreateFontW(13, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_SWISS.0) as u32, w!("Segoe UI"));
            SelectObject(mem_dc, h_font_small);
            SetTextColor(mem_dc, COLORREF(0x00AAAAAA)); 
            
            let esc_text = if CURRENT_CANCEL_KEY.is_empty() { "Esc".to_string() } else { format!("Esc / {}", CURRENT_CANCEL_KEY) };
            let hint = format!("{}  |  {}  |  {} {}", locale.text_input_footer_submit, locale.text_input_footer_newline, esc_text, locale.text_input_footer_cancel);
            let mut hint_w = crate::overlay::utils::to_wstring(&hint);
            let mut r_hint = RECT { left: 20, top: h - 30, right: w - 20, bottom: h - 5 };
            DrawTextW(mem_dc, &mut hint_w, &mut r_hint, DT_CENTER | DT_SINGLELINE);

            SelectObject(mem_dc, old_font);
            DeleteObject(h_font);
            DeleteObject(h_font_small);

            // 4. Draw Close Button 'X'
            let c_cx = w - 30;
            let c_cy = 20;
            let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00AAAAAA));
            let old_pen = SelectObject(mem_dc, pen);
            MoveToEx(mem_dc, c_cx - 5, c_cy - 5, None);
            LineTo(mem_dc, c_cx + 5, c_cy + 5);
            MoveToEx(mem_dc, c_cx + 5, c_cy - 5, None);
            LineTo(mem_dc, c_cx - 5, c_cy + 5);
            SelectObject(mem_dc, old_pen);
            DeleteObject(pen);

            // Final Blit
            BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
            SelectObject(mem_dc, old_bmp);
            DeleteObject(mem_bmp);
            DeleteDC(mem_dc);
            
            EndPaint(hwnd, &mut ps);
            LRESULT(0)
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
