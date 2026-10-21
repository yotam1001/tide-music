//! Optional Jam networking worker. It owns capabilities, never the local account
//! or player. One latest snapshot/event each bounds memory and avoids blocking UI.
use std::{
    collections::VecDeque,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tide_jam_lab::{
    client::{Client, Error},
    endpoint::Endpoint,
    protocol::*,
};
use zeroize::Zeroizing;

pub enum Start {
    Host {
        endpoint: String,
        operator: Option<PathBuf>,
        invite: PathBuf,
        timeline: Timeline,
    },
    Guest {
        invite: PathBuf,
    },
    HostCode {
        endpoint: String,
        timeline: Timeline,
    },
    GuestCode {
        code: Zeroizing<String>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Admin {
    Admission(bool),
    RotateInvite,
    Remove(u32),
    Acknowledge(u64),
}
pub enum Event {
    Hosting {
        guests: usize,
        invite_code: Zeroizing<String>,
        members: Vec<u32>,
        open: bool,
        requests: Vec<QueueRequest>,
    },
    View {
        view: View,
        rtt_ms: u64,
        received: Instant,
        listener: Option<u32>,
    },
    Problem {
        message: &'static str,
        terminal: bool,
    },
}
struct Frame {
    timeline: Timeline,
    sampled: Instant,
}
struct Shared {
    stop: AtomicBool,
    frame: Mutex<Option<Frame>>,
    event: Mutex<Option<Event>>,
    admin: Mutex<VecDeque<Admin>>,
    enqueue: Mutex<VecDeque<VideoId>>,
    notice: Mutex<Option<&'static str>>,
    wake: Condvar,
}
impl Shared {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
    fn event(&self, event: Event) {
        if let Ok(mut slot) = self.event.lock() {
            *slot = Some(event);
        }
    }
    fn wait(&self) {
        if let Ok(frame) = self.frame.lock() {
            let _ = self
                .wake
                .wait_timeout_while(frame, Duration::from_secs(1), |_| !self.stopped());
        }
    }
}
pub struct Session {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl Session {
    pub fn start(start: Start) -> Result<Self, &'static str> {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            frame: Mutex::new(None),
            event: Mutex::new(None),
            admin: Mutex::new(VecDeque::new()),
            enqueue: Mutex::new(VecDeque::new()),
            notice: Mutex::new(None),
            wake: Condvar::new(),
        });
        let worker_state = shared.clone();
        let worker = thread::Builder::new()
            .name("tide-jam".into())
            .spawn(move || {
                let result = match start {
                    Start::Host {
                        endpoint,
                        operator,
                        invite,
                        timeline,
                    } => host(
                        &worker_state,
                        &endpoint,
                        operator.as_deref(),
                        Some(&invite),
                        timeline,
                        false,
                    ),
                    Start::Guest { invite } => guest(&worker_state, &invite),
                    Start::HostCode { endpoint, timeline } => {
                        host(&worker_state, &endpoint, None, None, timeline, true)
                    }
                    Start::GuestCode { code } => {
                        Invite::from_code(&code).and_then(|invite| follow(&worker_state, invite))
                    }
                };
                if let Err(message) = result {
                    worker_state.event(Event::Problem {
                        message,
                        terminal: true,
                    });
                }
            })
            .map_err(|_| "Could not start Jam networking")?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }
    pub fn publish(&self, timeline: Timeline) {
        if let Ok(mut frame) = self.shared.frame.lock() {
            *frame = Some(Frame {
                timeline,
                sampled: Instant::now(),
            });
        }
    }
    pub fn take_event(&self) -> Option<Event> {
        self.shared.event.lock().ok()?.take()
    }
    pub fn admin(&self, command: Admin) -> bool {
        if self.shared.stopped() || self.finished() {
            return false;
        }
        let Ok(mut commands) = self.shared.admin.lock() else {
            return false;
        };
        if commands.contains(&command) {
            return true;
        }
        if commands.len() >= 40 {
            return false;
        }
        commands.push_back(command);
        self.shared.wake.notify_all();
        true
    }
    pub fn enqueue(&self, video: VideoId) -> bool {
        if self.shared.stopped() || self.finished() {
            return false;
        }
        let Ok(mut queue) = self.shared.enqueue.lock() else {
            return false;
        };
        if queue.len() >= 5 {
            return false;
        }
        queue.push_back(video);
        true
    }
    pub fn take_notice(&self) -> Option<&'static str> {
        self.shared.notice.lock().ok()?.take()
    }
    pub fn cancel(&self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.wake.notify_all();
    }
    pub fn finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(|worker| worker.is_finished())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn read_private(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    tide_jam_lab::files::read_private(path, limit)
}
fn operator(path: &Path) -> Result<Secret, &'static str> {
    if !path.to_string_lossy().ends_with(".jam-operator-key") {
        return Err("Select a .jam-operator-key file");
    }
    let bytes = read_private(path, 66)?;
    Secret::parse(
        std::str::from_utf8(&bytes)
            .map_err(|_| "Invalid operator key file")?
            .trim_end_matches(['\r', '\n']),
    )
}
fn write_invite(path: &Path, invite: &Invite) -> Result<(), &'static str> {
    tide_jam_lab::files::check_private_path(path)?;
    if !path.to_string_lossy().ends_with(".jam-invite.json") {
        return Err("Save the invite as .jam-invite.json");
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Cannot save invite; choose a new private filename")?;
    let bytes = tide_jam_lab::client::encode(invite).map_err(|_| "Cannot encode Jam invite")?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err("Cannot write Jam invite");
    }
    Ok(())
}
struct Hosted<'a> {
    client: &'a Client,
    room: Created,
}
impl Drop for Hosted<'_> {
    fn drop(&mut self) {
        let _ = self.client.close(&self.room.room, &self.room.host);
    }
}
struct Membership<'a> {
    client: &'a Client,
    room: String,
    token: Secret,
}
impl Drop for Membership<'_> {
    fn drop(&mut self) {
        let _ = self.client.leave(&self.room, &self.token);
    }
}
fn wake_relay(shared: &Shared, client: &Client) -> Result<bool, &'static str> {
    if !client.needs_wakeup() {
        return Ok(!shared.stopped());
    }
    let began = Instant::now();
    shared.event(Event::Problem {
        message: "Starting the free Jam server; this can take about a minute",
        terminal: false,
    });
    while !shared.stopped() && began.elapsed() < Duration::from_secs(90) {
        if client.ready().is_ok() {
            return Ok(!shared.stopped());
        }
        shared.wait();
    }
    if shared.stopped() {
        Ok(false)
    } else {
        Err("Jam server did not start in time; retry shortly")
    }
}
fn host(
    shared: &Shared,
    endpoint: &str,
    key: Option<&Path>,
    path: Option<&Path>,
    mut timeline: Timeline,
    shared_mode: bool,
) -> Result<(), &'static str> {
    timeline.validate()?;
    let endpoint = Endpoint::parse(endpoint)
        .map_err(|_| "Enter a canonical HTTPS relay or literal loopback address")?;
    let client = Client::new(endpoint.as_str()).map_err(|_| "Invalid Jam relay")?;
    let key = key.map(operator).transpose()?;
    if endpoint.is_remote() && key.is_none() && !shared_mode {
        return Err("HTTPS hosting requires the relay operator's key file");
    }
    if shared.stopped() {
        return Ok(());
    }
    if shared_mode && !wake_relay(shared, &client)? {
        return Ok(());
    }
    let created = match &key {
        Some(key) => client.create_with_key(key),
        None if shared_mode && endpoint.is_remote() => client.create_shared(),
        None => client.create(),
    }
    .map_err(|error| {
        if shared_mode && error == Error::Limited {
            "Free shared server full or rate-limited. Retry later, or use your own server under Advanced setup."
        } else if shared_mode {
            "Cannot create Jam; the server may be busy or its free allowance exhausted"
        } else {
            "Cannot create Jam; check the relay and operator key"
        }
    })?;
    drop(key);
    let room = Hosted {
        client: &client,
        room: created,
    };
    if shared.stopped() {
        return Ok(());
    }
    let mut invitation = Invite {
        endpoint: endpoint.as_str().into(),
        room: room.room.room.clone(),
        invite: room.room.invite.clone(),
    };
    if let Some(path) = path {
        write_invite(path, &invitation)?;
    }
    let mut invite_code = invitation.to_code()?;
    let mut sampled = Instant::now();
    let mut revision = 0u64;
    let mut failures = 0;
    let mut pending_admin: Option<(Admin, u8)> = None;
    while !shared.stopped() {
        let command = match pending_admin.take() {
            Some(command) => Some(command),
            None => shared
                .admin
                .lock()
                .map_err(|_| "Jam controls unavailable")?
                .pop_front()
                .map(|command| (command, 0)),
        };
        if let Some((command, attempt)) = command {
            let result = match command {
                Admin::Admission(open) => client
                    .set_admission(&room.room.room, &room.room.host, open)
                    .map(|_| None),
                Admin::RotateInvite => client
                    .rotate_invite(&room.room.room, &room.room.host)
                    .map(Some),
                Admin::Remove(id) => client
                    .remove_member(&room.room.room, &room.room.host, id)
                    .map(Some),
                Admin::Acknowledge(id) => client
                    .acknowledge(&room.room.room, &room.room.host, id)
                    .map(|_| None),
            };
            match result {
                Ok(Some(token)) => {
                    invitation.invite = token;
                    invite_code = invitation.to_code()?;
                }
                Ok(None) => {}
                Err(_) => {
                    if matches!(command, Admin::RotateInvite | Admin::Remove(_)) {
                        // Never restore a possibly revoked code to the clipboard UI.
                        invite_code = Zeroizing::new(String::new());
                    }
                    if attempt < 2 {
                        pending_admin = Some((command, attempt + 1));
                    }
                    shared.event(Event::Problem {
                        message: "Could not update room controls; retrying is bounded",
                        terminal: false,
                    });
                }
            }
        }
        if let Some(frame) = shared
            .frame
            .lock()
            .map_err(|_| "Jam snapshot unavailable")?
            .take()
        {
            timeline = frame.timeline;
            sampled = frame.sampled;
        }
        let mut outgoing = timeline.clone();
        revision = revision
            .checked_add(1)
            .ok_or("Jam revision limit reached")?;
        outgoing.revision = revision;
        let age = sampled.elapsed().as_millis() as u64;
        if age >= 2000 {
            outgoing.loading = true;
            outgoing.paused = true;
        } else if !outgoing.paused && !outgoing.loading {
            outgoing.position_ms = outgoing
                .position_ms
                .saturating_add(age)
                .min(MAX_POSITION_MS);
        }
        match client.publish(&room.room.room, &room.room.host, &outgoing) {
            Ok(()) => {
                failures = 0;
                // The host poll updates guest count only; no authority comes from it.
                if !shared.stopped() {
                    match client.host_view(&room.room.room, &room.room.host) {
                        Ok(view) => shared.event(Event::Hosting {
                            guests: view.members.len(),
                            invite_code: invite_code.clone(),
                            members: view.members,
                            open: view.open,
                            // Older relays do not implement collaborative queues.
                            // That must not interrupt synchronized playback.
                            requests: client
                                .requests(&room.room.room, &room.room.host)
                                .unwrap_or_default(),
                        }),
                        Err(_) => shared.event(Event::Problem {
                            message: "Jam hosted; guest count temporarily unavailable",
                            terminal: false,
                        }),
                    }
                }
            }
            Err(Error::Unavailable | Error::Invalid) => return Err("Jam room closed or expired"),
            Err(_) => {
                failures += 1;
                if failures >= 5 {
                    return Err("Jam relay unavailable after bounded retries");
                }
                shared.event(Event::Problem {
                    message: "Jam connection interrupted; retrying",
                    terminal: false,
                });
            }
        }
        shared.wait();
    }
    Ok(())
}
fn guest(shared: &Shared, path: &Path) -> Result<(), &'static str> {
    if !path.to_string_lossy().ends_with(".jam-invite.json") {
        return Err("Select a .jam-invite.json file");
    }
    let bytes = read_private(path, MAX_BODY)?;
    let invite: Invite = serde_json::from_slice(&bytes).map_err(|_| "Invalid Jam invite file")?;
    drop(bytes);
    follow(shared, invite)
}
fn follow(shared: &Shared, invite: Invite) -> Result<(), &'static str> {
    let client = Client::new(&invite.endpoint)
        .map_err(|_| "Invite requires a canonical HTTPS relay or literal loopback address")?;
    if shared.stopped() {
        return Ok(());
    }
    if !wake_relay(shared, &client)? {
        return Ok(());
    }
    let joined = client
        .join(&invite)
        .map_err(|_| "Cannot join Jam; the invite may be expired or the room full")?;
    let membership = Membership {
        client: &client,
        room: invite.room,
        token: joined.member,
    };
    let listener = client.listener_id(&membership.room, &membership.token).ok();
    let mut failures = 0;
    while !shared.stopped() {
        let video = shared
            .enqueue
            .lock()
            .map_err(|_| "Jam queue unavailable")?
            .pop_front();
        if let Some(video) = video {
            let message = match client.enqueue(&membership.room, &membership.token, video) {
                Ok(()) => "Song sent to the shared queue",
                Err(Error::Limited) => "Queue request limit reached. Wait before adding another song.",
                Err(_) => "Song addition was not confirmed. Check the queue before retrying; the host may need a newer build or the room may be busy.",
            };
            if let Ok(mut notice) = shared.notice.lock() {
                *notice = Some(message);
            }
        }
        let began = Instant::now();
        match client.poll(&membership.room, &membership.token) {
            Ok(view) => {
                failures = 0;
                shared.event(Event::View {
                    view,
                    rtt_ms: began.elapsed().as_millis() as u64,
                    received: Instant::now(),
                    listener,
                });
            }
            Err(Error::Unavailable | Error::Invalid) => {
                return Err("Jam closed, expired or returned invalid state")
            }
            Err(_) => {
                failures += 1;
                if failures >= 5 {
                    return Err("Jam disconnected after bounded retries");
                }
                shared.event(Event::Problem {
                    message: "Jam connection interrupted; playback will pause",
                    terminal: false,
                });
            }
        }
        shared.wait();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};

    struct Fixture {
        endpoint: String,
        invite: PathBuf,
        stop: Arc<AtomicBool>,
        removed: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }
    impl Fixture {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let stop = Arc::new(AtomicBool::new(false));
            let removed = Arc::new(AtomicBool::new(false));
            let cancel = stop.clone();
            let observed_remove = removed.clone();
            let invite =
                std::env::temp_dir().join(format!("{}.jam-invite.json", room_id().unwrap()));
            let worker = thread::spawn(move || {
                let created = Created {
                    room: room_id().unwrap(),
                    host: Secret::random().unwrap(),
                    invite: Secret::random().unwrap(),
                };
                let member = Secret::random().unwrap();
                let mut state = None;
                let mut pending: Vec<QueueRequest> = Vec::new();
                let mut request_id = 0;
                let deadline = Instant::now() + Duration::from_secs(8);
                while !cancel.load(Ordering::Acquire) && Instant::now() < deadline {
                    let mut socket = match listener.accept() {
                        Ok((socket, _)) => socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                            continue;
                        }
                        Err(error) => panic!("synthetic listener failed: {error}"),
                    };
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let (head, body) = request(&mut socket);
                    assert!(head.contains("x-tide-jam-protocol: 1"));
                    let (status, body) = if head.starts_with("POST /v1/rooms HTTP") {
                        assert!(!head.to_lowercase().contains("authorization:"));
                        ("200 OK", serde_json::to_vec(&created).unwrap())
                    } else if head.starts_with("POST ") && head.contains("/join HTTP") {
                        (
                            "200 OK",
                            serde_json::to_vec(&Joined {
                                member: member.clone(),
                            })
                            .unwrap(),
                        )
                    } else if head.starts_with("POST ") && head.contains("/requests HTTP") {
                        let value: Enqueue = serde_json::from_slice(&body).unwrap();
                        request_id += 1;
                        pending.push(QueueRequest {
                            id: request_id,
                            video: value.video,
                        });
                        ("204 No Content", vec![])
                    } else if head.starts_with("GET ") && head.contains("/requests HTTP") {
                        ("200 OK", serde_json::to_vec(&pending).unwrap())
                    } else if head.starts_with("DELETE ") && head.contains("/requests/") {
                        let id = head
                            .split("/requests/")
                            .nth(1)
                            .unwrap()
                            .split_whitespace()
                            .next()
                            .unwrap()
                            .parse::<u64>()
                            .unwrap();
                        pending.retain(|r| r.id != id);
                        ("204 No Content", vec![])
                    } else if head.starts_with("PUT ") {
                        let timeline: Timeline = serde_json::from_slice(&body).unwrap();
                        timeline.validate().unwrap();
                        state = Some(timeline);
                        ("204 No Content", vec![])
                    } else if head.starts_with("GET ") && head.contains("/members HTTP") {
                        (
                            "200 OK",
                            serde_json::to_vec(&HostView {
                                members: vec![1],
                                open: true,
                            })
                            .unwrap(),
                        )
                    } else if head.starts_with("GET ") {
                        (
                            "200 OK",
                            serde_json::to_vec(&View {
                                timeline: state.clone(),
                                age_ms: 0,
                                members: 1,
                            })
                            .unwrap(),
                        )
                    } else if head.starts_with("DELETE ") {
                        observed_remove.store(true, Ordering::Release);
                        ("204 No Content", vec![])
                    } else {
                        panic!("unexpected synthetic request method");
                    };
                    write!(
                        socket,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .unwrap();
                    socket.write_all(&body).unwrap();
                }
            });
            Self {
                endpoint,
                invite,
                stop,
                removed,
                worker: Some(worker),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.worker.take().unwrap().join().unwrap();
            let _ = fs::remove_file(&self.invite);
        }
    }
    fn request(socket: &mut TcpStream) -> (String, Zeroizing<Vec<u8>>) {
        let mut head = Zeroizing::new(Vec::new());
        let mut byte = [0];
        while !head.ends_with(b"\r\n\r\n") {
            assert!(head.len() < 8192);
            socket.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head.to_vec()).unwrap();
        let size = head
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(str::to_owned)
            })
            .map_or(0, |size| size.parse::<usize>().unwrap());
        assert!(size <= MAX_BODY);
        let mut body = Zeroizing::new(vec![0; size]);
        socket.read_exact(&mut body).unwrap();
        (head, body)
    }
    fn wait<T>(mut predicate: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(value) = predicate() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "synthetic Jam worker deadline exceeded"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn end(session: &Session) {
        let began = Instant::now();
        session.cancel();
        assert!(
            began.elapsed() < Duration::from_millis(100),
            "cancellation blocked the UI caller"
        );
        wait(|| session.finished().then_some(()));
    }
    #[test]
    fn guest_queue_worker_delivers_once_and_host_acknowledges_without_changing_timeline() {
        let fixture = Fixture::start();
        let host = Session::start(Start::HostCode {
            endpoint: fixture.endpoint.clone(),
            timeline: Timeline {
                revision: 1,
                play_id: 1,
                queue: vec![VideoId::parse("fixture0001").unwrap()],
                index: 0,
                position_ms: 0,
                paused: true,
                loading: false,
            },
        })
        .unwrap();
        let code = wait(|| match host.take_event() {
            Some(Event::Hosting { invite_code, .. }) => Some(invite_code),
            _ => None,
        });
        let guest = Session::start(Start::GuestCode { code }).unwrap();
        wait(|| match guest.take_event() {
            Some(Event::View { .. }) => Some(()),
            _ => None,
        });
        assert!(guest.enqueue(VideoId::parse("fixture0002").unwrap()));
        let id = wait(|| match host.take_event() {
            Some(Event::Hosting { requests, .. }) if !requests.is_empty() => {
                assert_eq!(requests.len(), 1);
                assert_eq!(requests[0].video.as_str(), "fixture0002");
                Some(requests[0].id)
            }
            _ => None,
        });
        assert!(host.admin(Admin::Acknowledge(id)));
        wait(|| match host.take_event() {
            Some(Event::Hosting { requests, .. }) if requests.is_empty() => Some(()),
            _ => None,
        });
        end(&guest);
        end(&host);
    }
    #[test]
    fn code_host_and_guest_exchange_state_without_reading_or_writing_invite_files() {
        let fixture = Fixture::start();
        let host = Session::start(Start::HostCode {
            endpoint: fixture.endpoint.clone(),
            timeline: Timeline {
                revision: 1,
                play_id: 42,
                queue: vec![VideoId::parse("fixture0001").unwrap()],
                index: 0,
                position_ms: 0,
                paused: true,
                loading: false,
            },
        })
        .unwrap();
        let code = wait(|| match host.take_event() {
            Some(Event::Hosting { invite_code, .. }) => Some(invite_code),
            Some(Event::Problem { message, .. }) => panic!("{message}"),
            _ => None,
        });
        assert!(
            !fixture.invite.exists(),
            "code hosting must not create an invite file"
        );
        let guest = Session::start(Start::GuestCode { code }).unwrap();
        wait(|| match guest.take_event() {
            Some(Event::View { view, .. }) if view.timeline.is_some() => Some(()),
            Some(Event::Problem { message, .. }) => panic!("{message}"),
            _ => None,
        });
        end(&guest);
        end(&host);
        assert!(!fixture.invite.exists());
        assert!(fixture.removed.load(Ordering::Acquire));
    }
    #[test]
    fn hosted_worker_saves_only_guest_invite_and_closes_without_blocking_cancellation() {
        let fixture = Fixture::start();
        let timeline = Timeline {
            revision: 1,
            play_id: 42,
            queue: vec![VideoId::parse("fixture0001").unwrap()],
            index: 0,
            position_ms: 0,
            paused: true,
            loading: false,
        };
        let session = Session::start(Start::Host {
            endpoint: fixture.endpoint.clone(),
            operator: None,
            invite: fixture.invite.clone(),
            timeline,
        })
        .unwrap();
        wait(|| match session.take_event() {
            Some(Event::Hosting { guests: 1, .. }) => Some(()),
            Some(Event::Problem { message, .. }) => panic!("{message}"),
            _ => None,
        });
        let bytes = read_private(&fixture.invite, MAX_BODY).unwrap();
        let invite: Invite = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(invite.endpoint, fixture.endpoint);
        let keys = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        assert_eq!(keys.as_object().unwrap().len(), 3);
        assert!(keys.get("host").is_none() && keys.get("operator").is_none());
        end(&session);
        assert!(fixture.removed.load(Ordering::Acquire));
    }
    #[test]
    fn guest_worker_delivers_only_validated_views_and_leaves_its_membership() {
        let fixture = Fixture::start();
        write_invite(
            &fixture.invite,
            &Invite {
                endpoint: fixture.endpoint.clone(),
                room: room_id().unwrap(),
                invite: Secret::random().unwrap(),
            },
        )
        .unwrap();
        let session = Session::start(Start::Guest {
            invite: fixture.invite.clone(),
        })
        .unwrap();
        wait(|| match session.take_event() {
            Some(Event::View { view, .. }) => {
                view.validate().unwrap();
                Some(())
            }
            Some(Event::Problem { message, .. }) => panic!("{message}"),
            _ => None,
        });
        end(&session);
        assert!(fixture.removed.load(Ordering::Acquire));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Isolated named-pipe file-input probe; never use a real private file"]
    fn private_pipe_input_does_not_block_cancellation() {
        let path = PathBuf::from(std::env::var_os("JAM_PRIVATE_PIPE_PROBE").unwrap());
        assert!(path
            .to_string_lossy()
            .starts_with(r"\\.\pipe\tide-jam-input-probe-"));
        let session = Session::start(Start::Guest { invite: path }).unwrap();
        assert!(matches!(
            wait(|| session.take_event()),
            Event::Problem { terminal: true, .. }
        ));
        session.cancel();
        let began = Instant::now();
        drop(session);
        assert!(began.elapsed() < Duration::from_secs(1));
    }
}
