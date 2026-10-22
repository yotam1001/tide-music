//! Native mini-player pinning. Never steals focus; full mode restores opacity.
use std::ffi::c_void;
#[derive(Default)]
pub struct MiniWindow {
    #[cfg(windows)]
    added_layered: bool,
}
impl MiniWindow {
    #[cfg(windows)]
    pub fn apply(&mut self, hwnd: *mut c_void, pinned: bool, opacity: u8) -> Result<(), String> {
        use windows::Win32::{
            Foundation::{COLORREF, HWND},
            UI::WindowsAndMessaging::*,
        };
        let hwnd = HWND(hwnd);
        unsafe {
            SetWindowPos(
                hwnd,
                Some(if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .map_err(|e| e.to_string())?;
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            // Restore only the style bit this controller added; preserve renderer-owned styles.
            if opacity < 255 || style & WS_EX_LAYERED.0 as isize != 0 {
                if style & WS_EX_LAYERED.0 as isize == 0 {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
                    self.added_layered = true;
                }
                SetLayeredWindowAttributes(hwnd, COLORREF(0), opacity.max(153), LWA_ALPHA)
                    .map_err(|e| e.to_string())?;
            }
        }
        if opacity == 255 && self.added_layered {
            unsafe {
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & !(WS_EX_LAYERED.0 as isize),
                );
            }
            self.added_layered = false;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    pub fn apply(&mut self, _hwnd: *mut c_void, _pinned: bool, _opacity: u8) -> Result<(), String> {
        Err("Pinned Mini is currently available on Windows".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::{core::w, Win32::UI::WindowsAndMessaging::*};
    #[test]
    fn pin_opacity_and_restore_preserve_native_window() {
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Tide Mini test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let mut mini = MiniWindow::default();
            mini.apply(hwnd.0, true, 180).unwrap();
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            assert_ne!(style & WS_EX_TOPMOST.0 as isize, 0);
            assert_ne!(style & WS_EX_LAYERED.0 as isize, 0);
            let mut alpha = 0;
            GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), None).unwrap();
            assert_eq!(alpha, 180);
            mini.apply(hwnd.0, false, 255).unwrap();
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            assert_eq!(style & (WS_EX_TOPMOST.0 | WS_EX_LAYERED.0) as isize, 0);
            assert!(IsWindow(Some(hwnd)).as_bool());
            DestroyWindow(hwnd).unwrap();
        }
    }
    #[test]
    fn invalid_window_fails_without_panicking() {
        assert!(MiniWindow::default()
            .apply(std::ptr::null_mut(), true, 200)
            .is_err());
    }
}
