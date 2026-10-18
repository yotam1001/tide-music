use std::time::{Duration, Instant};

use super::{active_is_current, PendingLoad, PlaybackIntent, PlayerState};
use serde_json::Value;

pub(super) const LOADING_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

/// Kept across mpv sessions, but never across an explicit replacement or Stop.
pub(super) struct Track {
    pub load: PendingLoad,
    pub retry_at: Option<Instant>,
    pub position: f64,
    pub duration: f64,
    pub loaded: bool,
    pub seek_revision: u64,
    attempts: usize,
    loading_since: Instant,
    progress_since: Instant,
    pending_seek: Option<f64>,
    stable_progress: f64,
}

impl Track {
    pub fn new(load: PendingLoad, now: Instant) -> Self {
        Self {
            load,
            retry_at: Some(now),
            position: 0.0,
            duration: 0.0,
            loaded: false,
            seek_revision: 0,
            attempts: 0,
            loading_since: now,
            progress_since: now,
            pending_seek: None,
            stable_progress: 0.0,
        }
    }

    pub fn retry_attempt(&self) -> u8 {
        self.attempts.min(2) as u8
    }

    pub fn session_started(&mut self, now: Instant) {
        self.retry_at = None;
        self.loaded = false;
        self.loading_since = now;
        self.progress_since = now;
        self.stable_progress = 0.0;
    }

    pub fn file_loaded(&mut self, now: Instant) {
        self.loaded = true;
        self.progress_since = now;
    }

    pub fn observe_position(&mut self, position: f64, now: Instant) {
        // A delayed observation from before a seek must not erase its target
        // if the IPC connection fails before the seek is confirmed.
        if let Some(target) = self.pending_seek {
            if (position - target).abs() > 2.0 {
                return;
            }
            self.pending_seek = None;
            self.position = position;
            self.progress_since = now;
            return;
        }
        let forward = position - self.position;
        // mpv can reset time-pos immediately before EOF or at startup. Only
        // explicit seek intent may move a confirmed resume position backwards.
        if forward < -1.0 {
            return;
        }
        if forward > 0.01 {
            self.progress_since = now;
            if self.loaded && forward < 5.0 {
                self.stable_progress += forward;
                if self.stable_progress >= 60.0 {
                    self.attempts = 0;
                    self.stable_progress = 0.0;
                }
            } else {
                self.stable_progress = 0.0;
            }
        }
        self.position = position;
    }

    pub fn apply_seek(&mut self, intent: &PlaybackIntent, now: Instant) -> Option<f64> {
        if intent.seek_revision == self.seek_revision {
            return None;
        }
        self.seek_revision = intent.seek_revision;
        self.position = intent.seek?;
        self.pending_seek = Some(self.position);
        self.stable_progress = 0.0;
        self.progress_since = now;
        Some(self.position)
    }

    pub fn timed_out(&mut self, paused: bool, now: Instant) -> bool {
        if self.retry_at.is_some() {
            return false;
        }
        if !self.loaded {
            return now.duration_since(self.loading_since) >= LOADING_TIMEOUT;
        }
        if paused {
            self.progress_since = now;
            self.stable_progress = 0.0;
            return false;
        }
        now.duration_since(self.progress_since) >= STALL_TIMEOUT
    }

    pub fn schedule_retry(&mut self, now: Instant) -> bool {
        let Some(delay) = RETRY_DELAYS.get(self.attempts) else {
            return false;
        };
        self.attempts += 1;
        self.stable_progress = 0.0;
        self.load.prepared = None; // refresh expired addresses through the original source
        self.retry_at = Some(now + *delay);
        true
    }
}

/// mpv EOF alone is insufficient to infer completion of a known finite track.
/// Unknown-duration/live sources retain ordinary EOF semantics.
pub(super) fn premature_eof(position: f64, duration: f64) -> bool {
    duration.is_finite()
        && duration > 0.0
        && position.is_finite()
        && duration - position > 3.0_f64.max(duration * 0.01)
}

// Compare only mpv's fixed error enum text, never logs or arbitrary URL text.
// The caller must first validate the session, generation and playlist entry.
pub(super) fn output_failure(message: &Value) -> bool {
    message
        .get("file_error")
        .and_then(Value::as_str)
        .or_else(|| message.get("error").and_then(Value::as_str))
        == Some("audio output initialization failed")
}

pub(super) fn message_failed(
    state: &PlayerState,
    track: &Track,
    message: &Value,
    generation: u64,
    epoch: u64,
) -> bool {
    if track.load.generation != generation || track.load.epoch != epoch {
        return false;
    }
    let rejected = message
        .get("request_id")
        .and_then(Value::as_i64)
        .is_some_and(|id| state.requests.contains_key(&id))
        && message
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error != "success");
    let end = active_is_current(state, generation, epoch)
        && message.get("event").and_then(Value::as_str) == Some("end-file")
        && message.get("playlist_entry_id").and_then(Value::as_u64) == state.active_entry_id;
    rejected
        || (end
            && match message.get("reason").and_then(Value::as_str) {
                Some("error") => true,
                Some("eof") => premature_eof(track.position, track.duration),
                _ => false,
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(now: Instant) -> Track {
        Track::new(
            PendingLoad {
                source: "owned local fixture".into(),
                generation: 2,
                epoch: 3,
                prepared: None,
            },
            now,
        )
    }
    #[test]
    fn classifies_only_the_fixed_audio_failure_on_the_current_entry() {
        let track = track(Instant::now());
        let state = PlayerState {
            active_generation: Some(2),
            active_epoch: Some(3),
            active_entry_id: Some(7),
            ..Default::default()
        };
        let mut message = serde_json::json!({"event":"end-file", "reason":"error", "playlist_entry_id":7, "file_error":"audio output initialization failed"});
        assert!(output_failure(&message) && message_failed(&state, &track, &message, 2, 3));
        assert!(!message_failed(&state, &track, &message, 3, 4));
        message["playlist_entry_id"] = serde_json::json!(8);
        assert!(!message_failed(&state, &track, &message, 2, 3));
        for error in [
            "loading failed",
            "private signed source",
            "audio output initialization failed in arbitrary log text",
        ] {
            assert!(!output_failure(&serde_json::json!({"file_error":error})));
        }
        assert!(!output_failure(
            &serde_json::json!({"event":"end-file", "reason":"eof"})
        ));
    }
    #[test]
    fn bounded_retries_survive_progress_and_refresh_original_source() {
        let now = Instant::now();
        let mut track = track(now);
        assert!(track.schedule_retry(now));
        assert_eq!(track.retry_at, Some(now + Duration::from_secs(1)));
        track.session_started(now);
        track.file_loaded(now);
        track.observe_position(30.0, now);
        assert!(track.schedule_retry(now));
        assert_eq!(track.position, 30.0);
        assert_eq!(track.retry_at, Some(now + Duration::from_secs(3)));
        assert!(!track.schedule_retry(now));
    }
    #[test]
    fn watchdog_ignores_paused_and_backoff_but_bounds_loading_and_stalls() {
        let now = Instant::now();
        let mut track = track(now);
        assert!(!track.timed_out(false, now + LOADING_TIMEOUT));
        track.session_started(now);
        assert!(!track.timed_out(true, now + LOADING_TIMEOUT - Duration::from_secs(1)));
        assert!(track.timed_out(true, now + LOADING_TIMEOUT));
        track.file_loaded(now);
        assert!(!track.timed_out(true, now + STALL_TIMEOUT));
        assert!(!track.timed_out(false, now + STALL_TIMEOUT + Duration::from_secs(1)));
        assert!(track.timed_out(false, now + STALL_TIMEOUT * 2));
    }
    #[test]
    fn seek_intent_supersedes_resume_once_and_resets_progress_deadline() {
        let now = Instant::now();
        let mut track = track(now);
        track.observe_position(50.0, now);
        let intent = PlaybackIntent {
            epoch: 3,
            paused: true,
            seek: Some(9.0),
            seek_revision: 1,
        };
        assert_eq!(track.apply_seek(&intent, now), Some(9.0));
        assert_eq!(track.apply_seek(&intent, now), None);
        assert_eq!(track.position, 9.0);
        track.observe_position(50.1, now);
        assert_eq!(
            track.position, 9.0,
            "stale observations must not erase a seek target"
        );
        track.observe_position(9.1, now);
        assert_eq!(track.position, 9.1);
    }
    #[test]
    fn natural_and_unknown_eof_do_not_replay_but_early_finite_eof_recovers() {
        assert!(!premature_eof(98.0, 100.0));
        assert!(!premature_eof(10.0, 0.0));
        assert!(!premature_eof(10.0, f64::INFINITY));
        assert!(premature_eof(30.0, 100.0));
    }

    #[test]
    fn stable_forward_playback_resets_budget_but_seek_pause_and_short_progress_do_not() {
        let now = Instant::now();
        let mut track = track(now);
        assert!(track.schedule_retry(now));
        assert!(track.schedule_retry(now));
        track.session_started(now);
        track.file_loaded(now);
        for second in 1..=59 {
            track.observe_position(second as f64, now + Duration::from_secs(second));
        }
        assert_eq!(track.attempts, 2);
        track.timed_out(true, now + Duration::from_secs(60));
        track.observe_position(60.0, now + Duration::from_secs(61));
        assert_eq!(track.attempts, 2);
        let intent = PlaybackIntent {
            epoch: 3,
            paused: false,
            seek: Some(500.0),
            seek_revision: 1,
        };
        track.apply_seek(&intent, now);
        track.observe_position(500.0, now);
        assert_eq!(track.attempts, 2);
        for second in 1..=60 {
            track.observe_position(500.0 + second as f64, now + Duration::from_secs(second));
        }
        assert_eq!(track.attempts, 0);
        assert!(track.schedule_retry(now));
        assert!(track.schedule_retry(now));
        assert!(!track.schedule_retry(now));
    }

    #[test]
    fn streamed_read_error_and_premature_eof_recover_only_current_matching_entry() {
        let mut track = track(Instant::now());
        track.position = 30.0;
        track.duration = 100.0;
        let state = PlayerState {
            active_generation: Some(2),
            active_epoch: Some(3),
            active_entry_id: Some(7),
            ..PlayerState::default()
        };
        let error = serde_json::json!({"event":"end-file","reason":"error", "playlist_entry_id":7,
            "file_error":"private signed address must never appear in diagnostics"});
        assert!(message_failed(&state, &track, &error, 2, 3));
        assert!(!message_failed(&state, &track, &error, 3, 4));
        assert!(!message_failed(
            &state,
            &track,
            &serde_json::json!({"event":"end-file", "reason":"error", "playlist_entry_id":6}),
            2,
            3
        ));
        let eof = serde_json::json!({"event":"end-file","reason":"eof", "playlist_entry_id":7});
        assert!(message_failed(&state, &track, &eof, 2, 3));
        track.position = 97.0;
        assert!(!message_failed(&state, &track, &eof, 2, 3));
        track.duration = 0.0;
        assert!(!message_failed(&state, &track, &eof, 2, 3));
    }
}
