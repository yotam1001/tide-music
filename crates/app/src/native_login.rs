//! A separate owned login helper; the player never loads WebView2, WKWebView or WebKitGTK.
use super::*;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use std::process::{Command, Stdio};

pub(super) struct Login {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    process: RefCell<Option<tide_playback::OwnedProcess>>,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    deadline: Cell<Option<Instant>>,
    timer: slint::Timer,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    cleanup_timer: slint::Timer,
}

fn helper_path(executable: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "tide-login.exe"
    } else {
        "tide-login"
    };
    let helper = executable.parent()?.join(name);
    let metadata = std::fs::symlink_metadata(&helper).ok()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return None;
        }
    }
    (metadata.is_file() && !metadata.file_type().is_symlink()).then_some(helper)
}

#[cfg(target_os = "linux")]
const WEBKITGTK_MISSING: i32 = 127;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn outcome_message(code: i32) -> &'static str {
    match code {
        0 => "Connection saved. Tide is checking your library.",
        2 => "Sign-in canceled. Your previous connection was kept.",
        3 => "YouTube Music rejected this session. Reopen sign-in and select the profile you use for music.",
        4 => "YouTube Music couldn’t verify your session. Check your connection and try again.",
        5 => "Your saved connection changed during sign-in. Check which profile is connected and retry if needed.",
        #[cfg(target_os = "macos")]
        6 => "Tide could not save the verified session in the macOS Keychain. Try again from your usual macOS account.",
        #[cfg(target_os = "linux")]
        6 => "Tide couldn’t save the verified session. Unlock your desktop keyring and try again.",
        #[cfg(windows)]
        6 => "Tide couldn’t save the verified session in Windows. Try again from your usual Windows account.",
        7 => "Sign-in timed out. Reopen the window when you’re ready to finish.",
        8 => "Google blocked sign-in in the embedded window (common for Family Link or supervised accounts). Use Browser companion or Import session file from your regular browser below.",
        // The dynamic loader exits 127 when libwebkit2gtk-4.1 is not installed.
        #[cfg(target_os = "linux")]
        WEBKITGTK_MISSING => "Sign in with Google needs WebKitGTK. Install libwebkit2gtk-4.1-0 (Debian, Ubuntu) or webkit2gtk4.1 (Fedora, openSUSE), or use Browser companion or Import session file from your regular browser below.",
        _ => "Sign-in could not finish. Retry or use the browser companion below.",
    }
}

pub(super) fn wire(ui: &AppWindow, state: Rc<RefCell<State>>) -> Rc<Login> {
    let login = Rc::new(Login {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        process: RefCell::new(None),
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        deadline: Cell::new(None),
        timer: slint::Timer::default(),
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        cleanup_timer: slint::Timer::default(),
    });
    let path = std::env::current_exe()
        .ok()
        .and_then(|exe| helper_path(&exe));
    ui.set_native_login_available(
        cfg!(any(windows, target_os = "macos", target_os = "linux")) && path.is_some(),
    );
    let weak = ui.as_weak();
    let owner = Rc::downgrade(&login);
    ui.on_native_sign_in(move || {
        let (Some(ui), Some(login)) = (weak.upgrade(), owner.upgrade()) else { return; };
        if ui.get_account_busy() || ui.get_native_login_busy() || !local_controls_allowed(&state.borrow()) { return; }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        let _ = (&login, &path, &ui);
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Some(path) = &path {
            let mut command = Command::new(path);
            command.current_dir(path.parent().expect("helper has parent"))
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            match tide_playback::OwnedProcess::spawn(&mut command) {
                Ok(process) => {
                    *login.process.borrow_mut() = Some(process);
                    login.deadline.set(Some(Instant::now() + Duration::from_secs(10 * 60 + 10)));
                    ui.set_native_login_busy(true);
                    ui.set_native_login_status("Finish signing in in the separate window, choose your Music profile, then Connect.".into());
                    let weak = ui.as_weak();
                    let owner = Rc::downgrade(&login);
                    login.timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
                        let (Some(ui), Some(login)) = (weak.upgrade(), owner.upgrade()) else { return; };
                        let outcome = {
                            let mut slot = login.process.borrow_mut();
                            let Some(process) = slot.as_mut() else { return; };
                            match process.try_wait() {
                                Ok(Some(status)) => Some(status.code().unwrap_or(1)),
                                Err(_) => Some(1),
                                Ok(None) if login.deadline.get().is_some_and(|deadline| Instant::now() >= deadline) => Some(7),
                                Ok(None) => None,
                            }
                        };
                        if let Some(code) = outcome {
                            login.stop(&ui);
                            ui.set_native_login_status(outcome_message(code).into());
                            if code == 8 || (cfg!(target_os = "linux") && code == 127) {
                                if ui.get_companion_setup_available() {
                                    ui.set_companion_setup_open(true);
                                } else {
                                    ui.set_advanced_open(true);
                                }
                            }
                        }
                    });
                }
                Err(_) => ui.set_native_login_status("The sign-in window could not open. Retry or use the browser companion below.".into()),
            }
        }
    });
    let weak = ui.as_weak();
    let owner = Rc::downgrade(&login);
    ui.on_cancel_native_sign_in(move || {
        if let (Some(ui), Some(login)) = (weak.upgrade(), owner.upgrade()) {
            login.stop(&ui);
            // A verified import may have committed just before cancellation; don't claim rollback.
            ui.set_native_login_status(
                "Sign-in window closed. Any verified connection is kept.".into(),
            );
        }
    });
    login
}

impl Login {
    fn stop(&self, ui: &AppWindow) {
        self.timer.stop();
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            self.process.borrow_mut().take(); // Drop kills only this helper's owned tree.
            self.deadline.set(None);
            // A forced cancel/crash cannot run the helper parent's profile Drop.
            // Reuse the ownership-checked stale sweep after Windows releases handles.
            self.cleanup_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(500),
                || {
                    let _ = tide_provider::Provider::new();
                },
            );
        }
        ui.set_native_login_busy(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_helper_is_only_resolved_beside_the_player() {
        let name = if cfg!(windows) {
            "tide-login.exe"
        } else {
            "tide-login"
        };
        let root = std::env::temp_dir().join(format!("tide-helper-path-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        assert!(helper_path(&root.join("tide-music.exe")).is_none());
        std::fs::write(root.join(name), b"fixture").unwrap();
        assert_eq!(
            helper_path(&root.join("tide-music.exe")),
            Some(root.join(name))
        );
        std::fs::remove_file(root.join(name)).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn blocked_outcome_mentions_family_link_and_browser_fallback() {
        let message = outcome_message(8);
        assert!(message.contains("Family Link"));
        assert!(message.contains("Browser companion"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_webkitgtk_names_the_package_and_browser_fallback() {
        let message = outcome_message(WEBKITGTK_MISSING);
        assert!(message.contains("libwebkit2gtk-4.1-0"));
        assert!(message.contains("webkit2gtk4.1"));
        assert!(message.contains("Browser companion"));
    }
}
