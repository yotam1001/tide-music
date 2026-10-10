//! Muted native-runtime check. Supply a seekable local audio file or HTTP(S) URL.
use std::thread;
use std::time::{Duration, Instant};

use tide_core::{PlaybackCommand, PlaybackSnapshot};
use tide_playback::{start, start_with_account, PlaybackHandle};

fn wait_for(
    handle: &PlaybackHandle,
    timeout: Duration,
    label: &str,
    condition: impl Fn(&PlaybackSnapshot) -> bool,
) -> Result<PlaybackSnapshot, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let snapshot = handle.snapshot();
        if let Some(error) = &snapshot.error {
            return Err(format!("{label}: {error}"));
        }
        if snapshot.volume != 0.0 {
            return Err(format!("{label}: smoke playback must remain muted"));
        }
        if condition(&snapshot) {
            return Ok(snapshot);
        }
        if Instant::now() >= deadline {
            return Err(format!("{label} timed out; snapshot={snapshot:?}"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn run() -> Result<(), String> {
    let source = std::env::args().nth(1).ok_or_else(|| {
        "Usage: cargo run -p tide-playback --example smoke -- <seekable audio file or URL>"
            .to_owned()
    })?;
    // Local files normally load quickly. Allow extraction/network startup time
    // for URLs, but every operation still has a fixed deadline.
    let timeout = Duration::from_secs(45);
    // Optional local fixture jar exercises mpv option escaping without placing
    // any cookie contents on the command line. Never use real sessions in CI.
    let account_index = std::env::args()
        .nth(3)
        .map(|value| value.parse::<u32>().map_err(|_| "Invalid account index"))
        .transpose()?
        .unwrap_or(0);
    let handle = start_with_account(std::env::args().nth(2).map(Into::into), account_index)?;
    let initial = handle.snapshot();
    if !initial.paused || initial.loading || initial.generation != 0 {
        return Err(format!("Unexpected initial snapshot: {initial:?}"));
    }
    handle.send(PlaybackCommand::SetVolume(0.0))?;
    handle.send(PlaybackCommand::Load {
        source: source.clone(),
        generation: 1,
    })?;
    let loaded = wait_for(&handle, timeout, "load", |s| {
        s.generation == 1 && !s.loading && s.duration >= 4.0 && s.position > 0.05 && !s.paused
    })?;
    println!(
        "PASS load: generation=1 duration={:.3}s position={:.3}s volume=0",
        loaded.duration, loaded.position
    );

    handle.send(PlaybackCommand::TogglePause)?;
    let paused = wait_for(&handle, timeout, "pause", |s| s.paused)?;
    thread::sleep(Duration::from_millis(200));
    let settled = handle.snapshot();
    if (settled.position - paused.position).abs() > 0.25 || !settled.paused {
        return Err(format!("Paused position kept advancing: {settled:?}"));
    }
    println!("PASS pause: position stopped advancing");
    handle.send(PlaybackCommand::SetPaused(true))?;
    handle.send(PlaybackCommand::SetPaused(true))?;
    thread::sleep(Duration::from_millis(200));
    if !handle.snapshot().paused {
        return Err("Repeated Pause resumed playback".into());
    }
    println!("PASS idempotent pause: repeated media Pause stays paused");

    let target = (loaded.duration * 0.5).min(3.0);
    handle.send(PlaybackCommand::Seek(target))?;
    wait_for(&handle, timeout, "seek", |s| {
        s.paused && (s.position - target).abs() < 0.35
    })?;
    println!("PASS seek: paused position reached {target:.3}s");

    // A replacement must start playing even if the previous process paused.
    // Back-to-back loads also exercise superseded-load cancellation.
    handle.send(PlaybackCommand::Load {
        source: source.clone(),
        generation: 2,
    })?;
    handle.send(PlaybackCommand::Load {
        source: source.clone(),
        generation: 3,
    })?;
    wait_for(&handle, timeout, "replacement", |s| {
        s.generation == 3 && !s.loading && s.position > 0.05 && !s.paused && !s.ended
    })?;
    if handle
        .send(PlaybackCommand::Load {
            source: source.clone(),
            generation: 2,
        })
        .is_ok()
    {
        return Err("An older generation was accepted".into());
    }
    println!("PASS replacement: generation=3 playing; stale generation rejected");

    handle.send(PlaybackCommand::Stop)?;
    let stopped = handle.snapshot();
    if stopped.loading || !stopped.paused || stopped.position != 0.0 || stopped.title.is_some() {
        return Err(format!("Stop did not reset snapshot: {stopped:?}"));
    }
    thread::sleep(Duration::from_millis(250));
    let settled = handle.snapshot();
    if settled.position != 0.0 || settled.loading || settled.error.is_some() {
        return Err(format!(
            "A late event changed the stopped snapshot: {settled:?}"
        ));
    }
    println!("PASS stop: snapshot reset and late events suppressed");

    handle.send(PlaybackCommand::Load {
        source: source.clone(),
        generation: 4,
    })?;
    wait_for(&handle, timeout, "restart after stop", |s| {
        s.generation == 4 && !s.loading && s.position > 0.05
    })?;
    handle.send(PlaybackCommand::TogglePause)?;
    let paused = wait_for(&handle, timeout, "pause before EOF", |s| s.paused)?;
    let near_end = paused.duration - 0.3;
    handle.send(PlaybackCommand::Seek(near_end))?;
    wait_for(&handle, timeout, "seek near EOF", |s| {
        (s.position - near_end).abs() < 0.25
    })?;
    handle.send(PlaybackCommand::TogglePause)?;
    wait_for(&handle, timeout, "EOF", |s| {
        s.generation == 4 && s.ended && s.paused
    })?;
    println!("PASS EOF: active generation marked ended and paused");
    let shutdown_started = Instant::now();
    handle.send(PlaybackCommand::Shutdown)?;
    if handle.send(PlaybackCommand::TogglePause).is_ok() {
        return Err("Commands were accepted after shutdown".into());
    }
    drop(handle);
    if shutdown_started.elapsed() > Duration::from_secs(3) {
        return Err("Shutdown exceeded its 3 second deadline".into());
    }
    println!(
        "PASS shutdown: worker joined in {:?}",
        shutdown_started.elapsed()
    );

    // Drop alone must stop a live helper without a Shutdown command.
    let handle = start()?;
    handle.send(PlaybackCommand::SetVolume(0.0))?;
    handle.send(PlaybackCommand::Load {
        source: source.clone(),
        generation: 1,
    })?;
    wait_for(&handle, timeout, "live drop setup", |s| {
        !s.loading && s.position > 0.05
    })?;
    let drop_started = Instant::now();
    drop(handle);
    if drop_started.elapsed() > Duration::from_secs(3) {
        return Err("Live handle drop exceeded its deadline".into());
    }
    println!(
        "PASS live drop: worker joined in {:?}",
        drop_started.elapsed()
    );

    let handle = start()?;
    handle.send(PlaybackCommand::SetVolume(0.0))?;
    handle.send(PlaybackCommand::Load {
        source,
        generation: 1,
    })?;
    thread::sleep(Duration::from_millis(10));
    let drop_started = Instant::now();
    drop(handle);
    if drop_started.elapsed() > Duration::from_secs(3) {
        return Err("Startup handle drop exceeded its deadline".into());
    }
    println!(
        "PASS startup drop: worker joined in {:?}",
        drop_started.elapsed()
    );

    let handle = start()?;
    handle.send(PlaybackCommand::SetVolume(0.0))?;
    let drop_started = Instant::now();
    drop(handle);
    if drop_started.elapsed() > Duration::from_secs(3) {
        return Err("Idle handle drop exceeded its deadline".into());
    }
    println!("PASS idle drop: no helper needed");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}
