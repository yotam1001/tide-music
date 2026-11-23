//! macOS shell behaviour: native menu bar and Dock menu, Mac window-close
//! semantics, and the optional song-change banner. See docs/MACOS_PORT.md.
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::platform::{self, MenuAction};
use crate::State;
use tide_music::AppWindow;

/// Volume change per ⌘↑ / ⌘↓, on Tide's 0–100 scale.
const VOLUME_STEP: f32 = 5.0;

pub struct MacShell {
    _song_timer: slint::Timer,
}

pub fn install(ui: &AppWindow, state: Rc<RefCell<State>>) -> MacShell {
    // AppKit menus need the winit window and NSApplication to exist, which only
    // happens after the event loop has started.
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(0), move || {
        let menu_weak = weak.clone();
        if let Err(error) =
            platform::install_app_menu(move |action| menu_action(&menu_weak, action))
        {
            if let Some(u) = weak.upgrade() {
                u.set_status_text(format!("macOS menus unavailable: {error}").into());
            }
        }
    });

    // Mac convention for media players: closing the window keeps the music
    // playing and hides Tide; the Dock icon brings it back and ⌘Q quits.
    ui.window().on_close_requested(|| {
        if platform::hide_app().is_ok() {
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            slint::CloseRequestResponse::HideWindow
        }
    });

    let song_timer = slint::Timer::default();
    let weak = ui.as_weak();
    let mut last = (String::new(), String::new());
    song_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            let Some(u) = weak.upgrade() else {
                return;
            };
            let current = (
                u.get_track_title().to_string(),
                u.get_track_artist().to_string(),
            );
            if current == last || current.0.is_empty() {
                return;
            }
            let first = last.0.is_empty();
            last = current;
            let s = state.borrow();
            // No banner for the song restored at launch, while paused, or in transient runs.
            if first || !s.settings.notify_song_change || s.settings.transient || u.get_paused() {
                return;
            }
            let _ = platform::notify_song(&last.0, &last.1);
        },
    );
    MacShell {
        _song_timer: song_timer,
    }
}

fn menu_action(weak: &slint::Weak<AppWindow>, action: MenuAction) {
    let Some(u) = weak.upgrade() else {
        return;
    };
    match action {
        MenuAction::PlayPause => u.invoke_media_command(2, 0.0),
        MenuAction::Next => u.invoke_media_command(3, 0.0),
        MenuAction::Previous => u.invoke_media_command(4, 0.0),
        MenuAction::VolumeUp => {
            u.invoke_change_volume((u.get_volume() + VOLUME_STEP).min(100.0));
        }
        MenuAction::VolumeDown => {
            u.invoke_change_volume((u.get_volume() - VOLUME_STEP).max(0.0));
        }
        MenuAction::Like => {
            if u.get_rating_enabled() && !u.get_rating_busy() {
                u.invoke_rate_song(if u.get_current_rating() == 1 { 0 } else { 1 });
            }
        }
        MenuAction::Search => {
            let _ = platform::show_app();
            if u.get_collapsed() {
                u.invoke_collapse_player();
            }
            u.set_settings_open(false);
            u.set_account_open(false);
            u.invoke_focus_search();
        }
        MenuAction::Settings => {
            let _ = platform::show_app();
            if u.get_collapsed() {
                u.invoke_collapse_player();
            }
            u.set_settings_open(true);
        }
        MenuAction::ToggleMini => u.invoke_collapse_player(),
        MenuAction::ShowWindow => {
            let _ = platform::show_app();
        }
        MenuAction::HideWindow => {
            let _ = platform::hide_app();
        }
        MenuAction::Quit => {
            let _ = slint::quit_event_loop();
        }
    }
}
