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
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::Semaphore};

const MAX_ROOMS: usize = 32;
const ROOM_LIFETIME_MS: u64 = 2 * 60 * 60 * 1000;
const MEMBER_IDLE_MS: u64 = 30_000;
type Failure = (StatusCode, &'static str);
const MISSING: Failure = (StatusCode::NOT_FOUND, "Room or membership unavailable");

struct Member {
    token: Secret,
    seen: u64,
}
struct Room {
    host: Secret,
    invite: Secret,
    born: u64,
    updated: u64,
    timeline: Option<Timeline>,
    members: Vec<Member>,
}
#[derive(Default)]
pub struct Store {
    rooms: HashMap<String, Room>,
    creates: VecDeque<u64>,
}
impl Store {
    fn clean(&mut self, now: u64) {
        self.rooms
            .retain(|_, room| now.saturating_sub(room.born) < ROOM_LIFETIME_MS);
        for room in self.rooms.values_mut() {
            room.members
                .retain(|member| now.saturating_sub(member.seen) < MEMBER_IDLE_MS);
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
        self.clean(now);
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
                host: result.host.clone(),
                invite: result.invite.clone(),
                born: now,
                updated: now,
                timeline: None,
                members: Vec::new(),
            },
        );
        self.creates.push_back(now);
        Ok(result)
    }
    pub fn join(&mut self, id: &str, invite: &Secret, now: u64) -> Result<Joined, Failure> {
        self.clean(now);
        let room = self.rooms.get_mut(id).ok_or(MISSING)?;
        if !room.invite.matches(invite) {
            return Err(MISSING);
        }
        if room.members.len() >= 8 {
            return Err((StatusCode::CONFLICT, "Room is full"));
        }
        let token = Secret::random().map_err(internal)?;
        room.members.push(Member {
            token: token.clone(),
            seen: now,
        });
        Ok(Joined { member: token })
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
        room.members.remove(index);
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
async fn create(State(s): State<Shared>, Json(_): Json<Empty>) -> Result<Json<Created>, Failure> {
    s.with(|store, now| store.create(now)).map(Json)
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
    serve_inner(listener, stop, None).await
}

/// Backend remains loopback-only. The operator must provide a trusted, correctly
/// configured TLS proxy; this function never opens a public plaintext listener.
pub async fn serve_private(
    listener: TcpListener,
    stop: tokio::sync::watch::Receiver<bool>,
    config: PrivateRelay,
) -> std::io::Result<()> {
    serve_inner(listener, stop, Some(config)).await
}

async fn serve_inner(
    listener: TcpListener,
    mut stop: tokio::sync::watch::Receiver<bool>,
    config: Option<PrivateRelay>,
) -> std::io::Result<()> {
    let address = listener.local_addr()?;
    if address.ip() != std::net::Ipv4Addr::LOCALHOST {
        return Err(std::io::Error::other(
            "The Jam lab can only listen on 127.0.0.1",
        ));
    }
    let (host, operator) = match config {
        Some(config) => (
            config.endpoint.authority().to_owned(),
            Some(config.operator),
        ),
        None => (address.to_string(), None),
    };
    let shared = Shared {
        store: Arc::new(Mutex::new(Store::default())),
        epoch: Instant::now(),
        host,
        operator,
        requests: Arc::new(Mutex::new(VecDeque::new())),
    };
    let router = Router::new()
        .route("/v1/rooms", post(create))
        .route("/v1/rooms/{id}/join", post(join))
        .route("/v1/rooms/{id}/state", get(view).put(publish))
        .route("/v1/rooms/{id}", delete(close))
        .route("/v1/rooms/{id}/members/self", delete(leave))
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
