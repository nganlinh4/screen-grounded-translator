// Windows 0.62+ type wrappers for thread safety
// HWND, HANDLE, HHOOK etc. are now *mut c_void which don't implement Send/Sync

use windows::Win32::Foundation::{HWND, HANDLE};
use windows::Win32::UI::WindowsAndMessaging::HHOOK;
use windows::Win32::Graphics::Gdi::HBITMAP;

/// Thread-safe wrapper for HWND
#[derive(Clone, Copy, Debug)]
pub struct SendHwnd(pub HWND);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

impl Default for SendHwnd {
    fn default() -> Self {
        SendHwnd(HWND::default())
    }
}

impl SendHwnd {
    pub fn is_invalid(&self) -> bool {
        self.0.is_invalid()
    }
    
    pub fn as_isize(&self) -> isize {
        self.0.0 as isize
    }
    
    pub fn from_isize(val: isize) -> Self {
        SendHwnd(HWND(val as *mut std::ffi::c_void))
    }
}

/// Thread-safe wrapper for HANDLE  
#[derive(Clone, Copy, Debug)]
pub struct SendHandle(pub HANDLE);
unsafe impl Send for SendHandle {}
unsafe impl Sync for SendHandle {}

impl SendHandle {
    pub fn is_invalid(&self) -> bool {
        self.0.is_invalid()
    }
}

/// Thread-safe wrapper for HHOOK
#[derive(Clone, Copy, Debug)]  
pub struct SendHhook(pub HHOOK);
unsafe impl Send for SendHhook {}
unsafe impl Sync for SendHhook {}

impl Default for SendHhook {
    fn default() -> Self {
        SendHhook(HHOOK::default())
    }
}

/// Thread-safe wrapper for HBITMAP
#[derive(Clone, Copy, Debug)]
pub struct SendHbitmap(pub HBITMAP);
unsafe impl Send for SendHbitmap {}
unsafe impl Sync for SendHbitmap {}

impl Default for SendHbitmap {
    fn default() -> Self {
        SendHbitmap(HBITMAP::default())
    }
}

impl SendHbitmap {
    pub fn is_invalid(&self) -> bool {
        self.0.is_invalid()
    }
}
