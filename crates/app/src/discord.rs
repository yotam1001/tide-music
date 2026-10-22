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
            let mut result = String::new();
            for c in value.chars().filter(|c| !c.is_control()) {
                if result.len() + c.len_utf8() > 128 {
                    break;
                }
                result.push(c);
            }
            result
        }
        let title = bounded(title);
        if title.trim().is_empty() {
            return None;
        }
        Some(Self {
            title,
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
    let activity = activity.map(|a| {
        let mut value = json!({"type":2,"details":a.title});
        if !a.artist.trim().is_empty() {
            value["state"] = json!(a.artist);
        }
        value
    });
    json!({"cmd":"SET_ACTIVITY", "args":{"pid":std::process::id(), "activity":activity}, "nonce":nonce.to_string()})
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
    retiring: Option<Worker>,
}
impl Presence {
    fn retire(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
            self.retiring = Some(worker);
        }
    }
    pub fn update(&mut self, enabled: bool, id: &str, activity: Option<Activity>) -> &'static str {
        if self
            .retiring
            .as_ref()
            .is_some_and(|w| w.thread.as_ref().is_none_or(|t| t.is_finished()))
        {
            self.retiring.take();
        }
        if !enabled {
            self.retire();
            self.id.clear();
            return "Off — listening activity stays private";
        }
        if !valid_id(id) {
            self.retire();
            self.id.clear();
            return "Enter a Discord application ID to connect";
        }
        if self.id != id {
            self.retire();
            self.id = id.to_owned();
        }
        if self.retiring.is_some() {
            return "Updating Discord connection";
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
    fn stop(&self) {
        let (lock, wake) = &*self.shared;
        if let Ok(mut state) = lock.lock() {
            state.stop = true;
            wake.notify_one();
        }
    }
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
        self.stop();
        // Never wait for Discord I/O on the UI thread. Retired workers finish
        // bounded I/O and clear activity; no replacement starts until then.
        if let Some(thread) = self.thread.take().filter(|t| t.is_finished()) {
            let _ = thread.join();
        }
    }
}
#[cfg(any(windows, test))]
trait Transport {
    fn send(&self, opcode: u32, data: &[u8], deadline: Instant) -> std::io::Result<()>;
    fn receive(&self, deadline: Instant) -> std::io::Result<(u32, Vec<u8>)>;
}
#[cfg(windows)]
impl Transport for tide_windows::discord_pipe::DiscordPipe {
    fn send(&self, opcode: u32, data: &[u8], deadline: Instant) -> std::io::Result<()> {
        self.send(opcode, data, deadline)
    }
    fn receive(&self, deadline: Instant) -> std::io::Result<(u32, Vec<u8>)> {
        self.receive(deadline)
    }
}
#[cfg(any(windows, test))]
fn request(pipe: &impl Transport, opcode: u32, data: Value, nonce: Option<&str>) -> Result<(), ()> {
    let deadline = Instant::now() + Duration::from_millis(500);
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
        if code != 1 {
            return Err(());
        }
        let response: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if response["evt"] == "ERROR" {
            return Err(());
        }
        if nonce.is_none() && response["evt"] == "READY"
            || nonce.is_some_and(|n| response["cmd"] == "SET_ACTIVITY" && response["nonce"] == n)
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
        let local_song = Activity::new("Local song", "").unwrap();
        assert!(payload(Some(&local_song), 3)["args"]["activity"]
            .get("state")
            .is_none());
        assert!(Activity::new("\0", "artist").is_none());
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

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};
    struct Fake {
        replies: RefCell<VecDeque<(u32, Vec<u8>)>>,
        writes: RefCell<Vec<(u32, Vec<u8>)>>,
    }
    impl Fake {
        fn new(values: Vec<(u32, Value)>) -> Self {
            Self {
                replies: RefCell::new(
                    values
                        .into_iter()
                        .map(|(op, v)| (op, serde_json::to_vec(&v).unwrap()))
                        .collect(),
                ),
                writes: RefCell::new(Vec::new()),
            }
        }
    }
    impl Transport for Fake {
        fn send(&self, op: u32, bytes: &[u8], _deadline: Instant) -> std::io::Result<()> {
            self.writes.borrow_mut().push((op, bytes.to_vec()));
            Ok(())
        }
        fn receive(&self, _deadline: Instant) -> std::io::Result<(u32, Vec<u8>)> {
            self.replies
                .borrow_mut()
                .pop_front()
                .ok_or(std::io::ErrorKind::UnexpectedEof.into())
        }
    }
    #[test]
    fn disabling_does_not_block_ui_or_start_overlapping_workers() {
        let shared = Arc::new((Mutex::new(Latest::default()), Condvar::new()));
        let task = shared.clone();
        let handle = std::thread::spawn(move || {
            let (lock, wake) = &*task;
            let guard = lock.lock().unwrap();
            drop(wake.wait_while(guard, |s| !s.stop).unwrap());
            std::thread::sleep(Duration::from_millis(200));
        });
        let id = "123456789012345678";
        let mut presence = Presence {
            id: id.into(),
            worker: Some(Worker {
                shared,
                status: Arc::new(AtomicU8::new(1)),
                thread: Some(handle),
            }),
            retiring: None,
        };
        let started = Instant::now();
        presence.update(false, id, None);
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(presence.worker.is_none());
        assert!(presence.retiring.is_some());
        assert_eq!(
            presence.update(true, id, None),
            "Updating Discord connection"
        );
        assert!(presence.worker.is_none());
        // Finish our synthetic worker without starting or contacting Discord.
        presence
            .retiring
            .as_mut()
            .unwrap()
            .thread
            .take()
            .unwrap()
            .join()
            .unwrap();
        presence.update(false, id, None);
        assert!(presence.retiring.is_none());
    }

    #[test]
    fn requires_ready_and_matching_acknowledgement() {
        let pipe = Fake::new(vec![(1, json!({"evt":"READY"}))]);
        assert!(request(
            &pipe,
            0,
            json!({"v":1,"client_id":"123456789012345678"}),
            None
        )
        .is_ok());
        let pipe = Fake::new(vec![
            (1, json!({"cmd":"SET_ACTIVITY","nonce":"old"})),
            (1, json!({"cmd":"SET_ACTIVITY","nonce":"2"})),
        ]);
        assert!(request(&pipe, 1, payload(None, 2), Some("2")).is_ok());
        let pipe = Fake::new(vec![(1, json!({"cmd":"WRONG","nonce":"2"}))]);
        assert!(request(&pipe, 1, payload(None, 2), Some("2")).is_err());
    }
    #[test]
    fn handles_ping_errors_and_unsolicited_event_flood() {
        let pipe = Fake::new(vec![
            (3, json!({"ping":1})),
            (1, json!({"cmd":"SET_ACTIVITY","nonce":"2"})),
        ]);
        assert!(request(&pipe, 1, payload(None, 2), Some("2")).is_ok());
        assert_eq!(pipe.writes.borrow()[1].0, 4);
        let pipe = Fake::new(vec![(1, json!({"evt":"ERROR","nonce":"2"}))]);
        assert!(request(&pipe, 1, payload(None, 2), Some("2")).is_err());
        let pipe = Fake::new(vec![(1, json!({"evt":"UNRELATED"})); 9]);
        assert!(request(&pipe, 0, json!({}), None).is_err());
        assert_eq!(pipe.replies.borrow().len(), 1);
        let pipe = Fake {
            replies: RefCell::new(VecDeque::from([(1, b"invalid json".to_vec())])),
            writes: RefCell::new(Vec::new()),
        };
        assert!(request(&pipe, 0, json!({}), None).is_err());
    }
}
