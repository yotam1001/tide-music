//! macOS platform integration for Tide. Mirrors the public API of `tide-windows`;
//! see docs/MACOS_PORT.md. This file currently holds the stub contract only.
use std::ffi::c_void;
use std::path::Path;

const UNSUPPORTED: &str = "This system integration is available on macOS only.";

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

/// Native menu bar, Dock menu and menu bar extra commands (macOS only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Like,
    Search,
    Settings,
    ToggleMini,
    ShowWindow,
    HideWindow,
}

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
}

pub fn install_app_menu(_callback: impl Fn(MenuAction) + 'static) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

/// Hide Tide (like ⌘H); the Dock icon restores it. Playback continues.
pub fn hide_app() -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

/// Unhide Tide and bring its window to the front.
pub fn show_app() -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn notify_song(_title: &str, _artist: &str) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn open_web_page(_url: &str) -> Result<(), String> {
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

pub mod shortcuts {
    use super::MediaAction;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Chord {
        modifiers: u32,
        key: u32,
    }

    pub fn layout_key_name(_character: char) -> Option<String> {
        None
    }

    pub fn parse_bindings(_values: &[String; 3]) -> Result<[Option<Chord>; 3], String> {
        Err(super::UNSUPPORTED.into())
    }

    pub struct GlobalShortcuts;

    impl GlobalShortcuts {
        pub fn new(_callback: impl Fn(MediaAction) + Send + 'static) -> Result<Self, String> {
            Err(super::UNSUPPORTED.into())
        }

        pub fn apply(&mut self, _chords: [Option<Chord>; 3]) -> Result<(), String> {
            Err(super::UNSUPPORTED.into())
        }
    }

    #[allow(dead_code)]
    fn _fields(chord: Chord) -> (u32, u32) {
        (chord.modifiers, chord.key)
    }
}

pub mod discord_pipe {
    use std::io;
    use std::time::{Duration, Instant};

    pub struct DiscordPipe;

    impl DiscordPipe {
        pub fn connect() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                super::UNSUPPORTED,
            ))
        }

        pub fn send(&self, _opcode: u32, _payload: &[u8], _deadline: Instant) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                super::UNSUPPORTED,
            ))
        }

        pub fn receive(&self, _deadline: Instant) -> io::Result<(u32, Vec<u8>)> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                super::UNSUPPORTED,
            ))
        }

        pub fn poll(&self) -> io::Result<Option<(u32, Vec<u8>)>> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                super::UNSUPPORTED,
            ))
        }

        pub fn deadline() -> Instant {
            Instant::now() + Duration::from_millis(500)
        }
    }
}
