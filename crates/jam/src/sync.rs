//! Drift correction uses the host's sample clock plus local elapsed time, not matching wall clocks.
use crate::jam::Playback;
use std::time::{Duration, Instant};

pub struct Target {
    pub playback: Playback,
    pub host_sent_ms: u64,
    pub received: Instant,
    pub transit_ms: u64,
}
impl Target {
    pub fn position(&self, now: Instant) -> u64 {
        let sample = &self.playback;
        if !sample.playing {
            return sample.position_ms;
        }
        sample
            .position_ms
            .saturating_add(
                self.host_sent_ms
                    .saturating_sub(sample.sampled_at_ms)
                    .min(5000),
            )
            .saturating_add(self.transit_ms.min(500))
            .saturating_add(now.saturating_duration_since(self.received).as_millis() as u64)
    }
    pub fn stale(&self, now: Instant) -> bool {
        self.host_sent_ms
            .saturating_sub(self.playback.sampled_at_ms)
            .saturating_add(now.saturating_duration_since(self.received).as_millis() as u64)
            > 5000
    }
}
pub struct Local<'a> {
    pub uri: &'a str,
    pub position_ms: u64,
    pub playing: bool,
    pub ready: bool,
}
#[derive(Debug, PartialEq)]
pub enum Correction {
    Load {
        uri: String,
        position_ms: u64,
        playing: bool,
    },
    Seek(u64),
    Play,
    Pause,
}
#[derive(Default)]
pub struct Follower {
    last_action: Option<Instant>,
    loading_uri: String,
    loads: u8,
}

impl Follower {
    pub fn exhausted(&self, uri: &str, now: Instant) -> bool {
        self.loading_uri == uri
            && self.loads >= 3
            && self
                .last_action
                .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_secs(3))
    }

    pub fn plan(&mut self, target: &Target, local: Local<'_>, now: Instant) -> Option<Correction> {
        if !local.ready || target.stale(now) {
            return None;
        }
        let sample = &target.playback;
        let position = target.position(now);
        if sample.uri != self.loading_uri {
            self.loading_uri = sample.uri.clone();
            self.loads = 0;
            self.last_action = None;
        }
        if self
            .last_action
            .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(3))
        {
            return None;
        }
        let correction = if sample.uri != local.uri {
            if self.loads >= 3 {
                return None;
            }
            self.loads += 1;
            Correction::Load {
                uri: sample.uri.clone(),
                position_ms: position,
                playing: sample.playing,
            }
        } else if sample.playing != local.playing {
            if sample.playing {
                Correction::Play
            } else {
                Correction::Pause
            }
        } else if position.abs_diff(local.position_ms) > 1500 {
            Correction::Seek(position)
        } else {
            return None;
        };
        self.last_action = Some(now);
        Some(correction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(now: Instant) -> Target {
        Target {
            playback: Playback {
                uri: "spotify:track:2JRo0gjbX4GrCqBYdRohoo".into(),
                title: "Track".into(),
                artist: "Artist".into(),
                position_ms: 10000,
                playing: true,
                sampled_at_ms: 900_000,
            },
            host_sent_ms: 900_100,
            received: now,
            transit_ms: 50,
        }
    }
    #[test]
    fn confirmed_cross_service_identifier_preserves_host_timing() {
        let now = Instant::now();
        let mut pairs = crate::matching::Pairings::default();
        pairs.set_room("room");
        pairs
            .remember(
                "youtube:abcdefghijk",
                "spotify:track:aaaaaaaaaaaaaaaaaaaaaa",
            )
            .unwrap();
        let mut remote = target(now);
        remote.playback.uri = "youtube:abcdefghijk".into();
        remote.playback.uri = pairs.get(&remote.playback.uri).unwrap().into();
        let mut follower = Follower::default();
        assert_eq!(
            follower.plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true
                },
                now
            ),
            Some(Correction::Load {
                uri: "spotify:track:aaaaaaaaaaaaaaaaaaaaaa".into(),
                position_ms: 10150,
                playing: true
            })
        );
        assert_eq!(remote.position(now + Duration::from_secs(2)), 12150);
        remote.playback.playing = false;
        assert_eq!(remote.position(now + Duration::from_secs(2)), 10000);
    }
    #[test]
    fn follows_track_without_requiring_matching_machine_clocks() {
        let now = Instant::now();
        let mut follower = Follower::default();
        let remote = target(now);
        assert_eq!(
            follower.plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true
                },
                now
            ),
            Some(Correction::Load {
                uri: remote.playback.uri.clone(),
                position_ms: 10150,
                playing: true
            })
        );
        assert_eq!(
            follower.plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true
                },
                now + Duration::from_secs(1)
            ),
            None
        );
    }
    #[test]
    fn corrects_drift_and_pause_but_ignores_stale_or_small_differences() {
        let now = Instant::now();
        let remote = target(now);
        assert_eq!(
            Follower::default().plan(
                &remote,
                Local {
                    uri: &remote.playback.uri,
                    position_ms: 10000,
                    playing: true,
                    ready: true
                },
                now
            ),
            None
        );
        assert_eq!(
            Follower::default().plan(
                &remote,
                Local {
                    uri: &remote.playback.uri,
                    position_ms: 5000,
                    playing: true,
                    ready: true
                },
                now
            ),
            Some(Correction::Seek(10150))
        );
        let mut paused = target(now);
        paused.playback.playing = false;
        assert_eq!(
            Follower::default().plan(
                &paused,
                Local {
                    uri: &paused.playback.uri,
                    position_ms: 10000,
                    playing: true,
                    ready: true
                },
                now
            ),
            Some(Correction::Pause)
        );
        assert_eq!(
            Follower::default().plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true
                },
                now + Duration::from_secs(6)
            ),
            None
        );
    }

    #[test]
    fn queue_updates_cannot_revive_an_old_playback_sample() {
        let now = Instant::now();
        let mut remote = target(now);
        remote.host_sent_ms = remote.playback.sampled_at_ms + 6000;
        assert!(remote.stale(now));
        assert_eq!(
            Follower::default().plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true,
                },
                now
            ),
            None
        );
        remote.host_sent_ms = remote.playback.sampled_at_ms + 4500;
        assert!(!remote.stale(now));
        assert!(remote.stale(now + Duration::from_millis(501)));
    }

    #[test]
    fn bounds_failed_loads_and_allows_a_new_track() {
        let now = Instant::now();
        let mut follower = Follower::default();
        for attempt in 0..3 {
            let tick = now + Duration::from_secs(attempt * 3);
            assert!(matches!(
                follower.plan(
                    &target(tick),
                    Local {
                        uri: "",
                        position_ms: 0,
                        playing: false,
                        ready: true,
                    },
                    tick
                ),
                Some(Correction::Load { .. })
            ));
        }
        let tick = now + Duration::from_secs(9);
        let mut remote = target(tick);
        assert!(follower.exhausted(&remote.playback.uri, tick));
        assert_eq!(
            follower.plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true,
                },
                tick
            ),
            None
        );
        remote.playback.uri = "spotify:track:3JRo0gjbX4GrCqBYdRohoo".into();
        assert!(matches!(
            follower.plan(
                &remote,
                Local {
                    uri: "",
                    position_ms: 0,
                    playing: false,
                    ready: true,
                },
                tick
            ),
            Some(Correction::Load { .. })
        ));
    }
}
