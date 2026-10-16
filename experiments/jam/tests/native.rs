//! Explicit opt-in, two real muted mpv workers; no account/session/SMTC access.
#![cfg(windows)]
mod common;
#[path = "common/link.rs"]
mod link;
use common::Relay;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tide_core::PlaybackCommand;
use tide_jam_lab::{
    client::Client,
    protocol::*,
    sync::{Action, Follower, Local},
};
use tide_playback::PlaybackHandle;

fn start_player() -> PlaybackHandle {
    #[cfg(feature = "headless-native")]
    {
        tide_playback::start_null_audio_for_lab().unwrap()
    }
    #[cfg(not(feature = "headless-native"))]
    {
        tide_playback::start().unwrap()
    }
}

fn milliseconds(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        (value * 1000.0) as u64
    } else {
        0
    }
}
fn local(engine: &PlaybackHandle) -> Local {
    let s = engine.snapshot();
    Local {
        generation: s.generation,
        position_ms: milliseconds(s.position),
        duration_ms: milliseconds(s.duration),
        paused: s.paused,
        ready: !s.loading && !s.ended && s.error.is_none() && s.duration > 0.0,
    }
}
struct Harness {
    _relay: Relay,
    link: Option<link::Link>,
    client: Client,
    guest_client: Client,
    room: Created,
    guest: Secret,
    host: PlaybackHandle,
    follower: PlaybackHandle,
    controller: Follower,
    fixture: PathBuf,
    started: Instant,
    published: Option<Instant>,
    revision: u64,
    play_id: u64,
    seeks: usize,
}
impl Harness {
    fn tick(&mut self, publish: bool) {
        let now = Instant::now();
        if publish
            && self
                .published
                .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(250))
        {
            let s = self.host.snapshot();
            self.revision += 1;
            self.client
                .publish(
                    &self.room.room,
                    &self.room.host,
                    &Timeline {
                        revision: self.revision,
                        play_id: self.play_id,
                        queue: vec![
                            VideoId::parse("abcdefghijk").unwrap(),
                            VideoId::parse("lmnopqrstuv").unwrap(),
                        ],
                        index: usize::from(self.play_id > 2),
                        position_ms: if s.generation == self.play_id {
                            milliseconds(s.position)
                        } else {
                            0
                        },
                        paused: s.paused,
                        loading: s.loading || s.generation != self.play_id,
                    },
                )
                .unwrap();
            self.published = Some(now);
        }
        let request = Instant::now();
        let actions = match self.guest_client.poll(&self.room.room, &self.guest) {
            Ok(view) => self
                .controller
                .decide(
                    &view,
                    request.elapsed().as_millis() as u64,
                    &local(&self.follower),
                    self.started.elapsed().as_millis() as u64,
                )
                .unwrap(),
            Err(tide_jam_lab::client::Error::Transport) => self.controller.network_failure(
                &local(&self.follower),
                self.started.elapsed().as_millis() as u64,
            ),
            Err(error) => panic!("Unexpected native fixture transport failure: {error}"),
        };
        self.apply(actions);
        std::thread::sleep(Duration::from_millis(100));
    }
    fn apply(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Load { video, generation } => {
                    assert!(matches!(video.as_str(), "abcdefghijk" | "lmnopqrstuv"));
                    // Test-only mapping: neither the wire nor the runnable lab accepts local paths.
                    self.follower
                        .send(PlaybackCommand::Load {
                            source: self.fixture.to_string_lossy().into_owned(),
                            generation,
                        })
                        .unwrap();
                }
                Action::Seek(ms) => {
                    self.seeks += 1;
                    self.follower
                        .send(PlaybackCommand::Seek(ms as f64 / 1000.0))
                        .unwrap();
                }
                Action::Pause(paused) => self
                    .follower
                    .send(PlaybackCommand::SetPaused(paused))
                    .unwrap(),
            }
        }
    }
    fn phase(&mut self, label: &str, publish: bool, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut stable = 0;
        while Instant::now() < deadline {
            self.tick(publish);
            if predicate(self) {
                stable += 1;
            } else {
                stable = 0;
            }
            if stable >= 4 {
                println!("PASS native Jam {label}; sampled drift {} ms", self.drift());
                return;
            }
        }
        panic!(
            "Native Jam phase failed: {label}; drift {} ms, host ready={}, guest ready={}, host error={:?}, guest error={:?}, host duration={}, guest duration={}",
            self.drift(),
            local(&self.host).ready,
            local(&self.follower).ready,
            self.host.snapshot().error,
            self.follower.snapshot().error,
            self.host.snapshot().duration,
            self.follower.snapshot().duration
        );
    }
    fn drift(&self) -> u64 {
        local(&self.host)
            .position_ms
            .abs_diff(local(&self.follower).position_ms)
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.client.close(&self.room.room, &self.room.host);
    }
}
fn harness(delayed: bool) -> Harness {
    let fixture = PathBuf::from(
        std::env::var_os("JAM_NATIVE_FIXTURE").expect("Set a silent local WAV fixture"),
    );
    assert!(fixture.is_file());
    let relay = Relay::start();
    let link = delayed.then(|| link::Link::start(&relay.endpoint));
    let client = Client::new(&relay.endpoint).unwrap();
    let room = client.create().unwrap();
    let guest = client
        .join(&Invite {
            endpoint: relay.endpoint.clone(),
            room: room.room.clone(),
            invite: room.invite.clone(),
        })
        .unwrap()
        .member;
    let host = start_player();
    let follower = start_player();
    host.send(PlaybackCommand::SetVolume(0.0)).unwrap();
    follower.send(PlaybackCommand::SetVolume(0.0)).unwrap();
    host.send(PlaybackCommand::Load {
        source: fixture.to_string_lossy().into_owned(),
        generation: 1,
    })
    .unwrap();
    Harness {
        guest_client: Client::new(link.as_ref().map_or(&relay.endpoint, |link| &link.endpoint))
            .unwrap(),
        link,
        _relay: relay,
        client,
        room,
        guest,
        host,
        follower,
        controller: Follower::default(),
        fixture,
        started: Instant::now(),
        published: None,
        revision: 0,
        play_id: 1,
        seeks: 0,
    }
}
#[test]
#[ignore = "Requires explicit JAM_NATIVE_FIXTURE and mpv; runs two muted native players"]
fn two_native_players_follow_pause_seek_repeat_skip_stale_host_and_close() {
    println!(
        "Native Jam fixture output: {}",
        if cfg!(feature = "headless-native") {
            "explicit null-audio test sink"
        } else {
            "normal system audio, muted"
        }
    );
    let mut h = harness(false);
    h.phase("late join", true, |h| {
        local(&h.host).ready
            && local(&h.follower).ready
            && !local(&h.follower).paused
            && h.drift() < 600
    });
    h.follower.send(PlaybackCommand::Seek(8.0)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while local(&h.follower).position_ms < 7500 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        h.drift() > 2000,
        "The native drift injection must actually take effect"
    );
    h.phase("drift correction", true, |h| {
        !local(&h.follower).paused && h.drift() < 600
    });
    h.host.send(PlaybackCommand::SetPaused(true)).unwrap();
    h.phase("host pause", true, |h| {
        local(&h.host).paused && local(&h.follower).paused && h.drift() < 600
    });
    h.host.send(PlaybackCommand::Seek(12.0)).unwrap();
    h.phase("paused host seek", true, |h| {
        local(&h.host).position_ms >= 11_500 && local(&h.follower).paused && h.drift() < 600
    });
    h.host.send(PlaybackCommand::SetPaused(false)).unwrap();
    h.phase("host resume", true, |h| {
        !local(&h.host).paused && !local(&h.follower).paused && h.drift() < 600
    });
    for play_id in [2, 3] {
        h.play_id = play_id;
        h.host
            .send(PlaybackCommand::Load {
                source: h.fixture.to_string_lossy().into_owned(),
                generation: play_id,
            })
            .unwrap();
        h.phase(
            if play_id == 2 {
                "same ID new play generation"
            } else {
                "next queue track"
            },
            true,
            |h| {
                local(&h.host).generation == play_id
                    && local(&h.follower).generation == play_id
                    && local(&h.host).ready
                    && local(&h.follower).ready
                    && !local(&h.follower).paused
                    && h.drift() < 600
            },
        );
    }
    h.phase("stale host failsafe", false, |h| {
        local(&h.follower).paused && !local(&h.host).paused
    });
    h.phase("host heartbeat recovery", true, |h| {
        !local(&h.follower).paused && h.drift() < 600
    });
    h.client.close(&h.room.room, &h.room.host).unwrap();
    assert!(h.client.poll(&h.room.room, &h.guest).is_err());
    let actions = h.controller.disconnect();
    h.apply(actions);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !local(&h.follower).paused && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(local(&h.follower).paused);
    assert!(
        h.seeks <= 12,
        "Corrections must remain bounded, not seek continuously"
    );
    assert_eq!(h.host.snapshot().volume, 0.0);
    assert_eq!(h.follower.snapshot().volume, 0.0);
    println!(
        "PASS native Jam room close; {} total bounded seeks; both players muted",
        h.seeks
    );
}

#[test]
#[ignore = "Requires explicit JAM_NATIVE_FIXTURE and mpv; runs two muted native players"]
fn two_native_players_recover_from_real_dropped_requests_and_delayed_polling() {
    use std::sync::atomic::Ordering;
    let mut h = harness(true);
    h.link
        .as_ref()
        .unwrap()
        .delay_ms
        .store(250, Ordering::Release);
    h.phase("delayed guest late join", true, |h| {
        local(&h.host).ready
            && local(&h.follower).ready
            && !local(&h.follower).paused
            && h.drift() < 600
    });
    h.link
        .as_ref()
        .unwrap()
        .outage
        .store(true, Ordering::Release);
    h.phase("real dropped poll requests pause guest", true, |h| {
        local(&h.follower).paused && !local(&h.host).paused
    });
    assert!(h.link.as_ref().unwrap().dropped.load(Ordering::Relaxed) > 0);
    h.link
        .as_ref()
        .unwrap()
        .outage
        .store(false, Ordering::Release);
    h.phase("same membership recovers after outage", true, |h| {
        !local(&h.follower).paused && h.drift() < 600
    });
    for play_id in 2..=7 {
        // Same video again, alternating the link delay each time. No account,
        // browser session or remote audio is used in this fixture.
        h.link
            .as_ref()
            .unwrap()
            .delay_ms
            .store(if play_id % 2 == 0 { 75 } else { 250 }, Ordering::Release);
        h.play_id = play_id;
        h.host
            .send(PlaybackCommand::Load {
                source: h.fixture.to_string_lossy().into_owned(),
                generation: play_id,
            })
            .unwrap();
        h.phase("repeated native handoff over delayed link", true, |h| {
            local(&h.host).generation == play_id
                && local(&h.follower).generation == play_id
                && local(&h.host).ready
                && local(&h.follower).ready
                && !local(&h.follower).paused
                && h.drift() < 600
        });
    }
    assert!(
        h.seeks <= 20,
        "The delayed link must not cause a continuous seek loop"
    );
    assert_eq!(h.host.snapshot().volume, 0.0);
    assert_eq!(h.follower.snapshot().volume, 0.0);
    println!("PASS native delayed-link recovery; seven plays, {} dropped requests, {} seeks; both players muted", h.link.as_ref().unwrap().dropped.load(Ordering::Relaxed), h.seeks);
}
