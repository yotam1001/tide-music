//! Subprocess checks use only synthetic capabilities in disposable private files.
//! They do not start playback, read an account or expose a relay.
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tide-jam-cli-{}-{}",
            std::process::id(),
            tide_jam_lab::protocol::room_id().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn key(&self) -> PathBuf {
        self.0.join("operator.jam-operator-key")
    }
    fn invite(&self) -> PathBuf {
        self.0.join("test.jam-invite.json")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        // Only individually owned fixture files; never a recursive delete.
        let _ = fs::remove_file(self.key());
        let _ = fs::remove_file(self.invite());
        let _ = fs::remove_dir(&self.0);
    }
}
fn run(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tide-jam-lab"));
    command.args(args);
    bounded(command)
}
fn bounded(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("CLI fixture exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn rejected(args: &[&str]) {
    let result = run(args);
    assert!(!result.status.success());
}

#[test]
fn key_generation_is_random_private_redacted_and_never_overwrites() {
    let scratch = Scratch::new();
    let path = scratch.key();
    let path = path.to_str().unwrap();
    let first = run(&["keygen", "--create-key-file", path]);
    assert!(first.status.success());
    let bytes = fs::read(path).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(tide_jam_lab::protocol::Secret::parse(text).is_ok());
    assert!(!String::from_utf8_lossy(&first.stdout).contains(text));
    assert!(!String::from_utf8_lossy(&first.stderr).contains(text));
    rejected(&["keygen", "--create-key-file", path]);
    assert_eq!(fs::read(path).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn incomplete_or_invalid_private_relay_configuration_fails_before_listening() {
    let scratch = Scratch::new();
    let path = scratch.key();
    let path = path.to_str().unwrap();
    rejected(&[
        "relay",
        "--port",
        "0",
        "--public-endpoint",
        "https://jam.example.test",
    ]);
    rejected(&["relay", "--port", "0", "--create-key-file", path]);
    rejected(&[
        "relay",
        "--port",
        "0",
        "--public-endpoint",
        "https://jam.example.test",
        "--create-key-file",
        path,
    ]);
    for data in [
        "".to_owned(),
        "bad".into(),
        "a".repeat(64) + &"\n".repeat(1000),
        "a".repeat(64) + "extra",
    ] {
        fs::write(path, data).unwrap();
        rejected(&[
            "relay",
            "--port",
            "0",
            "--public-endpoint",
            "https://jam.example.test",
            "--create-key-file",
            path,
        ]);
    }
    fs::write(path, "a".repeat(64)).unwrap();
    rejected(&[
        "relay",
        "--port",
        "0",
        "--public-endpoint",
        "http://127.0.0.1:1234",
        "--create-key-file",
        path,
    ]);
    rejected(&[
        "host",
        "--endpoint",
        "https://jam.example.test",
        "--queue-file",
        "unused",
        "--invite-file",
        "unused.jam-invite.json",
    ]);
}

#[test]
fn duplicate_unknown_or_insecure_cli_options_are_not_silently_ignored() {
    for args in [
        vec!["relay", "--port", "0", "--port", "1"],
        vec!["relay", "--port", "0", "--bind", "0.0.0.0"],
        vec!["relay", "--port", "0", "--insecure"],
        vec![
            "host",
            "--endpoint",
            "https://jam.example.test",
            "--create-key-file",
        ],
        vec![
            "join",
            "--invite-file",
            "unused.jam-invite.json",
            "--create-key-file",
            "unused.jam-operator-key",
        ],
        vec!["keygen", "--create-key-file", "unused.json"],
    ] {
        rejected(&args);
    }
}

#[test]
fn environment_proxies_receive_neither_requests_nor_capabilities() {
    let scratch = Scratch::new();
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let trap = TcpListener::bind("127.0.0.1:0").unwrap();
    trap.set_nonblocking(true).unwrap();
    let endpoint = format!("https://{}", target.local_addr().unwrap());
    let invite = tide_jam_lab::protocol::Invite {
        endpoint,
        room: tide_jam_lab::protocol::room_id().unwrap(),
        invite: tide_jam_lab::protocol::Secret::random().unwrap(),
    };
    fs::write(
        scratch.invite(),
        tide_jam_lab::client::encode(&invite).unwrap().as_slice(),
    )
    .unwrap();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match target.accept() {
                Ok((mut socket, _)) => {
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut input = [0; 1024];
                    let _ = socket.read(&mut input);
                    // Plaintext is intentionally rejected by the HTTPS client.
                    let _ =
                        socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    return true;
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => return false,
            }
        }
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_tide-jam-lab"));
    command
        .arg("join")
        .arg("--invite-file")
        .arg(scratch.invite());
    let proxy = format!("http://{}", trap.local_addr().unwrap());
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env(key, &proxy);
    }
    command.env("NO_PROXY", "").env("no_proxy", "");
    let result = bounded(command);
    assert!(!result.status.success());
    assert!(
        worker.join().unwrap(),
        "client did not contact the selected endpoint directly"
    );
    assert!(matches!(trap.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    for output in [&result.stdout, &result.stderr] {
        assert!(!String::from_utf8_lossy(output).contains(invite.invite.expose()));
    }
}
