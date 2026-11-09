use std::time::{Duration, Instant};

/// One bounded recommendation request sequence for a playback generation.
#[derive(Default)]
pub(crate) struct RadioRequests {
    generation: Option<u64>,
    phase: Phase,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Fetching {
        attempt: u8,
    },
    Waiting {
        attempt: u8,
        retry_at: Instant,
    },
    Filled,
    Exhausted,
}

impl RadioRequests {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn ready(&self, generation: u64, now: Instant) -> bool {
        self.generation != Some(generation)
            || matches!(self.phase, Phase::Waiting { retry_at, .. } if now >= retry_at)
    }

    pub(crate) fn started(&mut self, generation: u64) {
        let attempt = if self.generation == Some(generation) {
            match self.phase {
                Phase::Fetching { attempt } | Phase::Waiting { attempt, .. } => {
                    attempt.saturating_add(1)
                }
                _ => 1,
            }
        } else {
            1
        };
        self.generation = Some(generation);
        self.phase = Phase::Fetching { attempt };
    }

    pub(crate) fn failed(&mut self, now: Instant) -> bool {
        let attempt = match self.phase {
            Phase::Fetching { attempt } | Phase::Waiting { attempt, .. } => attempt,
            Phase::Exhausted => return false,
            _ => 0,
        };
        if attempt >= 3 {
            self.phase = Phase::Exhausted;
            return false;
        }
        self.phase = Phase::Waiting {
            attempt,
            retry_at: now + Duration::from_secs(10 * u64::from(attempt.max(1))),
        };
        true
    }

    pub(crate) fn succeeded(&mut self) {
        self.phase = Phase::Filled;
    }

    pub(crate) fn queue_depleted(&mut self, generation: u64) {
        // Only successful refills may be reopened without a new generation.
        // Exhausted requests require an explicit retry or a different track.
        if self.generation == Some(generation) && matches!(self.phase, Phase::Filled) {
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_edits_reopen_successful_refills_but_preserve_failure_backoff() {
        let now = Instant::now();
        let mut requests = RadioRequests::default();
        requests.started(7);
        requests.failed(now);
        requests.queue_depleted(7);
        assert!(!requests.ready(7, now + Duration::from_secs(9)));
        requests.started(7);
        requests.failed(now);
        requests.started(7);
        assert!(!requests.failed(now));
        requests.queue_depleted(7);
        assert!(!requests.ready(7, now + Duration::from_secs(3600)));
        requests.reset();
        requests.started(7);
        requests.succeeded();
        requests.queue_depleted(8);
        assert!(!requests.ready(7, now));
        requests.queue_depleted(7);
        assert!(requests.ready(7, now));
    }

    #[test]
    fn retries_wait_and_stop_after_three_attempts_but_new_tracks_can_request() {
        let mut requests = RadioRequests::default();
        let now = Instant::now();
        assert!(requests.ready(1, now));
        requests.started(1);
        assert!(!requests.ready(1, now + Duration::from_secs(60)));
        assert!(requests.failed(now));
        assert!(!requests.ready(1, now + Duration::from_secs(9)));
        assert!(requests.ready(1, now + Duration::from_secs(10)));
        requests.started(1);
        assert!(requests.failed(now));
        assert!(!requests.ready(1, now + Duration::from_secs(19)));
        assert!(requests.ready(1, now + Duration::from_secs(20)));
        requests.started(1);
        assert!(!requests.failed(now));
        assert!(!requests.ready(1, now + Duration::from_secs(3600)));
        assert!(requests.ready(2, now));
        requests.started(2);
        requests.succeeded();
        assert!(!requests.ready(2, now + Duration::from_secs(3600)));
        requests.reset();
        assert!(requests.ready(2, now));
    }
}
