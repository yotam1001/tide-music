use crate::endpoint::Endpoint;
use crate::protocol::*;
use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{
    collections::{hash_map::RandomState, HashMap, VecDeque},
    hash::BuildHasher,
    net::{IpAddr, Ipv6Addr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::Semaphore};

const MAX_ROOMS: usize = 32;
const ROOM_LIFETIME_MS: u64 = 8 * 60 * 60 * 1000;
const ROOM_IDLE_MS: u64 = 120_000;
const PEER_HEADER: &str = "x-tide-jam-peer";
const MEMBER_IDLE_MS: u64 = 30_000;
type Failure = (StatusCode, &'static str);
const MISSING: Failure = (StatusCode::NOT_FOUND, "Room or membership unavailable");

struct Member {
    id: u32,
    token: Secret,
    seen: u64,
    requests: VecDeque<u64>,
}
struct PendingRequest {
    member: u32,
    request: QueueRequest,
}
struct Room {
    open: bool,
    next_member: u32,
    peer: Option<u64>,
    host: Secret,
    invite: Secret,
    born: u64,
    updated: u64,
    timeline: Option<Timeline>,
    members: Vec<Member>,
    pending: Vec<PendingRequest>,
    next_request: u64,
    skip: Option<(u32, Skip)>,
}
#[derive(Default)]
pub struct Store {
    rooms: HashMap<String, Room>,
    creates: VecDeque<u64>,
    peer_creates: HashMap<u64, VecDeque<u64>>,
}
impl Store {
    fn clean(&mut self, now: u64) {
        self.rooms.retain(|_, room| {
            now.saturating_sub(room.born) < ROOM_LIFETIME_MS
                && now.saturating_sub(room.updated) < ROOM_IDLE_MS
        });
        for room in self.rooms.values_mut() {
            room.members
                .retain(|member| now.saturating_sub(member.seen) < MEMBER_IDLE_MS);
            room.pending
                .retain(|p| room.members.iter().any(|m| m.id == p.member));
            if room
                .skip
                .as_ref()
                .is_some_and(|(id, _)| !room.members.iter().any(|m| m.id == *id))
            {
                room.skip = None;
            }
        }
        while self
            .creates
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 60_000)
        {
            self.creates.pop_front();
        }
    }
    pub fn create(&mut self, now: u64) -> Result<Created, Failure> {
        self.create_inner(None, now)
    }
    pub fn create_shared(&mut self, peer: u64, now: u64) -> Result<Created, Failure> {
        self.create_inner(Some(peer), now)
    }
    fn create_inner(&mut self, peer: Option<u64>, now: u64) -> Result<Created, Failure> {
        self.clean(now);
        self.peer_creates.retain(|_, times| {
            while times
                .front()
                .is_some_and(|t| now.saturating_sub(*t) >= 60_000)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
        if let Some(peer) = peer {
            let active = self
                .rooms
                .values()
                .filter(|room| room.peer == Some(peer))
                .count();
            if active >= 2
                || self
                    .peer_creates
                    .get(&peer)
                    .is_some_and(|times| times.len() >= 2)
                || (!self.peer_creates.contains_key(&peer) && self.peer_creates.len() >= 1024)
            {
                return Err((StatusCode::TOO_MANY_REQUESTS, "Room creation limit reached"));
            }
        }
        if self.rooms.len() >= MAX_ROOMS || self.creates.len() >= 16 {
            return Err((StatusCode::TOO_MANY_REQUESTS, "Room capacity reached"));
        }
        let result = Created {
            room: room_id().map_err(internal)?,
            host: Secret::random().map_err(internal)?,
            invite: Secret::random().map_err(internal)?,
        };
        self.rooms.insert(
            result.room.clone(),
            Room {
                open: true,
                next_member: 1,
                peer,
                host: result.host.clone(),
                invite: result.invite.clone(),
                born: now,
                updated: now,
                timeline: None,
                members: Vec::new(),
                pending: Vec::new(),
                next_request: 1,
                skip: None,
            },
        );
        self.creates.push_back(now);
        if let Some(peer) = peer {
            self.peer_creates.entry(peer).or_default().push_back(now);
        }
        Ok(result)
    }
    pub fn join(&mut self, id: &str, invite: &Secret, now: u64) -> Result<Joined, Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        if !room.invite.matches(invite) {
            return Err(MISSING);
        }
        if !room.open {
            return Err((StatusCode::CONFLICT, "Room is locked"));
        }
        if room.members.len() >= 8 {
            return Err((StatusCode::CONFLICT, "Room is full"));
        }
        let token = Secret::random().map_err(internal)?;
        let id = room.next_member;
        room.next_member = id
            .checked_add(1)
            .filter(|id| *id <= i32::MAX as u32)
            .ok_or(internal(""))?;
        room.members.push(Member {
            id,
            token: token.clone(),
            seen: now,
            requests: VecDeque::new(),
        });
        Ok(Joined { member: token })
    }
    fn hosted(&mut self, id: &str, host: &Secret, now: u64) -> Result<&mut Room, Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        if !room.host.matches(host) {
            return Err(MISSING);
        }
        Ok(room)
    }
    pub fn host_view(&mut self, id: &str, host: &Secret, now: u64) -> Result<HostView, Failure> {
        let room = self.hosted(id, host, now)?;
        Ok(HostView {
            members: room.members.iter().map(|m| m.id).collect(),
            open: room.open,
        })
    }
    pub fn set_admission(
        &mut self,
        id: &str,
        host: &Secret,
        open: bool,
        now: u64,
    ) -> Result<(), Failure> {
        self.hosted(id, host, now)?.open = open;
        Ok(())
    }
    pub fn rotate_invite(
        &mut self,
        id: &str,
        host: &Secret,
        now: u64,
    ) -> Result<RotatedInvite, Failure> {
        let room = self.hosted(id, host, now)?;
        room.invite = Secret::random().map_err(internal)?;
        Ok(RotatedInvite {
            invite: room.invite.clone(),
        })
    }
    pub fn remove_member(
        &mut self,
        id: &str,
        host: &Secret,
        member_id: u32,
        now: u64,
    ) -> Result<RotatedInvite, Failure> {
        let room = self.hosted(id, host, now)?;
        let index = room
            .members
            .iter()
            .position(|member| member.id == member_id)
            .ok_or(MISSING)?;
        let replacement = Secret::random().map_err(internal)?;
        room.members.remove(index);
        room.pending.retain(|p| p.member != member_id);
        if room.skip.as_ref().is_some_and(|(id, _)| *id == member_id) {
            room.skip = None;
        }
        room.invite = replacement;
        Ok(RotatedInvite {
            invite: room.invite.clone(),
        })
    }
    pub fn listener_id(
        &mut self,
        id: &str,
        member: &Secret,
        now: u64,
    ) -> Result<ListenerId, Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        let member = room
            .members
            .iter()
            .find(|m| m.token.matches(member))
            .ok_or(MISSING)?;
        Ok(ListenerId { id: member.id })
    }
    pub fn enqueue(
        &mut self,
        id: &str,
        token: &Secret,
        video: VideoId,
        now: u64,
    ) -> Result<(), Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        let member = room
            .members
            .iter_mut()
            .find(|m| m.token.matches(token))
            .ok_or(MISSING)?;
        while member
            .requests
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 60_000)
        {
            member.requests.pop_front();
        }
        if member.requests.len() >= 5 || room.pending.len() >= MAX_REQUESTS {
            return Err((StatusCode::TOO_MANY_REQUESTS, "Queue request limit reached"));
        }
        let request_id = room.next_request;
        room.next_request = request_id.checked_add(1).ok_or(internal(""))?;
        member.requests.push_back(now);
        member.seen = now;
        room.pending.push(PendingRequest {
            member: member.id,
            request: QueueRequest {
                id: request_id,
                video,
            },
        });
        Ok(())
    }
    pub fn request_skip(
        &mut self,
        id: &str,
        token: &Secret,
        skip: Skip,
        now: u64,
    ) -> Result<(), Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        let member = room
            .members
            .iter_mut()
            .find(|m| m.token.matches(token))
            .ok_or(MISSING)?;
        if !room.timeline.as_ref().is_some_and(|t| skip.matches(t)) {
            return Err((StatusCode::CONFLICT, "Song has already changed"));
        }
        // All listeners requesting the same play collapse to a single operation.
        if room
            .skip
            .as_ref()
            .is_some_and(|(_, pending)| pending == &skip)
        {
            return Ok(());
        }
        while member
            .requests
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 60_000)
        {
            member.requests.pop_front();
        }
        if member.requests.len() >= 5 {
            return Err((StatusCode::TOO_MANY_REQUESTS, "Jam request limit reached"));
        }
        member.requests.push_back(now);
        member.seen = now;
        room.skip = Some((member.id, skip));
        Ok(())
    }
    pub fn skip_request(
        &mut self,
        id: &str,
        host: &Secret,
        now: u64,
    ) -> Result<Option<Skip>, Failure> {
        Ok(self
            .hosted(id, host, now)?
            .skip
            .as_ref()
            .map(|(_, skip)| skip.clone()))
    }
    pub fn requests(
        &mut self,
        id: &str,
        host: &Secret,
        now: u64,
    ) -> Result<Vec<QueueRequest>, Failure> {
        Ok(self
            .hosted(id, host, now)?
            .pending
            .iter()
            .map(|p| p.request.clone())
            .collect())
    }
    pub fn acknowledge(
        &mut self,
        id: &str,
        host: &Secret,
        request: u64,
        now: u64,
    ) -> Result<(), Failure> {
        self.hosted(id, host, now)?
            .pending
            .retain(|p| p.request.id != request);
        Ok(())
    }
    pub fn publish(
        &mut self,
        id: &str,
        host: &Secret,
        timeline: Timeline,
        now: u64,
    ) -> Result<(), Failure> {
        timeline
            .validate()
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid timeline"))?;
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        if !room.host.matches(host) {
            return Err(MISSING);
        }
        if let Some(previous) = &room.timeline {
            if timeline.revision <= previous.revision || timeline.play_id < previous.play_id {
                return Err((StatusCode::CONFLICT, "Outdated timeline"));
            }
            if now.saturating_sub(room.updated) < 100 {
                return Err((StatusCode::TOO_MANY_REQUESTS, "Publish rate exceeded"));
            }
        }
        if room
            .skip
            .as_ref()
            .is_some_and(|(_, skip)| !skip.matches(&timeline))
        {
            room.skip = None;
        }
        room.timeline = Some(timeline);
        room.updated = now;
        Ok(())
    }
    pub fn view(&mut self, id: &str, token: &Secret, now: u64) -> Result<View, Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        if !room.host.matches(token) {
            let member = room
                .members
                .iter_mut()
                .find(|m| m.token.matches(token))
                .ok_or(MISSING)?;
            member.seen = now;
        }
        Ok(View {
            timeline: room.timeline.clone(),
            age_ms: now.saturating_sub(room.updated),
            members: room.members.len(),
        })
    }
    pub fn close(&mut self, id: &str, host: &Secret, now: u64) -> Result<(), Failure> {
        self.clean(now);
        if !self.rooms.get(id).ok_or(MISSING)?.host.matches(host) {
            return Err(MISSING);
        }
        self.rooms.remove(id);
        Ok(())
    }
    pub fn leave(&mut self, id: &str, member: &Secret, now: u64) -> Result<(), Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        let index = room
            .members
            .iter()
            .position(|m| m.token.matches(member))
            .ok_or(MISSING)?;
        let member = room.members.remove(index);
        room.pending.retain(|p| p.member != member.id);
        if room.skip.as_ref().is_some_and(|(id, _)| *id == member.id) {
            room.skip = None;
        }
        Ok(())
    }
}
fn internal(_: &'static str) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, "Room setup unavailable")
}
fn token(headers: &HeaderMap) -> Result<Secret, Failure> {
    let text = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(MISSING)?;
    Secret::parse(text).map_err(|_| MISSING)
}
fn checked_id(id: &str) -> Result<(), Failure> {
    if valid_hex(id, 32) {
        Ok(())
    } else {
        Err(MISSING)
    }
}

#[derive(Clone)]
struct Shared {
    store: Arc<Mutex<Store>>,
    epoch: Instant,
    host: String,
    operator: Option<Secret>,
    shared_relay: bool,
    peer_hash: RandomState,
    requests: Arc<Mutex<VecDeque<u64>>>,
}
impl Shared {
    fn now(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
    fn with<T>(
        &self,
        action: impl FnOnce(&mut Store, u64) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        let mut store = self.store.lock().map_err(|_| internal(""))?;
        action(&mut store, self.now())
    }
}

async fn guard(State(shared): State<Shared>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    // Only the explicit shared reverse-proxy mode trusts a peer header. The
    // proxy must overwrite it from the socket address; direct client values
    // cannot select authority or bypass the loopback-only backend listener.
    if (shared.shared_relay && checked_peer(headers).is_err())
        || (!shared.shared_relay && headers.contains_key(PEER_HEADER))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if headers.contains_key(header::ORIGIN)
        || headers.contains_key("forwarded")
        || headers.contains_key("x-forwarded-host")
        || headers.contains_key("x-forwarded-proto")
        || headers.get_all(header::HOST).iter().count() != 1
        || headers.get_all("x-tide-jam-protocol").iter().count() != 1
        || headers.get_all(header::AUTHORIZATION).iter().count() > 1
        || headers.get_all(header::CONTENT_TYPE).iter().count() > 1
        || request.uri().scheme().is_some()
        || request.uri().authority().is_some()
        || request.uri().path().contains(['%', '\\'])
        || request.uri().path().contains("//")
        || headers.get(header::HOST).and_then(|h| h.to_str().ok()) != Some(shared.host.as_str())
        || headers
            .get("x-tide-jam-protocol")
            .and_then(|h| h.to_str().ok())
            != Some("1")
        || request.uri().query().is_some()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Bounded global admission, independent of untrusted forwarded IP headers.
    // This is a private relay, not a claim to resist Internet denial of service.
    {
        let Ok(mut requests) = shared.requests.lock() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let now = shared.now();
        while requests
            .front()
            .is_some_and(|time| now.saturating_sub(*time) >= 1000)
        {
            requests.pop_front();
        }
        if requests.len() >= 512 {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        requests.push_back(now);
    }
    if request.method() == axum::http::Method::POST && request.uri().path() == "/v1/rooms" {
        if let Some(operator) = &shared.operator {
            if !token(headers).is_ok_and(|supplied| operator.matches(&supplied)) {
                // Authenticate before reading JSON or consuming room capacity.
                return StatusCode::NOT_FOUND.into_response();
            }
        }
    }
    if (request.method() == axum::http::Method::POST || request.method() == axum::http::Method::PUT)
        && headers
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            != Some("application/json")
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let mut response = match tokio::time::timeout(Duration::from_secs(2), next.run(request)).await {
        Ok(response) => response,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn checked_peer(headers: &HeaderMap) -> Result<IpAddr, Failure> {
    if headers.get_all(PEER_HEADER).iter().count() != 1 {
        return Err(MISSING);
    }
    let value = headers
        .get(PEER_HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or(MISSING)?;
    let ip: IpAddr = value.parse().map_err(|_| MISSING)?;
    if ip.to_string() != value {
        return Err(MISSING);
    }
    // Bucket IPv6 addresses by /64 so changing the interface identifier does
    // not grant more rooms. No raw IP is retained in the in-memory store.
    Ok(match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & (u128::MAX << 64))),
        },
        other => other,
    })
}
async fn create(
    State(s): State<Shared>,
    headers: HeaderMap,
    Json(_): Json<Empty>,
) -> Result<Json<Created>, Failure> {
    if s.shared_relay {
        if headers.contains_key(header::AUTHORIZATION) {
            return Err(MISSING);
        }
        let peer = s.peer_hash.hash_one(checked_peer(&headers)?);
        s.with(|store, now| store.create_shared(peer, now))
            .map(Json)
    } else {
        s.with(|store, now| store.create(now)).map(Json)
    }
}
async fn join(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(_): Json<Empty>,
) -> Result<Json<Joined>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.join(&id, &auth, now)).map(Json)
}
async fn enqueue(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(value): Json<Enqueue>,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.enqueue(&id, &auth, value.video, now))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn request_skip(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(value): Json<Skip>,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.request_skip(&id, &auth, value, now))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn skip_request(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Option<Skip>>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.skip_request(&id, &auth, now))
        .map(Json)
}
async fn requests(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<QueueRequest>>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.requests(&id, &auth, now))
        .map(Json)
}
async fn acknowledge(
    State(s): State<Shared>,
    Path((id, request)): Path<(String, u64)>,
    headers: HeaderMap,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.acknowledge(&id, &auth, request, now))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn publish(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(timeline): Json<Timeline>,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.publish(&id, &auth, timeline, now))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn view(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<View>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.view(&id, &auth, now)).map(Json)
}
async fn close(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.close(&id, &auth, now))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn leave(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.leave(&id, &auth, now))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn host_view(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<HostView>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.host_view(&id, &auth, now))
        .map(Json)
}
async fn admission(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(value): Json<Admission>,
) -> Result<StatusCode, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.set_admission(&id, &auth, value.open, now))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn rotate_invite(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(_): Json<Empty>,
) -> Result<Json<RotatedInvite>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.rotate_invite(&id, &auth, now))
        .map(Json)
}
async fn remove_member(
    State(s): State<Shared>,
    Path((id, member)): Path<(String, u32)>,
    headers: HeaderMap,
) -> Result<Json<RotatedInvite>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.remove_member(&id, &auth, member, now))
        .map(Json)
}
async fn listener_id(
    State(s): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ListenerId>, Failure> {
    checked_id(&id)?;
    let auth = token(&headers)?;
    s.with(|store, now| store.listener_id(&id, &auth, now))
        .map(Json)
}

/// Explicit, inseparable configuration for a private TLS reverse proxy on the
/// same machine. No forwarded header can select this authority or supply a key.
#[derive(Debug)]
pub struct PrivateRelay {
    endpoint: Endpoint,
    operator: Secret,
}
impl PrivateRelay {
    pub fn new(endpoint: &str, operator: Secret) -> Result<Self, &'static str> {
        let endpoint = Endpoint::parse(endpoint).map_err(|_| "Invalid private relay endpoint")?;
        if !endpoint.is_remote() {
            return Err("Private relays require an HTTPS endpoint");
        }
        Ok(Self { endpoint, operator })
    }
}

/// Ordinary lab mode. Literal loopback only, without proxy support.
pub async fn serve(
    listener: TcpListener,
    stop: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    serve_inner(listener, stop, None, None).await
}

/// Backend remains loopback-only. The operator must provide a trusted, correctly
/// configured TLS proxy; this function never opens a public plaintext listener.
pub async fn serve_private(
    listener: TcpListener,
    stop: tokio::sync::watch::Receiver<bool>,
    config: PrivateRelay,
) -> std::io::Result<()> {
    serve_inner(listener, stop, Some(config), None).await
}

/// Shared room creation behind a same-machine TLS proxy. Unlike private mode,
/// no operator secret is distributed to player installations. The proxy must
/// overwrite x-tide-jam-peer with its remote socket IP on every request.
#[derive(Debug)]
pub struct SharedRelay {
    endpoint: Endpoint,
}
impl SharedRelay {
    pub fn new(endpoint: &str) -> Result<Self, &'static str> {
        let endpoint = Endpoint::parse(endpoint).map_err(|_| "Invalid shared relay endpoint")?;
        if !endpoint.is_remote() {
            return Err("Shared relays require an HTTPS endpoint");
        }
        Ok(Self { endpoint })
    }
}
pub async fn serve_shared(
    listener: TcpListener,
    stop: tokio::sync::watch::Receiver<bool>,
    config: SharedRelay,
) -> std::io::Result<()> {
    serve_inner(listener, stop, None, Some(config)).await
}

async fn serve_inner(
    listener: TcpListener,
    mut stop: tokio::sync::watch::Receiver<bool>,
    config: Option<PrivateRelay>,
    shared_config: Option<SharedRelay>,
) -> std::io::Result<()> {
    let address = listener.local_addr()?;
    if address.ip() != std::net::Ipv4Addr::LOCALHOST {
        return Err(std::io::Error::other(
            "The Jam lab can only listen on 127.0.0.1",
        ));
    }
    let shared_relay = shared_config.is_some();
    let shared_host = shared_config.map(|config| config.endpoint.authority().to_owned());
    let (host, operator) = match config {
        Some(config) => (
            config.endpoint.authority().to_owned(),
            Some(config.operator),
        ),
        None => (shared_host.unwrap_or_else(|| address.to_string()), None),
    };
    let shared = Shared {
        store: Arc::new(Mutex::new(Store::default())),
        epoch: Instant::now(),
        host,
        operator,
        shared_relay,
        peer_hash: RandomState::new(),
        requests: Arc::new(Mutex::new(VecDeque::new())),
    };
    let router = Router::new()
        .route("/v1/rooms", post(create))
        .route("/v1/rooms/{id}/join", post(join))
        .route("/v1/rooms/{id}/requests", get(requests).post(enqueue))
        .route("/v1/rooms/{id}/skip", get(skip_request).post(request_skip))
        .route("/v1/rooms/{id}/requests/{request}", delete(acknowledge))
        .route("/v1/rooms/{id}/state", get(view).put(publish))
        .route("/v1/rooms/{id}", delete(close))
        .route(
            "/v1/rooms/{id}/members/self",
            get(listener_id).delete(leave),
        )
        .route("/v1/rooms/{id}/members", get(host_view))
        .route("/v1/rooms/{id}/members/{member}", delete(remove_member))
        .route("/v1/rooms/{id}/admission", axum::routing::put(admission))
        .route("/v1/rooms/{id}/invite", post(rotate_invite))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(shared.clone(), guard))
        .with_state(shared.clone());
    let slots = Arc::new(Semaphore::new(16));
    let mut tasks = tokio::task::JoinSet::new();
    let mut cleanup = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = cleanup.tick() => { let _ = shared.with(|store, now| { store.clean(now); Ok(()) }); },
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let Ok(permit) = slots.clone().try_acquire_owned() else { drop(stream); continue; };
                // Completed task handles are bounded too, including under a request flood.
                while tasks.len() >= 32 { let _ = tasks.join_next().await; }
                let service = TowerToHyperService::new(router.clone());
                tasks.spawn(async move {
                    let _permit = permit;
                    let connection = hyper::server::conn::http1::Builder::new()
                        .timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(2))
                        .keep_alive(false).max_buf_size(MAX_BODY)
                        .serve_connection(TokioIo::new(stream), service);
                    let _ = tokio::time::timeout(Duration::from_secs(4), connection).await;
                });
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod shared_admission_tests {
    use super::*;
    #[test]
    fn skips_require_membership_match_the_current_play_and_do_not_control_the_timeline() {
        let mut store = Store::default();
        let room = store.create(0).unwrap();
        let guest = store.join(&room.room, &room.invite, 1).unwrap();
        let second = store.join(&room.room, &room.invite, 2).unwrap();
        let mut timeline = Timeline {
            revision: 1,
            play_id: 8,
            queue: vec![
                VideoId::parse("abcdefghijk").unwrap(),
                VideoId::parse("lmnopqrstuv").unwrap(),
            ],
            index: 0,
            position_ms: 0,
            paused: false,
            loading: false,
        };
        store
            .publish(&room.room, &room.host, timeline.clone(), 3)
            .unwrap();
        let skip = Skip {
            play_id: 8,
            video: timeline.queue[0].clone(),
        };
        for token in [&room.invite, &room.host] {
            assert!(store
                .request_skip(&room.room, token, skip.clone(), 4)
                .is_err());
        }
        let wrong = Skip {
            play_id: 8,
            video: timeline.queue[1].clone(),
        };
        assert!(store
            .request_skip(&room.room, &guest.member, wrong, 5)
            .is_err());
        store
            .request_skip(&room.room, &guest.member, skip.clone(), 6)
            .unwrap();
        store
            .request_skip(&room.room, &second.member, skip.clone(), 7)
            .unwrap();
        assert!(store.skip_request(&room.room, &guest.member, 8).is_err());
        assert_eq!(
            store.skip_request(&room.room, &room.host, 8).unwrap(),
            Some(skip.clone())
        );
        assert_eq!(
            store.view(&room.room, &guest.member, 9).unwrap().timeline,
            Some(timeline.clone())
        );
        timeline.revision += 1;
        timeline.play_id += 1;
        timeline.index = 1;
        store
            .publish(&room.room, &room.host, timeline.clone(), 110)
            .unwrap();
        assert!(store
            .skip_request(&room.room, &room.host, 111)
            .unwrap()
            .is_none());
        assert!(store
            .request_skip(&room.room, &guest.member, skip, 112)
            .is_err());
        let skip = Skip {
            play_id: 9,
            video: timeline.queue[1].clone(),
        };
        store
            .request_skip(&room.room, &guest.member, skip.clone(), 113)
            .unwrap();
        store.leave(&room.room, &guest.member, 114).unwrap();
        assert!(store
            .skip_request(&room.room, &room.host, 115)
            .unwrap()
            .is_none());
        assert!(store
            .request_skip(&room.room, &guest.member, skip, 116)
            .is_err());
    }
    #[test]
    fn skip_rate_limit_is_shared_with_queue_additions() {
        let mut store = Store::default();
        let room = store.create(0).unwrap();
        let guest = store.join(&room.room, &room.invite, 1).unwrap();
        let video = VideoId::parse("abcdefghijk").unwrap();
        store
            .publish(
                &room.room,
                &room.host,
                Timeline {
                    revision: 1,
                    play_id: 1,
                    queue: vec![video.clone()],
                    index: 0,
                    position_ms: 0,
                    paused: false,
                    loading: false,
                },
                2,
            )
            .unwrap();
        for now in 3..8 {
            store
                .enqueue(&room.room, &guest.member, video.clone(), now)
                .unwrap();
        }
        assert_eq!(
            store
                .request_skip(&room.room, &guest.member, Skip { play_id: 1, video }, 8)
                .unwrap_err()
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    #[test]
    fn queue_requests_are_member_only_bounded_and_host_acknowledged() {
        let mut store = Store::default();
        let room = store.create(0).unwrap();
        let guest = store.join(&room.room, &room.invite, 1).unwrap();
        let video = VideoId::parse("abcdefghijk").unwrap();
        assert!(store
            .enqueue(&room.room, &room.invite, video.clone(), 2)
            .is_err());
        assert!(store
            .enqueue(&room.room, &room.host, video.clone(), 2)
            .is_err());
        for now in 2..7 {
            store
                .enqueue(&room.room, &guest.member, video.clone(), now)
                .unwrap();
        }
        assert!(store.enqueue(&room.room, &guest.member, video, 7).is_err());
        assert!(store.requests(&room.room, &guest.member, 8).is_err());
        let requests = store.requests(&room.room, &room.host, 8).unwrap();
        assert_eq!(requests.len(), 5);
        assert!(store
            .acknowledge(&room.room, &guest.member, requests[0].id, 9)
            .is_err());
        store
            .acknowledge(&room.room, &room.host, requests[0].id, 9)
            .unwrap();
        store
            .acknowledge(&room.room, &room.host, requests[0].id, 10)
            .unwrap();
        assert_eq!(store.requests(&room.room, &room.host, 11).unwrap().len(), 4);
        // Guest submissions never mutate the authoritative playback timeline.
        assert!(store
            .view(&room.room, &guest.member, 12)
            .unwrap()
            .timeline
            .is_none());
        store.leave(&room.room, &guest.member, 13).unwrap();
        assert!(store
            .requests(&room.room, &room.host, 14)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn disconnected_and_removed_members_lose_pending_requests() {
        let mut store = Store::default();
        let room = store.create(0).unwrap();
        let guest = store.join(&room.room, &room.invite, 1).unwrap();
        store
            .enqueue(
                &room.room,
                &guest.member,
                VideoId::parse("abcdefghijk").unwrap(),
                2,
            )
            .unwrap();
        assert!(store
            .requests(&room.room, &room.host, 30_003)
            .unwrap()
            .is_empty());
        let guest = store.join(&room.room, &room.invite, 30_004).unwrap();
        let id = store
            .listener_id(&room.room, &guest.member, 30_005)
            .unwrap()
            .id;
        store
            .enqueue(
                &room.room,
                &guest.member,
                VideoId::parse("abcdefghijk").unwrap(),
                30_006,
            )
            .unwrap();
        store
            .remove_member(&room.room, &room.host, id, 30_007)
            .unwrap();
        assert!(store
            .requests(&room.room, &room.host, 30_008)
            .unwrap()
            .is_empty());
        assert!(store
            .enqueue(
                &room.room,
                &guest.member,
                VideoId::parse("abcdefghijk").unwrap(),
                30_009
            )
            .is_err());
    }
    fn timeline(revision: u64) -> Timeline {
        Timeline {
            revision,
            play_id: 1,
            queue: vec![VideoId::parse("abcdefghijk").unwrap()],
            index: 0,
            position_ms: 0,
            paused: true,
            loading: false,
        }
    }
    #[test]
    fn one_peer_cannot_fill_the_relay_and_closing_does_not_reset_its_rate() {
        let mut store = Store::default();
        let a = store.create_shared(1, 0).unwrap();
        let b = store.create_shared(1, 1).unwrap();
        assert!(store.create_shared(1, 2).is_err());
        assert!(store.create_shared(2, 2).is_ok());
        store.close(&a.room, &a.host, 3).unwrap();
        store.close(&b.room, &b.host, 4).unwrap();
        assert!(store.create_shared(1, 5).is_err());
        assert!(store.create_shared(1, 60_001).is_ok());
    }
    #[test]
    fn active_rooms_survive_long_listening_and_idle_rooms_release_capacity() {
        let mut store = Store::default();
        let room = store.create_shared(1, 0).unwrap();
        for minute in 0..240u64 {
            store
                .publish(
                    &room.room,
                    &room.host,
                    timeline(minute + 1),
                    minute * 60_000,
                )
                .unwrap();
        }
        assert!(store
            .view(&room.room, &room.host, 239 * 60_000 + 100)
            .is_ok());
        assert!(store.view(&room.room, &room.host, 241 * 60_000).is_err());
        assert!(store.create_shared(1, 241 * 60_000).is_ok());
    }
    #[test]
    fn proxy_peer_validation_groups_ipv6_and_rejects_ambiguous_addresses() {
        let make = |ip: &'static str| {
            let mut h = HeaderMap::new();
            h.insert(PEER_HEADER, ip.parse().unwrap());
            h
        };
        let first = checked_peer(&make("2001:db8:1:2::1")).unwrap();
        let second = checked_peer(&make("2001:db8:1:2::abcd")).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, checked_peer(&make("2001:db8:1:3::1")).unwrap());
        for ip in [
            "203.0.113.7, 203.0.113.8",
            "example.test",
            "0203.0.113.7",
            "2001:0DB8::1",
        ] {
            assert!(checked_peer(&make(ip)).is_err());
        }
    }
}
