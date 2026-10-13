//! Windows System Media Transport Controls for a live application window.
//!
//! Create, update, and drop the session on the window's UI thread. Callbacks
//! arrive from Windows and must dispatch their work onto the app's event loop.
//! SMTC routes hardware media keys even while another app has focus. Keep this
//! as the only media-key handler: disable media keys in the playback backend
//! rather than also registering a keyboard hook or global media hotkeys.

use std::ffi::c_void;

/// Current monitor work area, excluding the taskbar. Defer fitting while minimized.
#[cfg(windows)]
pub fn window_work_area(hwnd: *mut c_void) -> Result<[i32; 4], String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};
    let hwnd = HWND(hwnd);
    unsafe {
        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            return Err("Window is minimized or maximized".into());
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool()
        {
            return Err("Window monitor is not ready".into());
        }
        let work = info.rcWork;
        Ok([work.left, work.top, work.right, work.bottom])
    }
}

/// Fit this application's window after startup, a mode, monitor or DPI change.
/// Includes the native frame and taskbar; never activates or raises the window.
#[cfg(windows)]
pub fn fit_window_to_work_area(hwnd: *mut c_void, work: [i32; 4]) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsZoomed, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };
    let hwnd = HWND(hwnd);
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            return Ok(());
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        let [left, top, right, bottom] = work;
        if right <= left || bottom <= top {
            return Err("Monitor work area is empty".into());
        }
        let width = (rect.right - rect.left).min(right - left);
        let height = (rect.bottom - rect.top).min(bottom - top);
        let x = rect.left.clamp(left, right - width);
        let y = rect.top.clamp(top, bottom - height);
        if width != rect.right - rect.left
            || height != rect.bottom - rect.top
            || x != rect.left
            || y != rect.top
        {
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Absolute playback position in seconds.
    Seek(f64),
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::MediaSession;

#[cfg(windows)]
pub fn start(
    hwnd: *mut c_void,
    callback: impl Fn(MediaAction) + Send + 'static,
) -> Result<MediaSession, String> {
    native::start(hwnd, callback)
}

#[cfg(not(windows))]
pub struct MediaSession;

#[cfg(not(windows))]
pub fn start(
    _hwnd: *mut c_void,
    _callback: impl Fn(MediaAction) + Send + 'static,
) -> Result<MediaSession, String> {
    Err("System media controls are currently supported on Windows only.".into())
}

#[cfg(not(windows))]
impl MediaSession {
    pub fn update(
        &mut self,
        _title: &str,
        _artist: &str,
        _album: &str,
        _playing: bool,
        _position: f64,
        _duration: f64,
    ) -> Result<(), String> {
        Err("System media controls are currently supported on Windows only.".into())
    }
}
