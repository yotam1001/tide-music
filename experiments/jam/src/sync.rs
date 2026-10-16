use crate::protocol::*;

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Load { video: VideoId, generation: u64 },
    Pause(bool),
    Seek(u64),
}
#[derive(Clone, Default)]
pub struct Local {
    pub generation: u64,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub paused: bool,
    pub ready: bool,
}

/// Monotonic local milliseconds only. Remote messages cannot set wall clocks.
pub struct Follower {
    active: bool,
    current: Option<(u64, VideoId)>,
    generation: u64,
    revision: u64,
    last_good: Option<u64>,
    last_correction: Option<u64>,
    initial_seek: bool,
}
impl Default for Follower {
    fn default() -> Self {
        Self {
            active: true,
            current: None,
            generation: 0,
            revision: 0,
            last_good: None,
            last_correction: None,
            initial_seek: false,
        }
    }
}
impl Follower {
    pub fn disconnect(&mut self) -> Vec<Action> {
        self.active = false;
        self.current = None;
        vec![Action::Pause(true)]
    }
    pub fn network_failure(&mut self, local: &Local, now: u64) -> Vec<Action> {
        if self.active
            && !local.paused
            && self
                .last_good
                .is_none_or(|last| now.saturating_sub(last) >= 3_000)
        {
            vec![Action::Pause(true)]
        } else {
            vec![]
        }
    }
    pub fn decide(
        &mut self,
        view: &View,
        rtt_ms: u64,
        local: &Local,
        now: u64,
    ) -> Result<Vec<Action>, &'static str> {
        view.validate()?;
        if !self.active {
            return Ok(vec![]);
        }
        if rtt_ms > 2_000 {
            return Ok(self.network_failure(local, now));
        }
        let Some(remote) = &view.timeline else {
            return Ok(if local.paused {
                vec![]
            } else {
                vec![Action::Pause(true)]
            });
        };
        if remote.revision < self.revision {
            return Ok(self.network_failure(local, now));
        }
        if view.age_ms.saturating_add(rtt_ms / 2) >= STALE_MS {
            return Ok(if local.paused {
                vec![]
            } else {
                vec![Action::Pause(true)]
            });
        }
        self.revision = remote.revision;
        self.last_good = Some(now);
        let video = remote.queue[remote.index].clone();
        if self.current.as_ref() != Some(&(remote.play_id, video.clone())) {
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or("Jam generation exhausted")?;
            self.current = Some((remote.play_id, video.clone()));
            self.last_correction = None;
            self.initial_seek = true;
            return Ok(vec![
                Action::Load {
                    video,
                    generation: self.generation,
                },
                Action::Pause(true),
            ]);
        }
        if local.generation != self.generation {
            return Ok(vec![]);
        }
        if !local.ready {
            return Ok(if (remote.loading || remote.paused) && !local.paused {
                vec![Action::Pause(true)]
            } else {
                vec![]
            });
        }
        if remote.loading {
            return Ok(if local.paused {
                vec![]
            } else {
                vec![Action::Pause(true)]
            });
        }
        let target = remote.position_ms.saturating_add(if remote.paused {
            0
        } else {
            view.age_ms + rtt_ms / 2
        });
        if target > MAX_POSITION_MS {
            return Err("Jam position out of bounds");
        }
        // Don't loop seeking to EOF when a participant has unavailable/shorter media.
        if local.duration_ms < 100 || target >= local.duration_ms.saturating_sub(100) {
            return Ok(if local.paused {
                vec![]
            } else {
                vec![Action::Pause(true)]
            });
        }
        let mut actions = Vec::new();
        if self.initial_seek
            || (target.abs_diff(local.position_ms) > 350
                && self
                    .last_correction
                    .is_none_or(|last| now.saturating_sub(last) >= 3_000))
        {
            actions.push(Action::Seek(target));
            self.last_correction = Some(now);
            self.initial_seek = false;
        }
        if local.paused != remote.paused {
            actions.push(Action::Pause(remote.paused));
        }
        Ok(actions)
    }
}
