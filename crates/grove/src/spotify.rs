//! Separate Web API and native librespot grants, with cancellable background work.
use crate::{
    auth, credentials,
    engine::Engine,
    web_api::{SavedLibrary, WebApi},
};
use librespot_core::authentication::Credentials;
use serde_json::{json, Value};
use std::{
    sync::{mpsc, Arc},
    thread,
};
use tokio::{sync::Mutex, task::JoinSet};

pub type RequestSender = tokio::sync::mpsc::UnboundedSender<Request>;
pub enum Request {
    SignInLibrary(String),
    Restore,
    StartPlayback(bool),
    Cancel,
    SignOut,
    Disconnect,
    Shutdown,
    Command(Value),
    RefreshLibrary,
    Feature {
        id: u64,
        action: crate::library::Action,
    },
}
pub enum Event {
    Feature {
        id: u64,
        data: Result<Value, String>,
    },
    Connected(u64),
    Disconnected(String),
    PlaybackStopped(u64),
    Payload(u64, Value),
    Library {
        name: String,
        playlists: Value,
    },
    SignedOut,
    Busy(bool, String),
    Error(String),
}
enum Outcome {
    Feature(u64, Result<Value, String>),
    Library(WebApi, Value),
    Playback(Engine, Credentials),
    Playlists(Value),
    Queue(u64, Value),
    Empty,
}

pub fn command(name: &str) -> Value {
    json!({"type":"command","command":name})
}

pub fn spotify_uri(input: &str) -> Result<String, String> {
    let input = input.trim();
    let parts: Vec<String> = if input.starts_with("spotify:") {
        input.split(':').skip(1).map(str::to_owned).collect()
    } else {
        let url = url::Url::parse(input).map_err(|_| "Paste a Spotify link or URI.")?;
        if url.scheme() != "https"
            || url.host_str() != Some("open.spotify.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err("Use an https://open.spotify.com link.".into());
        }
        url.path_segments()
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .filter(|s| !s.starts_with("intl-"))
            .collect()
    };
    if parts.len() != 2
        || !["track", "album", "playlist", "episode"].contains(&parts[0].as_str())
        || parts[1].len() != 22
        || !parts[1].bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err("Use a Spotify track, album, playlist, or episode link.".into());
    }
    Ok(format!("spotify:{}:{}", parts[0], parts[1]))
}

pub fn worker() -> (RequestSender, mpsc::Receiver<Event>, thread::JoinHandle<()>) {
    let (tx, requests) = tokio::sync::mpsc::unbounded_channel();
    let (events, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(run(requests, events)),
            Err(_) => {
                let _ = events.send(Event::Error("Could not start Spotify networking.".into()));
            }
        }
    });
    (tx, rx, handle)
}

async fn run(
    mut requests: tokio::sync::mpsc::UnboundedReceiver<Request>,
    events: mpsc::Sender<Event>,
) {
    let mut web: Option<Arc<Mutex<WebApi>>> = None;
    let mut account_id = String::new();
    let mut account_name = String::new();
    let mut premium = None;
    let mut engine: Option<Engine> = None;
    let mut jobs: JoinSet<(u64, bool, Result<Outcome, String>)> = JoinSet::new();
    let mut generation = 1u64;
    let mut busy = false;
    let mut queue_pending = false;
    let mut queue_timer = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { break; };
                match request {
                    Request::Feature { id, action } => {
                        if let Some(api) = web.as_ref().cloned() {
                            let gen = generation;
                            if jobs.len() >= 8 {
                                let _ = events.send(Event::Feature { id, data: Err("Spotify is busy. Wait for the current request.".into()) });
                            } else {
                                jobs.spawn(async move {
                                    let result = api.lock().await.feature(action).await;
                                    (gen,false,Ok(Outcome::Feature(id,result)))
                                });
                            }
                        } else {
                            let _ = events.send(Event::Feature { id, data: Err("Sign in for Spotify library access first.".into()) });
                        }
                    }
                    Request::Shutdown => break,
                    Request::Cancel => {
                        generation += 1; jobs.abort_all(); busy = false; queue_pending = false;
                        let _ = events.send(Event::Busy(false, "Sign-in cancelled.".into()));
                    }
                    Request::SignOut => {
                        generation += 1; jobs.abort_all(); busy = false; queue_pending = false;
                        engine = None; web = None;
                        let library = credentials::delete(credentials::LIBRARY);
                        let playback = credentials::delete(credentials::PLAYBACK);
                        let _ = events.send(Event::Disconnected("Signed out".into()));
                        let _ = events.send(Event::SignedOut);
                        let _ = events.send(Event::Busy(false, String::new()));
                        if let Err(error) = library.and(playback) { let _ = events.send(Event::Error(error)); }
                    }
                    Request::Disconnect => {
                        engine = None;
                        let _ = events.send(Event::Disconnected("Spotify playback is disconnected.".into()));
                    }
                    Request::SignInLibrary(id) => {
                        if busy { continue; }
                        generation += 1; jobs.abort_all(); queue_pending = false;
                        busy = true;
                        let _ = events.send(Event::Busy(true,"Continue in your browser to connect Spotify.".into()));
                        let gen = generation;
                        let progress = events.clone();
                        jobs.spawn(async move {
                            let result = async {
                                let id = if id.trim().is_empty() { auth::WEB_CLIENT_ID.to_owned() } else { id.trim().to_owned() };
                                let tokens = auth::authorize(&id,8989,auth::WEB_SCOPES).await?;
                                let mut api = WebApi::verify(id,tokens,None,|seconds| { let _=progress.send(Event::Busy(true,format!("Spotify is busy. Keeping your approval and retrying in {seconds} seconds…"))); }).await?;
                                let page = api.playlists().await.unwrap_or_else(|_| json!({"items":[],"load_failed":true}));
                                Ok(Outcome::Library(api,page))
                            }.await;
                            (gen,true,result)
                        });
                    }
                    Request::Restore => {
                        if busy { continue; }
                        busy = true;
                        let _ = events.send(Event::Busy(true,"Checking Spotify…".into()));
                        let gen = generation;
                        let progress = events.clone();
                        jobs.spawn(async move {
                            let result = async {
                                let Some(saved) = credentials::load::<SavedLibrary>(credentials::LIBRARY)? else { return Ok(Outcome::Empty); };
                                let tokens = auth::refresh(&saved.client_id,&saved.refresh_token).await?;
                                let mut api = WebApi::verify(saved.client_id.clone(),tokens,Some(&saved.account_id),|seconds| { let _=progress.send(Event::Busy(true,format!("Spotify is busy. Keeping your approval and retrying in {seconds} seconds…"))); }).await?;
                                let page = api.playlists().await.unwrap_or_else(|_| json!({"items":[],"load_failed":true}));
                                Ok(Outcome::Library(api,page))
                            }.await;
                            (gen,true,result)
                        });
                    }
                    Request::StartPlayback(fresh) => {
                        if busy { continue; }
                        if web.is_none() { let _ = events.send(Event::Error("Sign in for library access first.".into())); continue; }
                        if premium==Some(false) { let _ = events.send(Event::Error("Local Spotify playback requires a Premium account.".into())); continue; }
                        let account = account_id.clone();
                        generation += 1; jobs.abort_all(); queue_pending = false;
                        engine = None;
                        let _ = events.send(Event::Disconnected("Connecting Spotify…".into()));
                        busy = true;
                        let _ = events.send(Event::Busy(true,"Continue in your browser to enable playback.".into()));
                        let gen = generation;
                        let sender = events.clone();
                        jobs.spawn(async move {
                            let result = async {
                                let saved = if fresh { None } else { credentials::load::<Credentials>(credentials::PLAYBACK)? };
                                let credential = if let Some(credential) = saved {
                                    if credential.username.as_deref() != Some(account.as_str()) { return Err("Saved playback belongs to another account. Approve playback again.".into()); }
                                    credential
                                } else {
                                    let token = auth::authorize(auth::PLAYBACK_CLIENT_ID,8898,&["streaming"]).await?;
                                    Credentials::with_access_token(token.access.clone())
                                };
                                let (engine, credentials) = Engine::start(credential,&account,sender,gen).await?;
                                Ok(Outcome::Playback(engine,credentials))
                            }.await;
                            (gen,true,result)
                        });
                    }
                    Request::RefreshLibrary => {
                        let Some(api) = web.as_ref().cloned() else { continue; };
                        let gen = generation;
                        jobs.spawn(async move {
                            let mut api = api.lock().await;
                            let result = api.playlists().await.map(Outcome::Playlists);
                            (gen,false,result)
                        });
                    }
                    Request::Command(payload) => {
                        if matches!(payload["command"].as_str(),Some("get_state"|"get_auth_state"|"get_queue")) { continue; }
                        let Some(local) = engine.as_ref() else { let _ = events.send(Event::Error("Set up local playback first.".into())); continue; };
                        if payload["command"] == "add_to_queue" {
                            let Some(api) = web.as_ref().cloned() else { continue; };
                            let Some(uri) = payload["uri"].as_str().and_then(|u| spotify_uri(u).ok()).filter(|u|u.starts_with("spotify:track:")) else { continue; };
                            let device = local.session.device_id().to_owned(); let engine_gen = local.generation; let gen = generation;
                            jobs.spawn(async move {
                                let mut api = api.lock().await;
                                let result = async { api.enqueue(&uri,&device).await?; api.queue().await.map(|v|Outcome::Queue(engine_gen,v)) }.await;
                                (gen,false,result)
                            });
                        } else if let Err(error) = local.command(&payload) { let _ = events.send(Event::Error(error)); }
                    }
                }
            }
            result = jobs.join_next(), if !jobs.is_empty() => {
                let Some(Ok((gen,auth_job,result))) = result else { continue; };
                if gen != generation { continue; }
                match result {
                    Ok(Outcome::Feature(id,data)) => { let _ = events.send(Event::Feature {id,data}); }
                    Ok(Outcome::Library(api,page)) => {
                        if web.is_some() && account_id != api.account_id {
                                engine = None;
                                if let Err(error) = credentials::delete(credentials::PLAYBACK) { let _ = events.send(Event::Error(error)); }
                                let _ = events.send(Event::Disconnected("Account changed. Set up playback for this account.".into()));
                                let _ = events.send(Event::SignedOut);
                        }
                        // Any remaining library jobs belong to the previous grant.
                        generation += 1; jobs.abort_all(); queue_pending = false;
                        account_id = api.account_id.clone(); account_name = api.name.clone(); premium = api.premium;
                        if let Err(error) = credentials::save(credentials::LIBRARY,&api.saved()) { let _ = events.send(Event::Error(error)); }
                        let name = api.name.clone(); web = Some(Arc::new(Mutex::new(api)));
                        busy = false; let _ = events.send(Event::Busy(false,String::new()));
                        let _ = events.send(Event::Library { name, playlists:page });
                    }
                    Ok(Outcome::Playback(local,mut credential)) => {
                        if local.has_ended() {
                            busy = false;
                            let _ = events.send(Event::Busy(false,String::new()));
                            let _ = events.send(Event::Error("Spotify playback stopped during connection. Reconnect playback to retry.".into()));
                            continue;
                        }
                        if let Err(error) = credentials::save(credentials::PLAYBACK,&credential) { let _ = events.send(Event::Error(error)); }
                        use zeroize::Zeroize; credential.auth_data.zeroize();
                        engine = Some(local); busy = false;
                        let _ = events.send(Event::Busy(false,String::new()));
                        let _ = events.send(Event::Connected(gen));
                        let _ = events.send(Event::Payload(gen,json!({"type":"auth_state","logged_in":true,"is_active":false})));
                    }
                    Ok(Outcome::Playlists(page)) => {
                        if web.is_some() { let _=events.send(Event::Library {name:account_name.clone(),playlists:page}); }
                    }
                    Ok(Outcome::Queue(engine_gen,value)) => {
                        queue_pending=false;
                        if engine.as_ref().is_some_and(|e|e.generation==engine_gen && e.is_active()) { let _=events.send(Event::Payload(engine_gen,value)); }
                    }
                    Ok(Outcome::Empty) => { busy=false; let _=events.send(Event::Busy(false,String::new())); }
                    Err(error) => {
                        queue_pending=false;
                        if auth_job { busy=false; let _=events.send(Event::Busy(false,String::new())); }
                        let _=events.send(Event::Error(error));
                    }
                }
            }
            _ = queue_timer.tick(), if engine.as_ref().is_some_and(Engine::is_active) && web.is_some() && !queue_pending => {
                let api=web.as_ref().unwrap().clone(); let gen=generation; let engine_gen=engine.as_ref().unwrap().generation;
                queue_pending=true;
                jobs.spawn(async move { let result=api.lock().await.queue().await.map(|v|Outcome::Queue(engine_gen,v)); (gen,false,result) });
            }
        }
        if let Some(api) = &web {
            if let Ok(mut api) = api.try_lock() {
                if api.grant_dirty {
                    api.grant_dirty = false;
                    if let Err(error) = credentials::save(credentials::LIBRARY, &api.saved()) {
                        let _ = events.send(Event::Error(error));
                    }
                }
            }
        }
    }
    jobs.abort_all();
    drop(engine);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_spotify_links_and_rejects_other_inputs() {
        let id = "2JRo0gjbX4GrCqBYdRohoo";
        for input in [
            format!("spotify:track:{id}"),
            format!("https://open.spotify.com/track/{id}?si=share"),
            format!("https://open.spotify.com/intl-he/track/{id}"),
        ] {
            assert_eq!(spotify_uri(&input).unwrap(), format!("spotify:track:{id}"));
        }
        for input in [
            "file:///track/a",
            "https://open.spotify.com.evil.test/track/a",
            "spotify:track:bad",
            "spotify:artist:2JRo0gjbX4GrCqBYdRohoo",
        ] {
            assert!(spotify_uri(input).is_err());
        }
    }
}
