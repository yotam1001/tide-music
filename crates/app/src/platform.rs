//! One platform integration crate per OS; all expose the same API.
//! See docs/MACOS_PORT.md and docs/LINUX_PORT.md for the contract.
#[cfg(target_os = "linux")]
pub use tide_linux::*;
#[cfg(target_os = "macos")]
pub use tide_macos::*;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub use tide_windows::*;

/// The native view handle the platform crate expects: an `HWND` on Windows,
/// winit's content `NSView*` on macOS, and the X11 window id on Linux.
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
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(handle) => {
            (handle.window != 0).then_some(handle.window as usize as *mut std::ffi::c_void)
        }
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(handle) => Some(handle.window.get() as usize as *mut std::ffi::c_void),
        _ => None,
    }
}

/// Run Tide on X11 (XWayland on Wayland desktops) so the Mini player can stay
/// on top, fade, and find its monitor's work area exactly as on Windows.
/// Wayland gives clients none of those. `TIDE_WAYLAND=1` opts into native
/// Wayland without those features.
#[cfg(target_os = "linux")]
pub fn select_display_backend() {
    use slint::winit_030::winit::platform::x11::EventLoopBuilderExtX11;
    let has_x11 = std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty());
    let wants_wayland = std::env::var_os("TIDE_WAYLAND").is_some_and(|value| value == "1");
    if !has_x11 || wants_wayland {
        return;
    }
    let mut builder = slint::winit_030::winit::event_loop::EventLoop::with_user_event();
    builder.with_x11();
    let _ = slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_event_loop_builder(builder)
        .select();
}
