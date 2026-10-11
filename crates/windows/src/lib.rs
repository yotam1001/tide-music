//! Windows System Media Transport Controls for a live application window.
//!
//! Create, update, and drop the session on the window's UI thread. Callbacks
//! arrive from Windows and must dispatch their work onto the app's event loop.
//! SMTC routes hardware media keys even while another app has focus. Keep this
//! as the only media-key handler: disable media keys in the playback backend
//! rather than also registering a keyboard hook or global media hotkeys.

use std::ffi::c_void;

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
