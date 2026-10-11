//! Opt-in muted integration check; never serialize private track metadata.
use super::*;

pub(super) fn start(
    state: Rc<RefCell<State>>,
    ui: &AppWindow,
    seconds: u64,
    local_source: Option<String>,
) -> (slint::Timer, Rc<RefCell<bool>>) {
    let timer = slint::Timer::default();
    let passed = Rc::new(RefCell::new(false));
    let result = passed.clone();
    let weak = ui.as_weak();
    let started = Instant::now();
    let mut playback_started = None::<Instant>;
    let mut previous = tide_core::PlaybackSnapshot::default();
    let mut last_progress = Instant::now();
    let mut natural_transitions = 0;
    let mut recommendations_seen = false;
    let mut pause_at = None::<Instant>;
    let mut pause_done = false;
    let mut pause_observed = false;
    let mut skip_done = false;
    let mut skip_generation = None;
    let mut seek_done = false;
    let mut last_minute = 0;
    let local = local_source.is_some();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
        let Some(u) = weak.upgrade() else { return; };
        let mut s = state.borrow_mut();
        let fail = |message: &str| {
            eprintln!("Listening verification failed: {message}");
            let _ = slint::quit_event_loop();
        };
        if playback_started.is_none() {
            if local || (s.account_verified && s.library_worker.is_none()) {
                let source = local_source.clone().unwrap_or_else(|| "https://www.youtube.com/watch?v=K4DyBUG242c".into());
                if local {
                    s.settings.autoplay = false;
                    s.settings.repeat = true;
                    for index in 0..3 {
                        s.settings.queue.tracks.push(Track {
                            title: format!("Local verification {}", index + 1),
                            artist: String::new(), album: String::new(), source: source.clone(),
                            artwork_url: None, recommended: false,
                        });
                    }
                    s.settings.queue.current = Some(0);
                }
                if !(if local { play(&mut s, &u) } else { add(&mut s, &u, source) }) {
                    fail("could not start muted source"); return;
                }
                playback_started = Some(Instant::now());
                last_progress = Instant::now();
            } else if started.elapsed() > Duration::from_secs(90) {
                fail("account verification did not finish");
            }
            return;
        }
        let Some(engine) = s.engine.as_ref() else { fail("engine unavailable"); return; };
        let p = engine.snapshot();
        if p.volume != 0.0 { fail("probe must remain muted"); return; }
        if p.error.is_some() { fail("terminal playback error"); return; }
        if !local && (s.account_error || !s.account_verified) { fail("account access failed"); return; }
        recommendations_seen |= s.settings.queue.tracks.iter().any(|t| t.recommended);
        if p.generation != previous.generation {
            if previous.generation > 0 && skip_generation != Some(p.generation)
                && previous.duration > 0.0 && previous.duration - previous.position <= 3.0 {
                natural_transitions += 1;
                println!("Listening check: natural transition {natural_transitions}");
            }
            last_progress = Instant::now();
        }
        if (p.position - previous.position).abs() > 0.01 || p.paused {
            last_progress = Instant::now();
        }
        if last_progress.elapsed() > Duration::from_secs(100) {
            fail("no decoded progress within recovery bound"); return;
        }
        let elapsed = playback_started.unwrap().elapsed();
        if pause_at.is_some() && p.paused {
            pause_observed = true;
        }
        if pause_done && pause_at.is_none() && elapsed > Duration::from_secs(55) && p.paused && !p.loading && !p.ended {
            fail("playback remained paused after resume"); return;
        }
        if !pause_done && elapsed > Duration::from_secs(45) && !p.loading && p.position > 2.0 {
            send(&mut s, &u, PlaybackCommand::SetPaused(true));
            pause_done = true;
            pause_at = Some(Instant::now());
        } else if pause_at.is_some_and(|at| at.elapsed() > Duration::from_secs(3)) {
            if !pause_observed { fail("pause was not acknowledged"); return; }
            send(&mut s, &u, PlaybackCommand::SetPaused(false));
            pause_at = None;
        } else if !skip_done && elapsed > Duration::from_secs(90) && !p.loading {
            skip_done = true;
            skip_generation = Some(s.generation + 1);
            previous = p;
            drop(s);
            u.invoke_next_track();
            return;
        } else if !seek_done && elapsed > Duration::from_secs(120) && !p.loading && p.position > 5.0 {
            seek_done = true;
            send(&mut s, &u, PlaybackCommand::Seek(p.position - 3.0));
        }
        if !s.active_playback && !s.radio_resume_when_ready {
            fail("playback stopped without pending continuation"); return;
        }
        let minute = elapsed.as_secs() / 60;
        if minute > last_minute {
            last_minute = minute;
            println!("Listening check: {minute} minutes, {natural_transitions} natural transitions; decoded progress monitored");
        }
        if elapsed.as_secs() >= seconds {
            if natural_transitions >= (if local { 10 } else { 2 }) && (local || recommendations_seen) && pause_observed && skip_done && seek_done {
                *result.borrow_mut() = true;
                println!("PASS listening check: {seconds}s, {natural_transitions} natural transitions, {}, pause/resume, skip and seek", if local { "local queue" } else { "radio" });
                let _ = slint::quit_event_loop();
            } else { fail("insufficient transition or control coverage"); }
        }
        previous = p;
    });
    (timer, passed)
}
