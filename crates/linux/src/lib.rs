//! Native Linux desktop integrations, mirroring `tide-windows`.
//!
//! - Media controls: MPRIS (`org.mpris.MediaPlayer2.tide`) on the session bus,
//!   which GNOME, KDE, Cinnamon and most panels show and route media keys to.
//! - Close to tray: a StatusNotifierItem with the same menu as the Windows
//!   notification-area icon.
//! - Mini pinning, opacity and work-area fitting: EWMH on X11 (Tide runs on
//!   X11 or XWayland; see `docs/LINUX_PORT.md`).
//! - Global shortcuts: the XDG GlobalShortcuts portal on Wayland desktops, and
//!   `XGrabKey` registered chords on X11.
//! - Discord: the `discord-ipc-N` Unix socket.
//!
//! Create, update and drop UI resources on the UI thread. Callbacks arrive on
//! background threads and must dispatch their work onto the app's event loop.
#[cfg(not(target_os = "linux"))]
use std::ffi::c_void;
#[cfg(not(target_os = "linux"))]
use std::path::Path;

#[cfg(not(target_os = "linux"))]
const UNSUPPORTED: &str = "This system integration is available on Linux only.";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    ShowMini,
    /// Absolute playback position in seconds.
    Seek(f64),
}

#[cfg(target_os = "linux")]
pub mod discord_pipe;
#[cfg(all(target_os = "linux", feature = "native-login"))]
pub mod login;
pub mod shortcuts;

#[cfg(target_os = "linux")]
mod native;
#[cfg(target_os = "linux")]
pub use native::*;

#[cfg(not(target_os = "linux"))]
mod stubs {
    use super::*;
    pub struct MediaSession;

    pub fn start(
        _view: *mut c_void,
        _callback: impl Fn(MediaAction) + Send + 'static,
    ) -> Result<MediaSession, String> {
        Err(UNSUPPORTED.into())
    }

    impl MediaSession {
        pub fn set_close_to_tray(&self, _enabled: bool) {}

        pub fn set_thumbnail(&mut self, _png: Option<&[u8]>) -> Result<(), String> {
            Err(UNSUPPORTED.into())
        }

        pub fn update(
            &mut self,
            _title: &str,
            _artist: &str,
            _album: &str,
            _playing: bool,
            _position: f64,
            _duration: f64,
        ) -> Result<(), String> {
            Err(UNSUPPORTED.into())
        }

        pub fn hide_to_tray(&self) -> bool {
            false
        }

        pub fn quit_requested(&self) -> bool {
            false
        }
    }

    pub fn integrate_desktop_entry() -> Result<(), String> {
        Ok(())
    }

    pub fn open_web_page(_url: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub fn open_touchpad_settings() -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub fn reveal_in_file_manager(_path: &Path) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub fn app_light_theme() -> Option<bool> {
        None
    }

    pub fn local_hour() -> Option<u8> {
        None
    }

    pub fn client_animations_enabled() -> bool {
        true
    }

    pub fn window_work_area(_view: *mut c_void) -> Result<[i32; 4], String> {
        Err(UNSUPPORTED.into())
    }

    pub fn fit_window_to_work_area(_view: *mut c_void, _work: [i32; 4]) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    pub struct OutputWatcher;

    impl OutputWatcher {
        pub fn new() -> Result<Self, String> {
            Err(UNSUPPORTED.into())
        }

        pub fn take_available(&self) -> Option<bool> {
            None
        }
    }

    pub mod mini {
        use std::ffi::c_void;

        #[derive(Default)]
        pub struct MiniWindow;

        impl MiniWindow {
            pub fn apply(
                &mut self,
                _view: *mut c_void,
                _pinned: bool,
                _opacity: u8,
            ) -> Result<(), String> {
                Err(super::UNSUPPORTED.into())
            }
        }
    }
}
#[cfg(not(target_os = "linux"))]
pub use stubs::*;
