use std::time::{Duration, Instant};

/// One bounded recommendation request sequence for a playback generation.
#[derive(Default)]
pub(crate) struct RadioRequests {
    generation: Option<u64>,
    attempts: u8,
    retry_at: Option<Instant>,
    complete: bool,
    filled: bool,
}

impl RadioRequests {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn ready(&self, generation: u64, now: Instant) -> bool {
        self.generation != Some(generation)
            || (!self.complete && self.attempts < 3 && self.retry_at.is_some_and(|at| now >= at))
    }

    pub(crate) fn started(&mut self, generation: u64) {
        if self.generation != Some(generation) {
            self.reset();
            self.generation = Some(generation);
        }
        self.attempts += 1;
        self.retry_at = None;
    }

    pub(crate) fn failed(&mut self, now: Instant) -> bool {
        if self.attempts >= 3 {
            self.complete = true;
            return false;
        }
        self.retry_at = Some(now + Duration::from_secs(10 * u64::from(self.attempts.max(1))));
        true
    }

    pub(crate) fn succeeded(&mut self) {
        self.complete = true;
        self.filled = true;
        self.retry_at = None;
    }

    pub(crate) fn queue_depleted(&mut self, generation: u64) {
        // Removing upcoming rows can exhaust a previously successful refill
        // without advancing playback generation. Never reopen failed/exhausted
        // attempts here: those still need bounded backoff or explicit Retry.
        if self.generation == Some(generation) && self.complete && self.filled {
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
