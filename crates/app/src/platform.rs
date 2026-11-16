//! One platform integration crate per OS; both expose the same API.
//! See docs/MACOS_PORT.md for the contract.
#[cfg(target_os = "macos")]
pub use tide_macos::*;
#[cfg(not(target_os = "macos"))]
pub use tide_windows::*;

/// The native view handle the platform crate expects: an `HWND` on Windows
/// and winit's content `NSView*` on macOS.
#[cfg(native_shell)]
pub fn native_view(window: &slint::Window) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle();
    let handle = handle.window_handle().ok()?;
    match handle.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => Some(handle.ns_view.as_ptr()),
        _ => None,
    }
}
