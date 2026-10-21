//! Credits decoded progress, never elapsed wall time or seek distance.
use std::time::Instant;
use tide_core::PlaybackSnapshot;

#[derive(Default)]
pub struct Listening {
    run: Option<Run>,
}
struct Run {
    generation: u64,
    video: String,
    position: f64,
    sampled: Instant,
    seconds: f64,
    submitted: bool,
    playing: bool,
}
impl Listening {
    pub fn reset(&mut self) {
        self.run = None;
    }
    /// Call only with the current account's real, opted-in playback. Call reset
    /// when consent/account changes or when running any synthetic smoke probe.
    pub fn observe(
        &mut self,
        generation: u64,
        video: &str,
        p: &PlaybackSnapshot,
        now: Instant,
    ) -> Option<String> {
        if p.generation != generation || !p.position.is_finite() || !p.duration.is_finite() {
            return None;
        }
        if self
            .run
            .as_ref()
            .is_none_or(|r| r.generation != generation || r.video != video)
        {
            self.run = Some(Run {
                generation,
                video: video.into(),
                position: p.position,
                sampled: now,
                seconds: 0.0,
                submitted: false,
                playing: !p.paused && !p.loading && !p.buffering && !p.ended,
            });
            return None;
        }
        let run = self.run.as_mut().unwrap();
        let elapsed = now.saturating_duration_since(run.sampled).as_secs_f64();
        let progress = p.position - run.position;
        run.sampled = now;
        run.position = p.position;
        let playing = !p.paused && !p.loading && !p.buffering && !p.ended;
        let previously_playing = run.playing;
        run.playing = playing;
        // Long scheduling/sleep gaps and position discontinuities do not prove
        // continuous decoding. A stale/frozen progress value earns zero credit.
        if playing
            && previously_playing
            && elapsed > 0.0
            && elapsed <= 2.5
            && progress > 0.0
            && progress <= elapsed * 1.5 + 0.25
        {
            run.seconds += progress.min(elapsed);
        }
        let threshold = if p.duration > 0.0 {
            (p.duration * 0.5).clamp(5.0, 30.0)
        } else {
            30.0
        };
        (!run.submitted && run.seconds >= threshold).then(|| run.video.clone())
    }
    /// Mark at dispatch, not success: an uncertain write must never be replayed.
    pub fn submitted(&mut self, generation: u64) {
        if let Some(run) = &mut self.run {
            if run.generation == generation {
                run.submitted = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn snapshot(position: f64) -> PlaybackSnapshot {
        PlaybackSnapshot {
            generation: 1,
            position,
            duration: 240.0,
            paused: false,
            ..Default::default()
        }
    }
    #[test]
    fn actual_progress_qualifies_once_and_a_real_replay_is_separate() {
        let start = Instant::now();
        let mut history = Listening::default();
        for second in 0..30 {
            assert!(history
                .observe(
                    1,
                    "K4DyBUG242c",
                    &snapshot(second as f64),
                    start + Duration::from_secs(second)
                )
                .is_none());
        }
        assert_eq!(
            history
                .observe(
                    1,
                    "K4DyBUG242c",
                    &snapshot(30.0),
                    start + Duration::from_secs(30)
                )
                .as_deref(),
            Some("K4DyBUG242c")
        );
        // If the independent worker was busy, the qualified play remains ready.
        assert!(history
            .observe(
                1,
                "K4DyBUG242c",
                &snapshot(31.0),
                start + Duration::from_secs(31)
            )
            .is_some());
        history.submitted(1);
        assert!(history
            .observe(
                1,
                "K4DyBUG242c",
                &snapshot(32.0),
                start + Duration::from_secs(32)
            )
            .is_none());
        let mut replay = snapshot(0.0);
        replay.generation = 2;
        assert!(history
            .observe(2, "K4DyBUG242c", &replay, start + Duration::from_secs(33))
            .is_none());
        assert_eq!(history.run.as_ref().unwrap().seconds, 0.0);
    }
    #[test]
    fn seeks_paused_buffering_sleep_and_frozen_end_do_not_fake_listening() {
        let start = Instant::now();
        let mut history = Listening::default();
        let mut p = snapshot(0.0);
        history.observe(1, "K4DyBUG242c", &p, start);
        p.position = 200.0;
        assert!(history
            .observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(1))
            .is_none());
        for second in 2..8 {
            p.position += 1.0;
            p.paused = second < 4;
            p.buffering = (4..6).contains(&second);
            p.loading = second >= 6;
            assert!(history
                .observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(second))
                .is_none());
        }
        p = snapshot(240.0);
        p.ended = true;
        for second in 8..80 {
            assert!(history
                .observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(second))
                .is_none());
        }
        p = snapshot(0.0);
        history.observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(80));
        p.position = 60.0;
        history.observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(140));
        assert_eq!(history.run.as_ref().unwrap().seconds, 0.0);
        history.reset();
        assert!(history.run.is_none());
    }
    #[test]
    fn short_tracks_need_real_progress_and_old_generation_never_qualifies() {
        let start = Instant::now();
        let mut history = Listening::default();
        for second in 0..10 {
            let mut p = snapshot(second as f64);
            p.duration = 20.0;
            assert!(history
                .observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(second))
                .is_none());
        }
        assert!(history
            .observe(
                2,
                "K4DyBUG242c",
                &snapshot(200.0),
                start + Duration::from_secs(10)
            )
            .is_none());
        let mut p = snapshot(10.0);
        p.duration = 20.0;
        assert!(history
            .observe(1, "K4DyBUG242c", &p, start + Duration::from_secs(10))
            .is_some());
    }
}
