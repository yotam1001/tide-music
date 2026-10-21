//! One retry per observed output return, scoped to the current playback generation.
use std::time::{Duration, Instant};
use tide_core::PlaybackSnapshot;

#[derive(Default)]
pub(crate) struct OutputRecovery {
    pending: Option<(u64, Instant)>,
}
impl OutputRecovery {
    pub fn changed(&mut self, available: bool, generation: u64, now: Instant) {
        self.pending = available.then_some((generation, now));
    }
    pub fn resume(
        &mut self,
        generation: u64,
        snapshot: &PlaybackSnapshot,
        now: Instant,
    ) -> Option<(f64, bool)> {
        let (expected, since) = self.pending?;
        if expected != generation
            || snapshot.generation != generation
            || now.duration_since(since) > Duration::from_secs(10)
        {
            self.pending = None;
            return None;
        }
        // Let the backend's bounded retries finish if the device-return event
        // arrives first. Never restart EOF, Stop or an unrelated network failure.
        if snapshot.loading
            || snapshot.ended
            || !snapshot.output_unavailable
            || snapshot.error.is_none()
        {
            return None;
        }
        self.pending = None;
        Some((snapshot.position.max(0.0), snapshot.paused))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn failed() -> PlaybackSnapshot {
        PlaybackSnapshot {
            generation: 7,
            position: 81.5,
            paused: true,
            error: Some("Audio output unavailable".into()),
            output_unavailable: true,
            ..Default::default()
        }
    }
    #[test]
    fn output_return_restores_position_and_pause_once_without_advancing_queue() {
        let now = Instant::now();
        let mut recovery = OutputRecovery::default();
        recovery.changed(true, 7, now);
        let mut view = failed();
        view.loading = true;
        assert!(recovery.resume(7, &view, now).is_none());
        view.loading = false;
        assert_eq!(
            recovery.resume(7, &view, now + Duration::from_secs(4)),
            Some((81.5, true))
        );
        assert!(recovery
            .resume(7, &view, now + Duration::from_secs(4))
            .is_none());
        recovery.changed(true, 7, now);
        view.paused = false;
        assert_eq!(recovery.resume(7, &view, now), Some((81.5, false)));
    }
    #[test]
    fn no_output_stale_events_stop_eof_and_non_audio_errors_never_resume() {
        let now = Instant::now();
        for mode in 0..6 {
            let mut recovery = OutputRecovery::default();
            recovery.changed(true, 7, now);
            let mut view = failed();
            let mut generation = 7;
            let mut at = now;
            match mode {
                0 => recovery.changed(false, 7, now),
                1 => generation = 8,
                2 => at += Duration::from_secs(11),
                3 => view.ended = true,
                4 => view.output_unavailable = false,
                _ => {
                    view = PlaybackSnapshot {
                        generation: 7,
                        ..Default::default()
                    };
                }
            }
            assert!(recovery.resume(generation, &view, at).is_none());
        }
    }
}
