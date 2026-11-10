//! Ephemeral nearby-room transport. No discovery, public relay, or Spotify credentials.
use crate::jam::{self, Playback, Room, Snapshot};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{watch, Mutex},
    task::{JoinHandle, JoinSet},
};

/// Ask the OS which local address it would route from; sends no datagram.
pub fn suggested_address() -> String {
    let address = (|| {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("192.0.2.1:80").ok()?;
        match socket.local_addr().ok()?.ip() {
            IpAddr::V4(ip) if ip.is_private() => Some(ip.to_string()),
            _ => None,
        }
    })();
    address.unwrap_or_else(|| "127.0.0.1".into())
}

pub enum Request {
    Host { name: String, address: String },
    Join { name: String, invite: String },
    Add { uri: String, title: String },
    Remove(String),
    Next,
    Complete { id: String, success: bool },
    Publish(Option<Playback>),
    Leave,
    Shutdown,
}
pub enum Event {
    Joined {
        invite: String,
        member_id: String,
        host: bool,
    },
    Snapshot {
        state: Snapshot,
        host_sent_ms: u64,
        transit_ms: u64,
    },
    Play {
        id: String,
        uri: String,
    },
    Left(String),
    Error(String),
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ClientMessage {
    Join {
        version: u8,
        token: String,
        name: String,
    },
    Add {
        operation: String,
        uri: String,
        title: String,
    },
    Remove {
        id: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ServerMessage {
    Welcome {
        version: u8,
        member_id: String,
        state: Snapshot,
        host_sent_ms: u64,
    },
    State {
        state: Snapshot,
        host_sent_ms: u64,
    },
    Error {
        message: String,
    },
}

struct Task(JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Shared {
    room: Mutex<Room>,
    updates: watch::Sender<Snapshot>,
}
impl Shared {
    async fn change<T>(
        &self,
        change: impl FnOnce(&mut Room) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut room = self.room.lock().await;
        let result = change(&mut room)?;
        self.updates.send_replace(room.snapshot.clone());
        Ok(result)
    }
}
struct Host {
    shared: Arc<Shared>,
    invite: String,
    member_id: String,
    _tasks: Vec<Task>,
}
struct Guest {
    commands: tokio::sync::mpsc::Sender<ClientMessage>,
    _tasks: Vec<Task>,
}
enum Mode {
    Host(Host),
    Guest(Guest),
}
enum Internal {
    State(u64, Snapshot, u64, u64),
    Left(u64, String),
    Error(u64, String),
}

pub fn worker() -> (
    tokio::sync::mpsc::UnboundedSender<Request>,
    mpsc::Receiver<Event>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (events, out) = mpsc::channel();
    let task = std::thread::spawn(move || {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(run(rx, events)),
            Err(_) => {
                let _ = events.send(Event::Error("Could not start shared jams.".into()));
            }
        }
    });
    (tx, out, task)
}

async fn run(
    mut requests: tokio::sync::mpsc::UnboundedReceiver<Request>,
    events: mpsc::Sender<Event>,
) {
    let (internal, mut incoming) = tokio::sync::mpsc::channel(64);
    let mut mode: Option<Mode> = None;
    let mut epoch = 0u64;
    loop {
        tokio::select! {
            request=requests.recv()=>{
                let Some(request)=request else{break;};
                let result:Result<(),String>=match request {
                    Request::Shutdown=>break,
                    Request::Leave=>{epoch+=1;mode=None;let _=events.send(Event::Left("You left the jam.".into()));Ok(())}
                    Request::Host{name,address}=>{
                        if mode.is_some(){Err("Leave the current jam first.".into())}else{
                            epoch+=1;
                            match host(&name,&address,epoch,internal.clone()).await {
                                Ok(host)=>{
                                    let _=events.send(Event::Joined{invite:host.invite.clone(),member_id:host.member_id.clone(),host:true});
                                    let _=events.send(Event::Snapshot{state:host.shared.updates.borrow().clone(),host_sent_ms:jam::unix_ms(),transit_ms:0});
                                    mode=Some(Mode::Host(host));Ok(())
                                },Err(error)=>Err(error)
                            }
                        }
                    }
                    Request::Join{name,invite}=>{
                        if mode.is_some(){Err("Leave the current jam first.".into())}else{
                            epoch+=1;
                            match guest(&name,&invite,epoch,internal.clone()).await {
                                Ok((guest,member,state,transit))=>{
                                    let _=events.send(Event::Joined{invite:String::new(),member_id:member,host:false});
                                    let _=events.send(Event::Snapshot{state:state.0,host_sent_ms:state.1,transit_ms:transit});
                                    mode=Some(Mode::Guest(guest));Ok(())
                                },Err(error)=>Err(error)
                            }
                        }
                    }
                    Request::Add{uri,title}=>match &mode {
                        Some(Mode::Host(host))=>host.shared.change(|room|room.add(&host.member_id,&jam::id(),&uri,&title)).await,
                        Some(Mode::Guest(guest))=>guest.commands.try_send(ClientMessage::Add{operation:jam::id(),uri,title}).map_err(|_|"The jam is busy. Try again.".into()),
                        None=>Err("Start or join a jam first.".into()),
                    },
                    Request::Remove(id)=>match &mode {
                        Some(Mode::Host(host))=>host.shared.change(|room|room.remove(&host.member_id,&id)).await,
                        Some(Mode::Guest(guest))=>guest.commands.try_send(ClientMessage::Remove{id}).map_err(|_|"The jam is busy. Try again.".into()),
                        None=>Err("You are not in a jam.".into()),
                    },
                    Request::Next=>match &mode {
                        Some(Mode::Host(host))=>match host.shared.change(Room::stage_next).await {
                            Ok(entry)=>{let _=events.send(Event::Play{id:entry.id,uri:entry.uri});Ok(())},Err(error)=>Err(error)
                        },_=>Err("Only the host can start the next shared track.".into())
                    },
                    Request::Complete{id,success}=>{
                        if let Some(Mode::Host(host))=&mode {let _=host.shared.change(|room|{room.complete(&id,success);Ok(())}).await;} Ok(())
                    }
                    Request::Publish(playback)=>{
                        if let Some(Mode::Host(host))=&mode {
                            let result=host.shared.change(|room|{let mut candidate=room.snapshot.clone();candidate.playback=playback;validate(&candidate)?;candidate.revision+=1;room.snapshot=candidate;Ok(())}).await;
                            if let Err(error)=result {let _=events.send(Event::Error(error));}
                        } Ok(())
                    }
                };
                if let Err(error)=result {let _=events.send(Event::Error(error));}
            }
            message=incoming.recv()=>{
                match message {
                    Some(Internal::State(generation,state,time,transit)) if generation==epoch=>{let _=events.send(Event::Snapshot{state,host_sent_ms:time,transit_ms:transit});}
                    Some(Internal::Left(generation,message)) if generation==epoch=>{epoch+=1;mode=None;let _=events.send(Event::Left(message));}
                    Some(Internal::Error(generation,message)) if generation==epoch=>{let _=events.send(Event::Error(message));}
                    _=>{}
                }
            }
        }
    }
    drop(mode);
}

fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
    }
}
fn invite(input: &str) -> Result<(SocketAddr, String), String> {
    let url = url::Url::parse(input.trim()).map_err(|_| "Paste a Tide or Grove jam invitation.")?;
    if url.scheme() != "grove-jam"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid jam invitation.".into());
    }
    let ip: IpAddr = url
        .host_str()
        .unwrap_or("")
        .trim_matches(['[', ']'])
        .parse()
        .map_err(|_| "Jam invitations must use a local or private IP address.")?;
    let port = url
        .port()
        .filter(|p| *p > 0)
        .ok_or("Jam invitation is missing its port.")?;
    let token = url.path().strip_prefix('/').unwrap_or("");
    if !private(ip)
        || token.len() < 20
        || token.len() > 128
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Use a valid invitation from a nearby Tide or Grove host.".into());
    }
    Ok((SocketAddr::new(ip, port), token.into()))
}

async fn host(
    name: &str,
    address: &str,
    epoch: u64,
    events: tokio::sync::mpsc::Sender<Internal>,
) -> Result<Host, String> {
    let ip: IpAddr = address
        .trim()
        .parse()
        .map_err(|_| "Enter 127.0.0.1 or this computer’s private LAN IP.")?;
    if !private(ip) {
        return Err("Nearby jams must bind to a loopback or private LAN address.".into());
    }
    let room = Room::new(name)?;
    let member_id = room.host_id().to_owned();
    let listener = TcpListener::bind(SocketAddr::new(ip, 0))
        .await
        .map_err(|_| "Could not start a jam on that address.")?;
    let address = listener
        .local_addr()
        .map_err(|_| "Could not find the jam address.")?;
    let token = jam::id();
    let invitation = format!("grove-jam://{address}/{token}");
    let (updates, mut changes) = watch::channel(room.snapshot.clone());
    let shared = Arc::new(Shared {
        room: Mutex::new(room),
        updates,
    });
    let state = shared.clone();
    let snapshot_events = events.clone();
    let publish = Task(tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            let snapshot = changes.borrow_and_update().clone();
            if snapshot_events
                .send(Internal::State(epoch, snapshot, jam::unix_ms(), 0))
                .await
                .is_err()
            {
                break;
            }
        }
    }));
    let server = Task(tokio::spawn(async move {
        let mut peers = JoinSet::new();
        loop {
            tokio::select! {
                accepted=listener.accept()=>{
                    let Ok((socket,_))=accepted else{break;};
                    if peers.len()>=32 {continue;}
                    let room=state.clone();let secret=token.clone();
                    peers.spawn(async move {peer(socket,room,&secret).await;});
                }
                _=peers.join_next(),if !peers.is_empty()=>{}
            }
        }
    }));
    Ok(Host {
        shared,
        invite: invitation,
        member_id,
        _tasks: vec![server, publish],
    })
}

async fn peer(socket: TcpStream, shared: Arc<Shared>, token: &str) {
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);
    let greeting = tokio::time::timeout(
        Duration::from_secs(5),
        read_json::<_, ClientMessage>(&mut reader, 8192),
    )
    .await;
    let member = match greeting {
        Ok(Ok(ClientMessage::Join {
            version: 2,
            token: provided,
            name,
        })) if provided == token => shared.change(|room| room.join(&name)).await,
        _ => Err("This invitation is invalid or expired.".into()),
    };
    let member = match member {
        Ok(member) => member,
        Err(error) => {
            let _ = write_json(&mut write, &ServerMessage::Error { message: error }).await;
            return;
        }
    };
    let mut changes = shared.updates.subscribe();
    let welcome = ServerMessage::Welcome {
        version: 2,
        member_id: member.clone(),
        state: changes.borrow_and_update().clone(),
        host_sent_ms: jam::unix_ms(),
    };
    if write_json(&mut write, &welcome).await.is_err() {
        let _ = shared
            .change(|r| {
                r.leave(&member);
                Ok(())
            })
            .await;
        return;
    }
    let (commands, mut messages) = tokio::sync::mpsc::channel(4);
    let _reader = Task(tokio::spawn(async move {
        loop {
            let result = read_json::<_, ClientMessage>(&mut reader, 8192).await;
            let stop = result.is_err();
            if commands.send(result).await.is_err() || stop {
                break;
            }
        }
    }));
    loop {
        tokio::select! {
            message=messages.recv()=>{
                let Some(Ok(message))=message else{break;};
                let result=shared.change(|room|match message {
                    ClientMessage::Add{operation,uri,title}=>room.add(&member,&operation,&uri,&title),
                    ClientMessage::Remove{id}=>room.remove(&member,&id),
                    _=>Err("Join only once per connection.".into()),
                }).await;
                if let Err(error)=result {if write_json(&mut write,&ServerMessage::Error{message:error}).await.is_err(){break;}}
            }
            changed=changes.changed()=>{
                if changed.is_err(){break;}
                let state=changes.borrow_and_update().clone();
                if write_json(&mut write,&ServerMessage::State{state,host_sent_ms:jam::unix_ms()}).await.is_err(){break;}
            }
        }
    }
    let _ = shared
        .change(|room| {
            room.leave(&member);
            Ok(())
        })
        .await;
}

async fn guest(
    name: &str,
    invitation: &str,
    epoch: u64,
    events: tokio::sync::mpsc::Sender<Internal>,
) -> Result<(Guest, String, (Snapshot, u64), u64), String> {
    let (address, token) = invite(invitation)?;
    let started = Instant::now();
    let socket = tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(address))
        .await
        .map_err(|_| "Jam connection timed out.")?
        .map_err(|_| "Could not reach the host. Check the invitation and network.")?;
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);
    write_json(
        &mut write,
        &ClientMessage::Join {
            version: 2,
            token,
            name: name.into(),
        },
    )
    .await?;
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        read_json::<_, ServerMessage>(&mut reader, 262144),
    )
    .await
    .map_err(|_| "The host did not answer the invitation.")??;
    let (member, state, time) = match response {
        ServerMessage::Welcome {
            version: 2,
            member_id,
            state,
            host_sent_ms,
        } => {
            validate(&state)?;
            (member_id, state, host_sent_ms)
        }
        ServerMessage::Error { message } => return Err(message),
        _ => return Err("Incompatible jam version. Update both apps.".into()),
    };
    let transit = (started.elapsed().as_millis() as u64 / 2).min(500);
    let (commands, mut outgoing) = tokio::sync::mpsc::channel(4);
    let errors = events.clone();
    let writer = Task(tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            if write_json(&mut write, &message).await.is_err() {
                let _ = errors
                    .send(Internal::Left(epoch, "Jam connection lost.".into()))
                    .await;
                break;
            }
        }
    }));
    let reader = Task(tokio::spawn(async move {
        loop {
            match read_json::<_, ServerMessage>(&mut reader, 262144).await {
                Ok(ServerMessage::State {
                    state,
                    host_sent_ms,
                }) => {
                    if validate(&state).is_err() {
                        break;
                    }
                    if events
                        .send(Internal::State(epoch, state, host_sent_ms, transit))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(ServerMessage::Error { message }) => {
                    let _ = events.send(Internal::Error(epoch, message)).await;
                }
                _ => break,
            }
        }
        let _ = events
            .send(Internal::Left(
                epoch,
                "The jam ended or the host disconnected.".into(),
            ))
            .await;
    }));
    Ok((
        Guest {
            commands,
            _tasks: vec![writer, reader],
        },
        member,
        (state, time),
        transit,
    ))
}

fn validate(state: &Snapshot) -> Result<(), String> {
    if state.members.is_empty()
        || state.members.len() > jam::MAX_MEMBERS
        || state.queue.len() > jam::MAX_TRACKS
    {
        return Err("Invalid jam state.".into());
    }
    if let Some(frame) = &state.playback {
        if crate::track_uri(&frame.uri).ok().as_deref() != Some(frame.uri.as_str())
            || frame.position_ms > 86_400_000
            || frame.title.chars().count() > 300
            || frame.artist.chars().count() > 300
            || frame
                .title
                .chars()
                .chain(frame.artist.chars())
                .any(char::is_control)
        {
            return Err("Invalid shared playback update.".into());
        }
    }
    Ok(())
}

async fn read_json<R: tokio::io::AsyncBufRead + Unpin, T: DeserializeOwned>(
    reader: &mut R,
    limit: usize,
) -> Result<T, String> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| "Jam connection lost.")?;
        if available.is_empty() {
            return Err("Jam connection closed.".into());
        }
        let newline = available.iter().position(|b| *b == b'\n');
        let count = newline.map_or(available.len(), |n| n + 1);
        if line.len() + count > limit {
            return Err("Jam message is too large.".into());
        }
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    serde_json::from_slice(&line).map_err(|_| "Invalid jam message.".into())
}
async fn write_json<W: tokio::io::AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| "Could not prepare the jam update.")?;
    bytes.push(b'\n');
    tokio::time::timeout(Duration::from_secs(2), writer.write_all(&bytes))
        .await
        .map_err(|_| "Jam update timed out.")?
        .map_err(|_| "Jam connection lost.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitations_reject_public_addresses_and_embedded_credentials() {
        assert!(invite("grove-jam://127.0.0.1:9876/abcdefghijklmnopqrstuv").is_ok());
        for url in [
            "grove-jam://8.8.8.8:9876/abcdefghijklmnopqrstuv",
            "grove-jam://user@127.0.0.1:9876/abcdefghijklmnopqrstuv",
            "https://127.0.0.1:9876/abcdefghijklmnopqrstuv",
        ] {
            assert!(invite(url).is_err());
        }
    }
    #[tokio::test]
    async fn cross_service_frames_pause_seek_and_version_guard_over_real_tcp() {
        let (events, mut received) = tokio::sync::mpsc::channel(64);
        let host = host("Tide", "127.0.0.1", 1, events.clone()).await.unwrap();
        let (_grove, _, _, _) = guest("Grove", &host.invite, 2, events).await.unwrap();
        for (uri, playing, position) in [
            ("youtube:abcdefghijk", true, 12000),
            ("youtube:abcdefghijk", false, 42000),
            ("spotify:track:aaaaaaaaaaaaaaaaaaaaaa", true, 8000),
        ] {
            host.shared
                .change(|room| {
                    room.snapshot.playback = Some(Playback {
                        uri: uri.into(),
                        title: "Same recording".into(),
                        artist: "Artist".into(),
                        position_ms: position,
                        playing,
                        sampled_at_ms: jam::unix_ms(),
                    });
                    validate(&room.snapshot)?;
                    room.snapshot.revision += 1;
                    Ok(())
                })
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Some(Internal::State(2, state, _, _)) = received.recv().await {
                        if let Some(p) = state.playback {
                            if p.uri == uri && p.position_ms == position && p.playing == playing {
                                break;
                            }
                        }
                    }
                }
            })
            .await
            .unwrap();
        }
        let (address, token) = invite(&host.invite).unwrap();
        let stream = TcpStream::connect(address).await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        write_json(
            &mut write,
            &ClientMessage::Join {
                version: 1,
                token,
                name: "Old app".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_json::<_, ServerMessage>(&mut read, 262144)
                .await
                .unwrap(),
            ServerMessage::Error { .. }
        ));
        let mut bad = host.shared.updates.borrow().clone();
        bad.playback.as_mut().unwrap().uri = "file:///private.mp3".into();
        assert!(validate(&bad).is_err());
    }
    #[tokio::test]
    async fn two_clients_share_host_queue_and_sync_frames_over_real_tcp() {
        let (events, mut received) = tokio::sync::mpsc::channel(64);
        let host = host("Host", "127.0.0.1", 1, events.clone()).await.unwrap();
        let (a, a_id, _, _) = guest("Alice", &host.invite, 2, events.clone())
            .await
            .unwrap();
        let (_b, _, _, _) = guest("Bob", &host.invite, 3, events).await.unwrap();
        a.commands
            .send(ClientMessage::Add {
                operation: "one".into(),
                uri: "spotify:track:2JRo0gjbX4GrCqBYdRohoo".into(),
                title: "Shared song".into(),
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(Internal::State(3, state, _, _)) = received.recv().await {
                    if state.queue.len() == 1 {
                        assert_eq!(state.queue[0].member_id, a_id);
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        host.shared
            .change(|room| {
                room.snapshot.playback = Some(Playback {
                    uri: "spotify:track:2JRo0gjbX4GrCqBYdRohoo".into(),
                    title: "Shared song".into(),
                    artist: "Artist".into(),
                    position_ms: 12000,
                    playing: true,
                    sampled_at_ms: jam::unix_ms(),
                });
                room.snapshot.revision += 1;
                Ok(())
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(Internal::State(2, state, _, _)) = received.recv().await {
                    if let Some(playback) = state.playback {
                        assert_eq!(playback.position_ms, 12000);
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        let serialized = serde_json::to_string(&host.shared.updates.borrow().clone()).unwrap();
        let (_, token) = invite(&host.invite).unwrap();
        assert!(!serialized.contains(&token));
    }
    #[tokio::test]
    async fn rejects_wrong_invite_and_notifies_guests_when_host_closes() {
        let (events, mut received) = tokio::sync::mpsc::channel(64);
        let host = host("Host", "127.0.0.1", 1, events.clone()).await.unwrap();
        let (address, _) = invite(&host.invite).unwrap();
        let wrong = format!("grove-jam://{address}/this-is-an-invalid-room-token");
        assert!(guest("Intruder", &wrong, 2, events.clone()).await.is_err());
        assert_eq!(host.shared.room.lock().await.snapshot.members.len(), 1);
        let (_guest, _, _, _) = guest("Alice", &host.invite, 3, events).await.unwrap();
        drop(host);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(Internal::Left(3, _)) = received.recv().await {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn bounded_frame_reader_rejects_oversized_input() {
        let mut reader = BufReader::new(&b"{\"oversized\":\"xxxxxxxxxxxxxxxxxxxxxxxx\"}\n"[..]);
        assert!(read_json::<_, serde_json::Value>(&mut reader, 16)
            .await
            .is_err());
    }
}
