//! Opt-in, metadata-only Discord activity. Latest-value slot bounds memory.
#[cfg(any(windows, test))]
use serde_json::{json, Value};
#[cfg(any(windows, test))]
use std::time::{Duration, Instant};
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub title: String,
    pub artist: String,
}
impl Activity {
    pub fn new(title: &str, artist: &str) -> Option<Self> {
        if title.trim().is_empty() {
            return None;
        }
        fn bounded(value: &str) -> String {
            value
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect()
        }
        Some(Self {
            title: bounded(title),
            artist: bounded(artist),
        })
    }
}
pub fn valid_id(id: &str) -> bool {
    (17..=20).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|v| v > 0)
}
#[cfg(any(windows, test))]
fn payload(activity: Option<&Activity>, nonce: u64) -> Value {
    json!({"cmd":"SET_ACTIVITY", "args":{"pid":std::process::id(), "activity":activity.map(|a| json!({"type":2,"details":a.title,"state":a.artist}))}, "nonce":nonce.to_string()})
}
#[derive(Default)]
struct Latest {
    activity: Option<Activity>,
    revision: u64,
    stop: bool,
}
struct Worker {
    shared: Arc<(Mutex<Latest>, Condvar)>,
    status: Arc<AtomicU8>,
    thread: Option<JoinHandle<()>>,
}
#[derive(Default)]
pub struct Presence {
    id: String,
    worker: Option<Worker>,
}
impl Presence {
    pub fn update(&mut self, enabled: bool, id: &str, activity: Option<Activity>) -> &'static str {
        if !enabled {
            self.worker.take();
            self.id.clear();
            return "Off — listening activity stays private";
        }
        if !valid_id(id) {
            self.worker.take();
            self.id.clear();
            return "Enter a Discord application ID to connect";
        }
        if self.id != id {
            self.worker.take();
            self.id = id.to_owned();
        }
        if self.worker.is_none() {
            self.worker = Worker::start(id.to_owned());
        }
        let Some(worker) = &self.worker else {
            return "Discord presence unavailable";
        };
        let (lock, wake) = &*worker.shared;
        if let Ok(mut latest) = lock.lock() {
            if latest.activity != activity {
                latest.activity = activity;
                latest.revision += 1;
                wake.notify_one();
            }
        }
        match worker.status.load(Ordering::Relaxed) {
            1 => "Connected to Discord desktop",
            _ => "Waiting for Discord desktop",
        }
    }
}
impl Worker {
    fn start(id: String) -> Option<Self> {
        let shared = Arc::new((Mutex::new(Latest::default()), Condvar::new()));
        let status = Arc::new(AtomicU8::new(0));
        let state = shared.clone();
        let report = status.clone();
        let thread = std::thread::Builder::new()
            .name("tide-discord".into())
            .spawn(move || run(id, state, report))
            .ok()?;
        Some(Self {
            shared,
            status,
            thread: Some(thread),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        if let Ok(mut state) = lock.lock() {
            state.stop = true;
            wake.notify_one();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(windows)]
fn request(
    pipe: &tide_windows::discord_pipe::DiscordPipe,
    opcode: u32,
    data: Value,
    nonce: Option<&str>,
) -> Result<(), ()> {
    let deadline = tide_windows::discord_pipe::DiscordPipe::deadline();
    pipe.send(
        opcode,
        &serde_json::to_vec(&data).map_err(|_| ())?,
        deadline,
    )
    .map_err(|_| ())?;
    // A bounded number of unsolicited events/pings cannot monopolize the worker.
    for _ in 0..8 {
        let (code, bytes) = pipe.receive(deadline).map_err(|_| ())?;
        if code == 3 {
            pipe.send(4, &bytes, deadline).map_err(|_| ())?;
            continue;
        }
        if code == 2 {
            return Err(());
        }
        let response: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if response["evt"] == "ERROR" {
            return Err(());
        }
        if nonce.is_none() && response["evt"] == "READY"
            || nonce.is_some_and(|n| response["nonce"] == n)
        {
            return Ok(());
        }
    }
    Err(())
}
#[cfg(windows)]
fn run(id: String, shared: Arc<(Mutex<Latest>, Condvar)>, status: Arc<AtomicU8>) {
    use tide_windows::discord_pipe::DiscordPipe;
    let mut pipe = None;
    let mut sent_revision = u64::MAX;
    let mut nonce = 0u64;
    let mut retry = Instant::now();
    let mut last_send = Instant::now() - Duration::from_secs(2);
    loop {
        let (lock, wake) = &*shared;
        let Ok(state) = lock.lock() else {
            break;
        };
        if state.stop {
            break;
        }
        let revision = state.revision;
        let activity = state.activity.clone();
        drop(state);
        if pipe.is_none() && Instant::now() >= retry {
            pipe = DiscordPipe::connect()
                .ok()
                .filter(|p| request(p, 0, json!({"v":1,"client_id":id}), None).is_ok());
            sent_revision = u64::MAX;
            retry = Instant::now() + Duration::from_secs(15);
        }
        if let Some(connection) = &pipe {
            if (revision != sent_revision && last_send.elapsed() >= Duration::from_secs(2))
                || last_send.elapsed() >= Duration::from_secs(30)
            {
                nonce = nonce.wrapping_add(1);
                let key = nonce.to_string();
                if request(connection, 1, payload(activity.as_ref(), nonce), Some(&key)).is_err() {
                    pipe = None;
                    status.store(0, Ordering::Relaxed);
                } else {
                    sent_revision = revision;
                    status.store(1, Ordering::Relaxed);
                }
                last_send = Instant::now();
            }
        }
        let Ok(state) = lock.lock() else {
            break;
        };
        if state.stop {
            break;
        }
        drop(wake.wait_timeout(state, Duration::from_secs(1)));
    }
    if let Some(connection) = pipe {
        let bytes = serde_json::to_vec(&payload(None, nonce.wrapping_add(1))).unwrap();
        let _ = connection.send(1, &bytes, DiscordPipe::deadline());
    }
}
#[cfg(not(windows))]
fn run(_id: String, shared: Arc<(Mutex<Latest>, Condvar)>, _status: Arc<AtomicU8>) {
    let (lock, wake) = &*shared;
    if let Ok(state) = lock.lock() {
        drop(wake.wait_while(state, |s| !s.stop));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_is_bounded_metadata_only() {
        let activity = Activity::new(&"界".repeat(1000), "artist\nname").unwrap();
        assert!(activity.title.len() <= 128);
        let value = payload(Some(&activity), 1);
        assert_eq!(value["args"]["activity"]["state"], "artistname");
        let object = value["args"]["activity"].as_object().unwrap();
        assert_eq!(object.len(), 3); // no URLs, artwork, session, account, invite or buttons
        assert!(Activity::new(" ", "artist").is_none());
        assert!(payload(None, 2)["args"]["activity"].is_null());
    }
    #[test]
    fn ids_and_disabled_state() {
        assert!(valid_id("123456789012345678"));
        for id in [
            "",
            "0",
            "../token",
            "12345678901234567x",
            "99999999999999999999",
        ] {
            assert!(!valid_id(id));
        }
        let mut presence = Presence::default();
        presence.update(false, "123456789012345678", None);
        assert!(presence.worker.is_none());
        presence.update(true, "bad", None);
        assert!(presence.worker.is_none());
    }
}
