//! Muted cold/prepared transition probe; only public sources, no account session.
use std::{
    thread,
    time::{Duration, Instant},
};
use tide_core::PlaybackCommand;
use tide_playback::PlaybackHandle;

fn loaded(handle: &PlaybackHandle, generation: u64) -> Result<Duration, String> {
    let started = Instant::now();
    loop {
        let snapshot = handle.snapshot();
        if let Some(error) = snapshot.error {
            return Err(error);
        }
        if snapshot.volume != 0.0 {
            return Err("Probe must remain muted".into());
        }
        if snapshot.generation == generation && !snapshot.loading && snapshot.position > 0.05 {
            return Ok(started.elapsed());
        }
        if started.elapsed() > Duration::from_secs(60) {
            return Err("Playback probe timed out".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn run() -> Result<(), String> {
    let first = std::env::args()
        .nth(1)
        .ok_or("Supply two public YouTube URLs")?;
    let next = std::env::args()
        .nth(2)
        .ok_or("Supply two public YouTube URLs")?;
    let handle = tide_playback::start()?;
    handle.send(PlaybackCommand::SetVolume(0.0))?;
    handle.send(PlaybackCommand::Load {
        source: first.clone(),
        generation: 1,
    })?;
    loaded(&handle, 1)?;

    handle.send(PlaybackCommand::Load {
        source: next.clone(),
        generation: 2,
    })?;
    let cold = loaded(&handle, 2)?;
    handle.send(PlaybackCommand::Load {
        source: first,
        generation: 3,
    })?;
    loaded(&handle, 3)?;

    let began = Instant::now();
    while !handle.next_is_prepared(&next) {
        handle.prepare_next(Some(&next))?;
        if began.elapsed() > Duration::from_secs(55) {
            return Err(
                "Next-song preparation did not succeed; normal loading remains available".into(),
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
    println!(
        "Prepared next source in {:.3}s",
        began.elapsed().as_secs_f64()
    );
    handle.send(PlaybackCommand::Load {
        source: next.clone(),
        generation: 4,
    })?;
    let prepared = loaded(&handle, 4)?;
    println!("Cold replacement {:.3}s; prepared replacement {:.3}s (includes mpv startup and first decoded audio)", cold.as_secs_f64(), prepared.as_secs_f64());

    // A pending extraction must be owned and cancelled before Drop returns.
    handle.prepare_next(Some(&next))?;
    thread::sleep(Duration::from_millis(100));
    let began = Instant::now();
    drop(handle);
    if began.elapsed() > Duration::from_secs(3) {
        return Err("Drop cleanup exceeded three seconds".into());
    }
    println!(
        "PASS Drop cancelled extraction and joined worker in {:.3}s",
        began.elapsed().as_secs_f64()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}
