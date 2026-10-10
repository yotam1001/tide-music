use std::ffi::c_void;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{factory, Interface, HSTRING};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    ISystemMediaTransportControls2, MediaPlaybackStatus, MediaPlaybackType,
    PlaybackPositionChangeRequestedEventArgs, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsDisplayUpdater, SystemMediaTransportControlsTimelineProperties,
};
use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::WinRT::{
    ISystemMediaTransportControlsInterop, RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use crate::MediaAction;

struct Apartment {
    initialized: bool,
    // WinRT initialization/uninitialization must balance on one thread.
    _ui_thread: PhantomData<Rc<()>>,
}

impl Apartment {
    fn initialize() -> Result<Self, String> {
        // SAFETY: The !Send guard balances successful initialization on this thread.
        let initialized = match unsafe { RoInitialize(RO_INIT_SINGLETHREADED) } {
            Ok(()) => true,
            // The event-loop runtime may already own a different apartment.
            Err(error) if error.code() == RPC_E_CHANGED_MODE => false,
            Err(error) => {
                return Err(format!(
                    "Could not initialize Windows media controls: {error}"
                ))
            }
        };
        Ok(Self {
            initialized,
            _ui_thread: PhantomData,
        })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: This guard remains on its creating thread.
            unsafe { RoUninitialize() };
        }
    }
}

struct CallbackState {
    active: AtomicBool,
    callback: Mutex<Box<dyn Fn(MediaAction) + Send>>,
}

impl CallbackState {
    fn dispatch(&self, action: MediaAction) {
        let callback = self
            .callback
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.active.load(Ordering::Acquire) {
            // A callback panic must not cross a COM event boundary.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(action)));
        }
    }

    fn deactivate(&self) {
        self.active.store(false, Ordering::Release);
        // Wait for a callback that began before deactivation to finish.
        drop(
            self.callback
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Metadata {
    title: String,
    artist: String,
    album: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Timeline {
    position: i64,
    duration: i64,
}

impl Timeline {
    fn from_seconds(position: f64, duration: f64, playing: bool) -> Result<Self, String> {
        if !position.is_finite() || !duration.is_finite() || position < 0.0 || duration < 0.0 {
            return Err("System media timeline values must be finite and non-negative.".into());
        }
        let duration_ticks = (duration * 10_000_000.0) as i64;
        // Windows extrapolates between updates. Half-second precision avoids
        // publishing a COM timeline for each application timer tick.
        let position = if duration > 0.0 {
            position.min(duration)
        } else {
            0.0
        };
        let position_ticks = if playing {
            ((position * 2.0).floor() * 5_000_000.0) as i64
        } else {
            (position * 10_000_000.0) as i64
        };
        Ok(Self {
            position: position_ticks.min(duration_ticks),
            duration: duration_ticks,
        })
    }
}

/// Owns the HWND's controls and subscriptions. Keep this !Send/!Sync owner on
/// the UI thread and drop it before destroying its application window.
pub struct MediaSession {
    controls: SystemMediaTransportControls,
    display: SystemMediaTransportControlsDisplayUpdater,
    timeline: Option<SystemMediaTransportControlsTimelineProperties>,
    button_token: Option<i64>,
    seek_token: Option<i64>,
    callbacks: Arc<CallbackState>,
    metadata_cache: Option<Metadata>,
    status_cache: Option<MediaPlaybackStatus>,
    timeline_cache: Option<Timeline>,
    // Last: release COM objects before uninitializing the apartment.
    _apartment: Apartment,
}

pub(super) fn start(
    hwnd: *mut c_void,
    callback: impl Fn(MediaAction) + Send + 'static,
) -> Result<MediaSession, String> {
    let hwnd = HWND(hwnd);
    // SAFETY: IsWindow validates this opaque handle without dereferencing it.
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(
            "Windows media controls need a valid, already-created application window.".into(),
        );
    }
    let apartment = Apartment::initialize()?;
    let interop: ISystemMediaTransportControlsInterop =
        factory::<SystemMediaTransportControls, _>()
            .map_err(|error| format!("Windows system media controls are unavailable: {error}"))?;
    // SAFETY: The caller supplies its live HWND; IsWindow checked validity.
    let controls: SystemMediaTransportControls =
        unsafe { interop.GetForWindow(hwnd) }.map_err(|error| {
            format!("Could not attach system media controls to the window: {error}")
        })?;
    drop(interop);
    let display = controls.DisplayUpdater().map_err(media_error)?;
    let timeline = if controls.cast::<ISystemMediaTransportControls2>().is_ok() {
        Some(SystemMediaTransportControlsTimelineProperties::new().map_err(media_error)?)
    } else {
        None
    };
    let callbacks = Arc::new(CallbackState {
        active: AtomicBool::new(true),
        callback: Mutex::new(Box::new(callback)),
    });
    // Own the controls before registering any handlers, so partial failure
    // also removes subscriptions and disables the controls.
    let mut session = MediaSession {
        controls,
        display,
        timeline,
        button_token: None,
        seek_token: None,
        callbacks,
        metadata_cache: None,
        status_cache: None,
        timeline_cache: None,
        _apartment: apartment,
    };
    session
        .controls
        .SetIsPlayEnabled(true)
        .map_err(media_error)?;
    session
        .controls
        .SetIsPauseEnabled(true)
        .map_err(media_error)?;
    session
        .controls
        .SetIsNextEnabled(true)
        .map_err(media_error)?;
    session
        .controls
        .SetIsPreviousEnabled(true)
        .map_err(media_error)?;
    session
        .controls
        .SetIsStopEnabled(true)
        .map_err(media_error)?;
    session
        .controls
        .SetPlaybackStatus(MediaPlaybackStatus::Stopped)
        .map_err(media_error)?;

    let callbacks = Arc::clone(&session.callbacks);
    let buttons = TypedEventHandler::<
        SystemMediaTransportControls,
        SystemMediaTransportControlsButtonPressedEventArgs,
    >::new(move |_, args| {
        if let Some(args) = args.as_ref() {
            if let Some(action) = button_action(args.Button()?) {
                callbacks.dispatch(action);
            }
        }
        Ok(())
    });
    session.button_token = Some(
        session
            .controls
            .ButtonPressed(&buttons)
            .map_err(media_error)?,
    );
    if session.timeline.is_some() {
        let callbacks = Arc::clone(&session.callbacks);
        let seeks = TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                let ticks = args.RequestedPlaybackPosition()?.Duration;
                if ticks >= 0 {
                    callbacks.dispatch(MediaAction::Seek(ticks as f64 / 10_000_000.0));
                }
            }
            Ok(())
        });
        session.seek_token = Some(
            session
                .controls
                .PlaybackPositionChangeRequested(&seeks)
                .map_err(media_error)?,
        );
    }
    session.controls.SetIsEnabled(true).map_err(media_error)?;
    Ok(session)
}

fn media_error(error: windows::core::Error) -> String {
    format!("Windows system media controls failed: {error}")
}

fn button_action(button: SystemMediaTransportControlsButton) -> Option<MediaAction> {
    match button {
        SystemMediaTransportControlsButton::Play => Some(MediaAction::Play),
        SystemMediaTransportControlsButton::Pause => Some(MediaAction::Pause),
        SystemMediaTransportControlsButton::Next => Some(MediaAction::Next),
        SystemMediaTransportControlsButton::Previous => Some(MediaAction::Previous),
        SystemMediaTransportControlsButton::Stop => Some(MediaAction::Stop),
        _ => None,
    }
}

impl MediaSession {
    pub fn update(
        &mut self,
        title: &str,
        artist: &str,
        album: &str,
        playing: bool,
        position: f64,
        duration: f64,
    ) -> Result<(), String> {
        let timeline_values = Timeline::from_seconds(position, duration, playing)?;
        let metadata_changed = self.metadata_cache.as_ref().is_none_or(|cached| {
            cached.title != title || cached.artist != artist || cached.album != album
        });
        if metadata_changed {
            self.display
                .SetType(MediaPlaybackType::Music)
                .map_err(media_error)?;
            let music = self.display.MusicProperties().map_err(media_error)?;
            music.SetTitle(&HSTRING::from(title)).map_err(media_error)?;
            music
                .SetArtist(&HSTRING::from(artist))
                .map_err(media_error)?;
            music
                .SetAlbumTitle(&HSTRING::from(album))
                .map_err(media_error)?;
            self.display.Update().map_err(media_error)?;
            self.metadata_cache = Some(Metadata {
                title: title.into(),
                artist: artist.into(),
                album: album.into(),
            });
        }
        let status = if playing {
            MediaPlaybackStatus::Playing
        } else if title.is_empty() && duration == 0.0 {
            MediaPlaybackStatus::Stopped
        } else {
            MediaPlaybackStatus::Paused
        };
        let status_changed = self.status_cache != Some(status);
        if status_changed {
            self.controls
                .SetPlaybackStatus(status)
                .map_err(media_error)?;
            self.status_cache = Some(status);
        }
        if self.timeline_cache != Some(timeline_values) || metadata_changed || status_changed {
            if let Some(timeline) = &self.timeline {
                timeline
                    .SetStartTime(TimeSpan { Duration: 0 })
                    .map_err(media_error)?;
                timeline
                    .SetMinSeekTime(TimeSpan { Duration: 0 })
                    .map_err(media_error)?;
                timeline
                    .SetEndTime(TimeSpan {
                        Duration: timeline_values.duration,
                    })
                    .map_err(media_error)?;
                timeline
                    .SetMaxSeekTime(TimeSpan {
                        Duration: timeline_values.duration,
                    })
                    .map_err(media_error)?;
                timeline
                    .SetPosition(TimeSpan {
                        Duration: timeline_values.position,
                    })
                    .map_err(media_error)?;
                self.controls
                    .UpdateTimelineProperties(timeline)
                    .map_err(media_error)?;
            }
            self.timeline_cache = Some(timeline_values);
        }
        Ok(())
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        self.callbacks.deactivate();
        if let Some(token) = self.button_token.take() {
            let _ = self.controls.RemoveButtonPressed(token);
        }
        if let Some(token) = self.seek_token.take() {
            let _ = self.controls.RemovePlaybackPositionChangeRequested(token);
        }
        let _ = self.controls.SetIsEnabled(false);
        let _ = self
            .controls
            .SetPlaybackStatus(MediaPlaybackStatus::Stopped);
        let _ = self.display.ClearAll();
        let _ = self.display.Update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_window_without_creating_a_session() {
        assert!(start(std::ptr::null_mut(), |_| {}).is_err());
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop; no physical keys injected"]
    fn native_window_session_lifecycle() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        };
        let _test_apartment = Apartment::initialize().unwrap();

        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                // SAFETY: This test owns the live HWND on this thread.
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        // SAFETY: STATIC is a built-in window class. This creates an invisible
        // top-level window on the test thread with no retained Rust pointers.
        let window = TestWindow(
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("Tide media integration test"),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap(),
        );
        let mut session = start(window.0 .0, |_| {}).unwrap();
        assert!(session.controls.IsEnabled().unwrap());
        assert!(session.button_token.is_some());
        session
            .update("Native test", "Test artist", "Test album", true, 1.01, 8.0)
            .unwrap();
        assert_eq!(
            session.controls.PlaybackStatus().unwrap(),
            MediaPlaybackStatus::Playing
        );
        assert_eq!(
            session
                .display
                .MusicProperties()
                .unwrap()
                .Title()
                .unwrap()
                .to_string(),
            "Native test"
        );
        session
            .update("Native test", "Test artist", "Test album", false, 3.19, 8.0)
            .unwrap();
        assert_eq!(
            session.controls.PlaybackStatus().unwrap(),
            MediaPlaybackStatus::Paused
        );
        if let Some(timeline) = &session.timeline {
            assert_eq!(timeline.Position().unwrap().Duration, 31_900_000);
            assert!(session.seek_token.is_some());
        }
        let controls = session.controls.clone();
        let callbacks = Arc::clone(&session.callbacks);
        drop(session);
        assert!(!controls.IsEnabled().unwrap());
        assert!(!callbacks.active.load(Ordering::Acquire));
        drop(controls);
        drop(window);
    }

    #[test]
    fn timeline_clamps_to_track_and_coalesces_timer_ticks() {
        let first = Timeline::from_seconds(1.01, 8.0, true).unwrap();
        assert_eq!(first, Timeline::from_seconds(1.49, 8.0, true).unwrap());
        assert_eq!(
            Timeline::from_seconds(9.0, 8.0, true).unwrap().position,
            80_000_000
        );
        assert_eq!(Timeline::from_seconds(4.0, 0.0, true).unwrap().position, 0);
        assert_eq!(
            Timeline::from_seconds(1.49, 8.0, false).unwrap().position,
            14_900_000
        );
        assert!(Timeline::from_seconds(f64::NAN, 8.0, true).is_err());
        assert!(Timeline::from_seconds(1.0, -1.0, true).is_err());
    }

    #[test]
    fn transport_buttons_map_to_distinct_actions() {
        assert_eq!(
            button_action(SystemMediaTransportControlsButton::Play),
            Some(MediaAction::Play)
        );
        assert_eq!(
            button_action(SystemMediaTransportControlsButton::Pause),
            Some(MediaAction::Pause)
        );
        assert_eq!(
            button_action(SystemMediaTransportControlsButton::Next),
            Some(MediaAction::Next)
        );
        assert_eq!(
            button_action(SystemMediaTransportControlsButton::Record),
            None
        );
    }

    #[test]
    fn callbacks_stop_after_deactivation() {
        use std::sync::atomic::AtomicUsize;
        let count = Arc::new(AtomicUsize::new(0));
        let callback_count = Arc::clone(&count);
        let state = CallbackState {
            active: AtomicBool::new(true),
            callback: Mutex::new(Box::new(move |_| {
                callback_count.fetch_add(1, Ordering::Relaxed);
            })),
        };
        state.dispatch(MediaAction::Play);
        state.deactivate();
        state.dispatch(MediaAction::Pause);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
