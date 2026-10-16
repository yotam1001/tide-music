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
fn bounded_file(path: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Cannot open lab input file")?
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read lab input")?;
    if bytes.len() > MAX_BODY {
        return Err("Lab input is too large".into());
    }
    Ok(bytes)
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
    let client = Client::new(&arg(args, "--endpoint")?).map_err(|e| e.to_string())?;
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
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Cannot create private invite file; existing files are preserved")?;
    let room = Hosted {
        client: &client,
        room: client.create().map_err(|e| e.to_string())?,
    };
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
    let mode = args.get(1).map(String::as_str).unwrap_or("help");
    if mode == "relay" {
        let port: u16 = arg(&args, "--port")?.parse().map_err(|_| "Invalid port")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Cannot start relay")?;
        return runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .map_err(|_| "Cannot bind loopback relay")?;
            println!(
                "Loopback-only Jam lab at http://{}; no accounts, audio or public rooms",
                listener
                    .local_addr()
                    .map_err(|_| "Relay address unavailable")?
            );
            let (_stop, receiver) = tokio::sync::watch::channel(false);
            tide_jam_lab::relay::serve(listener, receiver)
                .await
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
            println!("Tide Jam feasibility lab: relay / host / join. See experiments/jam/README.md. Not a release feature.");
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
