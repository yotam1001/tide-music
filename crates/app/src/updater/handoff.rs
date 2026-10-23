//! Bounded readiness handshake with the exact, already verified setup process.
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(super) struct Pending {
    child: Child,
    receiver: mpsc::Receiver<Result<(), String>>,
    reader: Option<JoinHandle<()>>,
    deadline: Instant,
    accepted: bool,
}

impl Pending {
    pub(super) fn start(path: &Path, version: &str) -> Result<Self, String> {
        let mut command = Command::new(path);
        command
            .arg("--update-from")
            .arg(std::process::id().to_string())
            .arg("--confirm-ready")
            .current_dir(path.parent().ok_or("Invalid installer path")?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|error| {
            format!(
                "Windows could not start setup (code {}). Your music continues; retry Restart.",
                error.raw_os_error().unwrap_or(0)
            )
        })?;
        let stdout = child.stdout.take().expect("piped setup stdout");
        let expected = format!("TIDE_UPDATE_READY|{}|{version}", std::process::id());
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = match std::thread::Builder::new()
            .name("tide-setup-ready".into())
            .spawn(move || {
                let mut line = String::new();
                let read = BufReader::new(stdout.take(256))
                    .read_line(&mut line)
                    .is_ok();
                let signal = if read && line.trim_end_matches(['\r', '\n']) == expected {
                    Ok(())
                } else {
                    Err(match line.trim_end_matches(['\r', '\n']) {
                        "TIDE_UPDATE_FAILED|busy" => "Another Tide setup is running. Your music continues; finish it and retry Restart.",
                        "TIDE_UPDATE_FAILED|ownership" => "Setup found a Windows registration conflict. Your music continues; open the installer for details.",
                        "TIDE_UPDATE_FAILED|package" => "Setup rejected the package. Your music continues; choose a fresh download.",
                        "TIDE_UPDATE_FAILED|preflight" => "Setup could not finish its installation checks. Your music continues; retry or open the installer for details.",
                        _ => "Setup did not confirm readiness. Your music continues; retry Restart.",
                    }.to_owned())
                };
                let _ = sender.send(signal);
            }) {
            Ok(reader) => reader,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Could not monitor setup. Your music continues; retry Restart.".into());
            }
        };
        Ok(Self {
            child,
            receiver,
            reader: Some(reader),
            deadline: Instant::now() + Duration::from_secs(30),
            accepted: false,
        })
    }

    pub(super) fn poll(&mut self) -> Option<Result<(), String>> {
        match self.receiver.try_recv() {
            Ok(Ok(())) => {
                match self.child.try_wait() {
                    Ok(None) => {}
                    _ => {
                        return Some(Err(
                            "Setup closed before restart was committed. Retry Restart.".into(),
                        ))
                    }
                }
                if self.child.stdin.as_mut().is_none_or(|input| {
                    input
                        .write_all(b"COMMIT\n")
                        .and_then(|()| input.flush())
                        .is_err()
                }) {
                    return Some(Err(
                        "Could not confirm restart with setup. Retry Restart.".into()
                    ));
                }
                self.accepted = true;
                Some(Ok(()))
            }
            Ok(Err(message)) => Some(Err(message)),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "Setup did not confirm readiness. Your music continues; retry Restart.".into(),
            )),
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= self.deadline => Some(Err(
                "Setup took too long to prepare. Your music continues; retry Restart.".into(),
            )),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }

    #[cfg(test)]
    pub(super) fn expire_for_test(&mut self) {
        self.deadline = Instant::now();
    }

    #[cfg(test)]
    pub(super) fn wait_for_test_exit(&mut self) {
        let _ = self.child.wait();
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.accepted {
            // Before acknowledgment setup has not modified the installation.
            // Stop only the child owned by this specific restart attempt.
            if let Some(mut input) = self.child.stdin.take() {
                let _ = input.write_all(b"CANCEL\n");
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match self.child.try_wait() {
                    Ok(Some(_)) => break,
                    _ if Instant::now() >= deadline => {
                        if self.child.kill().is_ok() {
                            let _ = self.child.wait();
                        }
                        break;
                    }
                    _ => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        }
        if let Some(reader) = self.reader.take() {
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }
}
