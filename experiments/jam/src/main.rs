use std::{
    fs,
    io::{BufRead, Read, Write},
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};
use tide_core::PlaybackCommand;
use tide_jam_lab::{
    client::{Client, Error},
    endpoint::Endpoint,
    protocol::*,
    sync::{Action, Follower, Local},
};
use tide_playback::PlaybackHandle;

fn arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("Missing {name}; see experiments/jam/README.md"))
}
fn present(args: &[String], name: &str) -> bool {
    args.iter().any(|value| value == name)
}
fn validate_args(args: &[String]) -> Result<(), String> {
    let mode = args.get(1).map(String::as_str).unwrap_or("help");
    let values: &[&str] = match mode {
        "relay" => &["--port", "--public-endpoint", "--create-key-file"],
        "keygen" => &["--create-key-file"],
        "host" => &[
            "--endpoint",
            "--queue-file",
            "--invite-file",
            "--seconds",
            "--create-key-file",
        ],
        "join" => &["--invite-file", "--seconds"],
        "help" if args.len() <= 2 => return Ok(()),
        _ => return Err("Unknown Jam lab command; see experiments/jam/README.md".into()),
    };
    let mut seen = std::collections::HashSet::new();
    let mut index = 2;
    while index < args.len() {
        let name = args[index].as_str();
        if !seen.insert(name) {
            return Err("Duplicate Jam option".into());
        }
        if name == "--mute" && matches!(mode, "host" | "join") {
            index += 1;
        } else if values.contains(&name)
            && args.get(index + 1).is_some_and(|v| !v.starts_with("--"))
        {
            index += 2;
        } else {
            return Err("Unknown Jam option or missing option value".into());
        }
    }
    Ok(())
}
fn operator_path(path: &str) -> Result<&Path, String> {
    if !path.ends_with(".jam-operator-key") {
        return Err("Use a private *.jam-operator-key file outside the repository".into());
    }
    Ok(Path::new(path))
}
fn private_file(path: &Path) -> Result<fs::File, String> {
    tide_jam_lab::files::check_private_path(path)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "Cannot create private file; existing files are preserved".into())
}
fn read_operator(path: &str) -> Result<Secret, String> {
    let path = operator_path(path)?;
    let bytes = tide_jam_lab::files::read_private(path, 66)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid operator key file")?;
    Secret::parse(text.trim_end_matches(['\r', '\n']))
        .map_err(|_| "Invalid operator key file".into())
}
fn keygen(args: &[String]) -> Result<(), String> {
    let path = arg(args, "--create-key-file")?;
    let path = operator_path(&path)?;
    let secret = Secret::random()?;
    let mut file = private_file(path)?;
    if file
        .write_all(secret.expose().as_bytes())
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err("Cannot write operator key file".into());
    }
    println!(
        "Private operator key file created. Share it only with trusted room hosts, never guests."
    );
    Ok(())
}
fn bounded_file(path: &str) -> Result<Vec<u8>, String> {
    tide_jam_lab::files::read_private(Path::new(path), MAX_BODY)
        .map(|bytes| bytes.to_vec())
        .map_err(String::from)
}
fn millis(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        (value * 1000.0).min(MAX_POSITION_MS as f64) as u64
    } else {
        0
    }
}
fn local(engine: &PlaybackHandle) -> Local {
    let s = engine.snapshot();
    Local {
        generation: s.generation,
        position_ms: millis(s.position),
        duration_ms: millis(s.duration),
        paused: s.paused,
        ready: !s.loading
            && !s.ended
            && s.error.is_none()
            && s.duration.is_finite()
            && s.duration > 0.0,
    }
}
fn apply(engine: &PlaybackHandle, actions: Vec<Action>) -> Result<(), String> {
    for action in actions {
        let command = match action {
            Action::Load { video, generation } => PlaybackCommand::Load {
                source: video.playback_url(),
                generation,
            },
            Action::Pause(paused) => PlaybackCommand::SetPaused(paused),
            Action::Seek(ms) => PlaybackCommand::Seek(ms as f64 / 1000.0),
        };
        engine
            .send(command)
            .map_err(|_| "Jam playback command failed")?;
    }
    Ok(())
}
struct Hosted<'a> {
    client: &'a Client,
    room: Created,
}
impl Drop for Hosted<'_> {
    fn drop(&mut self) {
        let _ = self.client.close(&self.room.room, &self.room.host);
    }
}
fn invite_path(path: &str) -> Result<&Path, String> {
    if !path.ends_with(".jam-invite.json") {
        return Err(
            "Use a private *.jam-invite.json file, never a shared/repository folder".into(),
        );
    }
    Ok(Path::new(path))
}
fn host(args: &[String], seconds: u64, mute: bool) -> Result<(), String> {
    let endpoint = Endpoint::parse(&arg(args, "--endpoint")?).map_err(|e| e.to_string())?;
    let client = Client::new(endpoint.as_str()).map_err(|e| e.to_string())?;
    let operator = if present(args, "--create-key-file") {
        Some(read_operator(&arg(args, "--create-key-file")?)?)
    } else if endpoint.is_remote() {
        return Err("Private HTTPS hosting requires --create-key-file".into());
    } else {
        None
    };
    let bytes = bounded_file(&arg(args, "--queue-file")?)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Queue must contain plain YouTube IDs, one per line")?;
    let queue: Vec<VideoId> = text
        .lines()
        .filter(|s| !s.is_empty())
        .map(VideoId::parse)
        .collect::<Result<_, _>>()?;
    if queue.is_empty() || queue.len() > MAX_QUEUE {
        return Err("Provide between 1 and 100 YouTube IDs".into());
    }
    let path_text = arg(args, "--invite-file")?;
    let path = invite_path(&path_text)?;
    let room = Hosted {
        client: &client,
        room: match operator.as_ref() {
            Some(key) => client.create_with_key(key),
            None => client.create(),
        }
        .map_err(|e| e.to_string())?,
    };
    drop(operator);
    let mut file = private_file(path)?;
    let invite = Invite {
        endpoint: client.endpoint().into(),
        room: room.room.room.clone(),
        invite: room.room.invite.clone(),
    };
    file.write_all(&tide_jam_lab::client::encode(&invite).map_err(|e| e.to_string())?)
        .map_err(|_| "Cannot write invite file")?;
    drop(file);
    println!("Private lab invite created. No account is read. Commands: pause, resume, next, previous, seek <seconds>, quit.");
    let engine = tide_playback::start()?;
    if mute {
        engine.send(PlaybackCommand::SetVolume(0.0))?;
    }
    let (tx, rx) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            let mut line = Vec::new();
            match input.by_ref().take(129).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if line.len() > 128 => break,
                Ok(_) => {
                    if tx
                        .try_send(String::from_utf8_lossy(&line).trim().to_owned())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let started = Instant::now();
    let mut index = 0usize;
    let mut generation = 1u64;
    let mut revision = 0u64;
    engine.send(PlaybackCommand::Load {
        source: queue[index].playback_url(),
        generation,
    })?;
    loop {
        let mut load = false;
        for command in rx.try_iter() {
            match command.as_str() {
                "quit" => return Ok(()),
                "pause" => engine.send(PlaybackCommand::SetPaused(true))?,
                "resume" => engine.send(PlaybackCommand::SetPaused(false))?,
                "next" if index + 1 < queue.len() => {
                    index += 1;
                    load = true;
                }
                "previous" if index > 0 => {
                    index -= 1;
                    load = true;
                }
                input if input.starts_with("seek ") => {
                    if let Ok(position) = input[5..].parse::<f64>() {
                        if position.is_finite() && (0.0..=86400.0).contains(&position) {
                            engine.send(PlaybackCommand::Seek(position))?;
                        }
                    }
                }
                _ => println!("Unknown or unavailable lab command"),
            }
        }
        let s = engine.snapshot();
        if s.error.is_some() {
            return Err(
                "Host media is unavailable; no account refresh is attempted by this lab".into(),
            );
        }
        if !load && s.generation == generation && s.ended {
            if index + 1 == queue.len() {
                break;
            }
            index += 1;
            load = true;
        }
        if load {
            generation += 1;
            engine.send(PlaybackCommand::Load {
                source: queue[index].playback_url(),
                generation,
            })?;
        }
        let s = engine.snapshot();
        revision += 1;
        client
            .publish(
                &room.room.room,
                &room.room.host,
                &Timeline {
                    revision,
                    play_id: generation,
                    queue: queue.clone(),
                    index,
                    position_ms: if s.generation == generation {
                        millis(s.position)
                    } else {
                        0
                    },
                    paused: s.paused,
                    loading: s.loading || s.generation != generation,
                },
            )
            .map_err(|e| e.to_string())?;
        if started.elapsed() >= Duration::from_secs(seconds) {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
fn join(args: &[String], seconds: u64, mute: bool) -> Result<(), String> {
    let path = arg(args, "--invite-file")?;
    invite_path(&path)?;
    let bytes = zeroize::Zeroizing::new(bounded_file(&path)?);
    let invite: Invite =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid private lab invite")?;
    let client = Client::new(&invite.endpoint).map_err(|e| e.to_string())?;
    let member = client.join(&invite).map_err(|e| e.to_string())?;
    let engine = tide_playback::start()?;
    if mute {
        engine.send(PlaybackCommand::SetVolume(0.0))?;
    }
    let result = (|| {
        let started = Instant::now();
        let mut follower = Follower::default();
        let mut failures = 0;
        while started.elapsed() < Duration::from_secs(seconds) {
            let request_started = Instant::now();
            let actions = match client.poll(&invite.room, &member.member) {
                Ok(view) => {
                    failures = 0;
                    follower.decide(
                        &view,
                        request_started.elapsed().as_millis() as u64,
                        &local(&engine),
                        started.elapsed().as_millis() as u64,
                    )?
                }
                Err(Error::Unavailable | Error::Invalid) => {
                    apply(&engine, follower.disconnect())?;
                    break;
                }
                Err(_) => {
                    failures += 1;
                    if failures >= 5 {
                        apply(&engine, follower.disconnect())?;
                        return Err("Jam disconnected after bounded retries".into());
                    }
                    follower.network_failure(&local(&engine), started.elapsed().as_millis() as u64)
                }
            };
            apply(&engine, actions)?;
            std::thread::sleep(Duration::from_secs(1));
        }
        apply(&engine, follower.disconnect())?;
        Ok(())
    })();
    let _ = client.leave(&invite.room, &member.member);
    result
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    validate_args(&args)?;
    let mode = args.get(1).map(String::as_str).unwrap_or("help");
    if mode == "keygen" {
        return keygen(&args);
    }
    if mode == "relay" {
        let port: u16 = arg(&args, "--port")?.parse().map_err(|_| "Invalid port")?;
        // Validate the full proxy contract before opening even a loopback socket.
        let private = match (
            present(&args, "--public-endpoint"),
            present(&args, "--create-key-file"),
        ) {
            (false, false) => None,
            (true, true) => Some(tide_jam_lab::relay::PrivateRelay::new(
                &arg(&args, "--public-endpoint")?,
                read_operator(&arg(&args, "--create-key-file")?)?,
            )?),
            _ => {
                return Err(
                    "Private relay mode requires both --public-endpoint and --create-key-file"
                        .into(),
                )
            }
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Cannot start relay")?;
        return runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .map_err(|_| "Cannot bind loopback relay")?;
            println!("Jam backend bound to loopback. No accounts, audio or room discovery. See the private relay guide before exposing a TLS proxy.");
            let (_stop, receiver) = tokio::sync::watch::channel(false);
            let result = match private {
                Some(config) => tide_jam_lab::relay::serve_private(listener, receiver, config).await,
                None => tide_jam_lab::relay::serve(listener, receiver).await,
            };
            result
                .map_err(|_| "Relay stopped".to_string())
        });
    }
    let seconds = arg(&args, "--seconds")
        .unwrap_or_else(|_| "300".into())
        .parse::<u64>()
        .map_err(|_| "Invalid duration")?;
    if !(5..=1800).contains(&seconds) {
        return Err("Lab runs must last 5 to 1800 seconds".into());
    }
    let mute = args.iter().any(|a| a == "--mute");
    match mode {
        "host" => host(&args, seconds, mute),
        "join" => join(&args, seconds, mute),
        _ => {
            println!("Tide Jam lab: relay / keygen / host / join. See experiments/jam/README.md. Not a release feature.");
            Ok(())
        }
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
