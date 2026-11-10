//! One Windows media handler, dispatched through the same guarded UI callbacks.
use crate::{state::State, GroveWindow};
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc, time::Duration};
pub fn install(ui: &GroveWindow, state: Rc<RefCell<State>>, offline: bool) -> slint::Timer {
    let weak = ui.as_weak();
    ui.on_media_command(move |code, value| {
        if let Some(ui) = weak.upgrade() {
            if !ui.get_media_keys() {
                return;
            }
            if ui.get_jam_connected() && !ui.get_jam_host() && ui.get_jam_following() {
                return;
            }
            match code {
                0 => ui.invoke_transport("play".into()),
                1 | 5 => ui.invoke_transport("pause".into()),
                2 => ui.invoke_transport(if ui.get_playing() { "pause" } else { "play" }.into()),
                3 => ui.invoke_transport("skip_next".into()),
                4 => ui.invoke_transport("skip_prev".into()),
                6 => ui.invoke_seek(value * 1000.0),
                _ => {}
            }
        }
    });
    let timer = slint::Timer::default();
    if offline {
        return timer;
    }
    let weak = ui.as_weak();
    let mut media = None::<tide_windows::MediaSession>;
    let mut attempts = 0;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(250),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if !ui.get_media_keys() {
                media = None;
                attempts = 0;
                return;
            }
            if media.is_none() && attempts < 20 {
                attempts += 1;
                match start(&ui) {
                    Ok(session) => media = Some(session),
                    Err(e) if attempts == 20 => {
                        ui.set_message(format!("Windows media controls unavailable: {e}").into())
                    }
                    _ => {}
                }
            }
            let s = state.borrow();
            if let Some(media) = media.as_mut() {
                let (title, artist, playing, position, duration) = if ui.get_local_active() {
                    (
                        ui.get_track_title(),
                        ui.get_artist(),
                        ui.get_playing(),
                        ui.get_position() as f64 / 1000.0,
                        ui.get_duration() as f64 / 1000.0,
                    )
                } else {
                    (
                        s.track.title.clone().into(),
                        s.track.artist.clone().into(),
                        s.playing,
                        s.position() as f64 / 1000.0,
                        s.track.duration as f64 / 1000.0,
                    )
                };
                let _ = media.update(&title, &artist, "Grove", playing, position, duration);
            }
        },
    );
    timer
}
fn start(ui: &GroveWindow) -> Result<tide_windows::MediaSession, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let window = ui.window().window_handle();
    let handle = window
        .window_handle()
        .map_err(|_| "Native window is not ready")?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("Windows media controls need a native window".into());
    };
    let weak = ui.as_weak();
    tide_windows::start(handle.hwnd.get() as *mut std::ffi::c_void, move |action| {
        use tide_windows::MediaAction::*;
        let (code, value) = match action {
            Play => (0, 0.0),
            Pause => (1, 0.0),
            Toggle => (2, 0.0),
            Next => (3, 0.0),
            Previous => (4, 0.0),
            Stop => (5, 0.0),
            Seek(seconds) => (6, seconds as f32),
        };
        let _ = weak.upgrade_in_event_loop(move |ui| ui.invoke_media_command(code, value));
    })
}
