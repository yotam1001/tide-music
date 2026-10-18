//! Opt-in, muted local-file benchmark. Reports contain counts/timings only.
use super::*;

pub(super) fn start(
    state: Rc<RefCell<State>>,
    ui: &AppWindow,
    output: PathBuf,
    source: String,
) -> (slint::Timer, Rc<Cell<bool>>) {
    let timer = slint::Timer::default();
    let passed = Rc::new(Cell::new(false));
    let result = passed.clone();
    let weak = ui.as_weak();
    let began = Instant::now();
    let mut started = false;
    let mut generation = 0;
    let mut sampled = 0;
    let mut skip_generation = None;
    let mut skips = 0_u64;
    let mut natural = 0_u64;
    let mut delays = Vec::<u64>::new();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(u) = weak.upgrade() else { return; };
        let mut s = state.borrow_mut();
        let fail = |reason: &str| { eprintln!("Local performance verification failed: {reason}"); let _ = slint::quit_event_loop(); };
        if !started {
            s.settings.autoplay = false; s.settings.repeat = true;
            s.settings.queue = Queue { current: Some(0), tracks: (0..3).map(|_| Track {
                title: "Performance fixture".into(), artist: String::new(), artists: Vec::new(),
                album: String::new(), source: source.clone(), duration_seconds: None,
                artwork_url: None, recommended: false,
            }).collect() };
            if !play(&mut s, &u) { fail("could not start local fixture"); return; }
            started = true;
            return;
        }
        let Some(engine) = &s.engine else { fail("engine unavailable"); return; };
        let p = engine.snapshot();
        if p.error.is_some() { fail(if p.output_unavailable { "audio output unavailable" } else { "engine failure" }); return; }
        if p.volume != 0.0 { fail("playback was not muted"); return; }
        if s.generation != generation {
            if generation > 0 && skip_generation != Some(s.generation) { natural += 1; }
            generation = s.generation;
        }
        if p.generation == generation && sampled != generation && !p.loading && !p.paused && p.position > 0.05 {
            if let Some((_, at)) = s.load_requested_at.filter(|(requested, _)| *requested == generation) {
                if delays.len() >= 128 { fail("too many transition samples"); return; }
                delays.push(at.elapsed().as_millis().min(u64::MAX as u128) as u64);
                sampled = generation;
            }
        }
        let elapsed = began.elapsed();
        if skips < 3 && elapsed.as_secs() >= (skips + 1) * 6 && sampled == generation && !p.loading {
            skips += 1; skip_generation = Some(s.generation + 1);
            drop(s); u.invoke_next_track(); return;
        }
        if elapsed >= Duration::from_secs(45) { fail("timed out"); return; }
        if elapsed >= Duration::from_secs(30) && !p.loading {
            if natural < 4 || skips != 3 || delays.len() < 6 { fail("insufficient transition coverage"); return; }
            let report = serde_json::json!({"schema":1,"workload":"muted_local_3s_null_output",
                "elapsed_ms":elapsed.as_millis() as u64,"sampling_interval_ms":100,
                "natural_transitions":natural,"skips":skips,"request_to_progress_ms":delays});
            let saved = std::fs::OpenOptions::new().write(true).create_new(true).open(&output)
                .and_then(|file| serde_json::to_writer_pretty(file, &report).map_err(std::io::Error::other));
            if saved.is_err() { fail("could not create benchmark report"); return; }
            result.set(true);
            println!("PASS: muted local performance benchmark with natural transitions and repeated skips");
            let _ = slint::quit_event_loop();
        }
    });
    (timer, passed)
}
