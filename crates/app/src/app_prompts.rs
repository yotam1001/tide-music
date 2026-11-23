//! One-time notices in the corner: a crash notice after Tide closed
//! unexpectedly, and a single request to star Tide on GitHub.
use super::*;

pub const STAR_URL: &str = "https://github.com/yotam1001/tide-music";
/// Ask only after Tide has been opened a few times, and a while into a session.
const STAR_AFTER_LAUNCHES: u32 = 3;
const STAR_DELAY: Duration = Duration::from_secs(90);

/// Matches the `app-prompt` values in `ui/main.slint`.
pub const NONE: i32 = 0;
pub const STAR: i32 = 1;
pub const CRASH: i32 = 2;

fn crash_log_path() -> Option<PathBuf> {
    Some(settings_path()?.parent()?.join("crash.log"))
}

/// Seconds since the Unix epoch when the crash log was last written.
fn crash_log_stamp() -> Option<u64> {
    let modified = std::fs::metadata(crash_log_path()?).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
}

/// A crash newer than the last one the listener saw.
pub fn unseen_crash(stamp: Option<u64>, seen: u64) -> bool {
    stamp.is_some_and(|stamp| stamp > seen)
}

pub fn star_due(launches: u32, done: bool) -> bool {
    !done && launches >= STAR_AFTER_LAUNCHES
}

fn reveal_crash_log() -> Result<(), String> {
    let path = crash_log_path()
        .filter(|p| p.is_file())
        .ok_or("No crash log was found")?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|_| "Could not open the folder with the crash log".into())
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::reveal_in_file_manager(&path)
            .map_err(|_| "Could not show the crash log in Finder".into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        Err("Opening the crash log is available on Windows and macOS".into())
    }
}

pub fn install(state: &Rc<RefCell<State>>, ui: &AppWindow, enabled: bool) {
    ui.set_crash_log_available(crash_log_stamp().is_some());
    {
        let weak = ui.as_weak();
        // Only Settings > About opens it; show a failure there.
        ui.on_open_crash_log(move || {
            let result = reveal_crash_log();
            if let Some(u) = weak.upgrade() {
                u.set_about_link_status(result.err().unwrap_or_default().into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_app_prompt_answer(move |accepted| {
            let Some(u) = weak.upgrade() else { return };
            let prompt = u.get_app_prompt();
            u.set_app_prompt(NONE);
            if !accepted {
                return;
            }
            let result = match prompt {
                STAR => crate::platform::open_web_page(STAR_URL),
                CRASH => reveal_crash_log(),
                _ => Ok(()),
            };
            if let Err(error) = result {
                u.set_status_text(error.into());
            }
        });
    }
    if !enabled {
        return;
    }
    let mut s = state.borrow_mut();
    if s.settings.transient {
        return;
    }
    s.settings.launches = s.settings.launches.saturating_add(1);
    let stamp = crash_log_stamp();
    if unseen_crash(stamp, s.settings.crash_seen) {
        // Show the crash notice once; the star request waits for a calmer launch.
        s.settings.crash_seen = stamp.unwrap_or_default();
        save(&s.settings);
        ui.set_app_prompt(CRASH);
        return;
    }
    save(&s.settings);
    if !star_due(s.settings.launches, s.settings.star_prompt_done) {
        return;
    }
    drop(s);
    let state = state.clone();
    let weak = ui.as_weak();
    slint::Timer::single_shot(STAR_DELAY, move || {
        let Some(u) = weak.upgrade() else { return };
        if u.get_app_prompt() != NONE {
            return;
        }
        let mut s = state.borrow_mut();
        if s.settings.star_prompt_done {
            return;
        }
        // Mark it before showing, so it never comes back.
        s.settings.star_prompt_done = true;
        save(&s.settings);
        u.set_app_prompt(STAR);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_prompt_waits_for_the_third_launch_and_shows_once() {
        assert!(!star_due(1, false));
        assert!(!star_due(2, false));
        assert!(star_due(3, false));
        assert!(star_due(40, false));
        assert!(!star_due(40, true));
    }

    #[test]
    fn crash_notice_only_for_a_newer_crash() {
        assert!(!unseen_crash(None, 0));
        assert!(unseen_crash(Some(10), 0));
        assert!(!unseen_crash(Some(10), 10));
        assert!(unseen_crash(Some(11), 10));
    }
}
