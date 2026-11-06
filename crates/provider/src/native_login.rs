//! Explicit opt-in native login prototype. Never part of normal player startup.
//! --probe is offline; no credentials or request headers are printed.
#[cfg(windows)]
pub fn main() {
    std::process::exit(run().unwrap_or(1));
}
#[cfg(not(windows))]
pub fn main() {
    eprintln!("Native login currently requires Windows.");
}

#[cfg(windows)]
fn run() -> Result<i32, ()> {
    use std::{
        path::Path,
        process::{Command, Stdio},
        sync::{atomic::AtomicBool, Arc},
        time::{Duration, Instant},
    };
    use tide_windows::login::{self, Outcome};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--child-probe" || arg == "--child-login")
    {
        let probe = args[0] == "--child-probe";
        if args.len() != 2 {
            return Err(());
        }
        let profile = Path::new(&args[1]);
        let provider = crate::Provider::new().map_err(|_| ())?;
        if !provider.owns_login_directory(profile) {
            if probe {
                eprintln!("Offline login probe: profile ownership rejected");
            }
            return Err(());
        }
        return match login::run(profile, probe).map_err(|error| {
            if probe {
                eprintln!("Offline login probe: {error}");
            }
        })? {
            Outcome::Cancelled => Ok(if probe { 0 } else { 2 }),
            Outcome::TimedOut => Ok(7),
            Outcome::Candidate(candidate) if !probe => {
                use crate::SessionImportError;
                // Exit codes are a fixed, credential-free contract with the player.
                let provider = match crate::Provider::new() {
                    Ok(provider) => provider,
                    Err(_) => return Ok(6),
                };
                Ok(
                    match provider
                        .import_browser_session(&candidate, Arc::new(AtomicBool::new(false)))
                    {
                        Ok(()) => 0,
                        Err(SessionImportError::Cancelled) => 2,
                        Err(
                            SessionImportError::InvalidHeaders
                            | SessionImportError::SessionRejected,
                        ) => 3,
                        Err(SessionImportError::VerificationUnavailable) => 4,
                        Err(SessionImportError::SessionChanged) => 5,
                        Err(SessionImportError::StorageError) => 6,
                    },
                )
            }
            _ => Err(()),
        };
    }
    if args.len() > 1 || args.first().is_some_and(|arg| arg != "--probe") {
        return Err(());
    }
    let probe = !args.is_empty();
    let provider = crate::Provider::new().map_err(|_| ())?;
    let profile = provider.new_login_directory().map_err(|_| ())?;
    let profile_path = profile.path().to_owned();
    let mut command = Command::new(std::env::current_exe().map_err(|_| ())?);
    command
        .arg(if probe {
            "--child-probe"
        } else {
            "--child-login"
        })
        .arg(profile.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if probe {
            Stdio::inherit()
        } else {
            Stdio::null()
        });
    let mut child = tide_playback::OwnedProcess::spawn(&mut command).map_err(|_| ())?;
    let deadline = Instant::now() + Duration::from_secs(if probe { 90 } else { 10 * 60 });
    let code = loop {
        if let Some(status) = child.try_wait().map_err(|_| ())? {
            break status.code().unwrap_or(1);
        }
        if Instant::now() >= deadline {
            break 7;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if probe {
        eprintln!("Offline login probe: helper exit code {code}");
    }
    // The job owns every browser/provider descendant, even after the direct child exits.
    child.terminate_tree().map_err(|_| {
        if probe {
            eprintln!("Offline login probe: process-tree shutdown failed");
        }
    })?;
    drop(child);
    drop(profile);
    if profile_path.exists() {
        if probe {
            eprintln!("Offline login probe: isolated profile cleanup failed");
        }
        return Err(());
    }
    match code {
        0 => println!("Native login {} completed; owned process tree stopped and isolated profile removed.", if probe { "offline probe" } else { "verification" }),
        2 => println!("Native login cancelled; saved account unchanged."),
        _ => eprintln!("Native login could not finish; saved account was not replaced by an unverified session."),
    }
    Ok(code)
}
