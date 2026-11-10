//! In-process librespot playback. No external daemon, keys, or plaintext login cache.
use crate::spotify::Event;
use librespot_connect::{ConnectConfig, LoadRequest, LoadRequestOptions, Spirc};
use librespot_core::{
    authentication::Credentials,
    config::{DeviceType, SessionConfig},
    Session,
};
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, MixerConfig},
    player::{Player, PlayerEvent},
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    time::Duration,
};
use tokio::task::JoinHandle;

pub struct Engine {
    pub generation: u64,
    pub session: Session,
    pub spirc: Option<Spirc>,
    tasks: Vec<JoinHandle<()>>,
    active: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(spirc) = &self.spirc {
            let _ = spirc.shutdown();
        }
        self.session.shutdown();
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Engine {
    pub async fn start(
        credentials: Credentials,
        account: &str,
        events: Sender<Event>,
        generation: u64,
    ) -> Result<(Self, Credentials), String> {
        // Passing no Cache prevents librespot from writing credentials.json.
        let session = Session::new(SessionConfig::default(), None);
        let mut engine = Self {
            generation,
            session,
            spirc: None,
            tasks: vec![],
            active: Arc::new(AtomicBool::new(false)),
            ended: Arc::new(AtomicBool::new(false)),
        };
        let sink = audio_backend::find(Some("rodio".to_owned()))
            .ok_or("Windows audio backend unavailable.")?;
        let mixer = mixer::find(Some("softvol")).ok_or("Software mixer unavailable.")?(
            MixerConfig::default(),
        )
        .map_err(|_| "Could not initialize volume control.")?;
        let player = Player::new(
            PlayerConfig {
                position_update_interval: Some(Duration::from_secs(1)),
                ..PlayerConfig::default()
            },
            engine.session.clone(),
            mixer.get_soft_volume(),
            move || sink(None, AudioFormat::default()),
        );
        let mut player_events = player.get_player_event_channel();
        let config = ConnectConfig {
            name: "Grove".into(),
            device_type: DeviceType::Computer,
            initial_volume: (u16::MAX as f32 * 0.35) as u16,
            ..ConnectConfig::default()
        };
        let (spirc, task) = tokio::time::timeout(
            Duration::from_secs(75),
            Spirc::new(config, engine.session.clone(), credentials, player, mixer),
        )
        .await
        .map_err(|_| "Spotify playback connection timed out. Retry when online.".to_string())?
        .map_err(|_| {
            "Spotify refused playback approval. Premium is required; try setting up playback again."
                .to_string()
        })?;
        if engine.session.username() != account {
            let _ = spirc.shutdown();
            return Err("Playback approval belongs to a different Spotify account. Use the same account for both approvals.".into());
        }
        let reusable = Credentials {
            username: Some(engine.session.username()),
            auth_type: librespot_protocol::authentication::AuthenticationType::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS,
            auth_data: engine.session.auth_data(),
        };
        engine.spirc = Some(spirc);
        let ended = engine.ended.clone();
        let active = engine.active.clone();
        let notify = events.clone();
        engine.tasks.push(tokio::spawn(async move {
            task.await;
            active.store(false, Ordering::Relaxed);
            ended.store(true, Ordering::Relaxed);
            let _ = notify.send(Event::PlaybackStopped(generation));
        }));
        let active_flag = engine.active.clone();
        engine.tasks.push(tokio::spawn(async move {
            let mut item = json!({}); let mut playing = false; let mut active = false;
            let mut position = 0u32; let mut volume = 35u32; let mut request_id:Option<u64>=None;
            let mut shuffled=false;let mut repeated=false;
            let mut loading=false;
            while let Some(event) = player_events.recv().await {
                match event {
                    PlayerEvent::TrackChanged { audio_item } => {
                        let names = match &audio_item.unique_fields {
                            librespot_metadata::audio::item::UniqueFields::Track { artists, .. } => artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
                            librespot_metadata::audio::item::UniqueFields::Episode { show_name, .. } => vec![show_name.clone()],
                            _ => vec![],
                        };
                        let creators = names.iter().map(|name| json!({"entity":{"decorations":{"identity":{"name":name}}}})).collect::<Vec<_>>();
                        item = json!({"uri":audio_item.uri,"artwork_url":audio_item.covers.first().map(|cover|&cover.url),"decorations":{"identity":{"name":audio_item.name},"creators":creators,"playback":{"duration_ms":audio_item.duration_ms}}});
                        position = 0;
                    }
                    PlayerEvent::Playing { position_ms, play_request_id, .. } => { playing = true; loading=false; active = true; position = position_ms; request_id=Some(play_request_id); }
                    PlayerEvent::Paused { position_ms, .. } => { playing = false; loading=false; position = position_ms; }
                    PlayerEvent::PositionChanged { position_ms, .. } | PlayerEvent::PositionCorrection { position_ms, .. } | PlayerEvent::Seeked { position_ms, .. } => { position = position_ms; }
                    PlayerEvent::VolumeChanged { volume: level } => {
                        volume = (level as f32 / u16::MAX as f32 * 100.0).round() as u32;
                        // A volume event carries no new position. Re-emitting the last
                        // full sample here would rewind the UI's playback clock.
                        let _ = events.send(Event::Payload(generation,json!({"type":"volume_changed","volume":volume})));
                        continue;
                    }
                    PlayerEvent::EndOfTrack { play_request_id, .. } => {
                        let _=events.send(Event::Payload(generation,json!({"type":"end_of_track","play_request_id":play_request_id})));playing=false;
                    }
                    PlayerEvent::Stopped { .. } => { playing = false; }
                    PlayerEvent::Loading { .. } => {
                        loading=true;playing=false;
                        let _=events.send(Event::Payload(generation,json!({"type":"loading"})));continue;
                    }
                    PlayerEvent::ShuffleChanged {shuffle} => {shuffled=shuffle;let _=events.send(Event::Payload(generation,json!({"type":"options_changed","options":{"shuffle":shuffle}})));continue;}
                    PlayerEvent::RepeatChanged {context,..} => {repeated=context;let _=events.send(Event::Payload(generation,json!({"type":"options_changed","options":{"repeat_context":context}})));continue;}
                    PlayerEvent::SessionConnected { .. } => { active = true; }
                    PlayerEvent::SessionDisconnected { .. } => { playing = false; active = false; }
                    PlayerEvent::Unavailable { .. } => {
                        let _ = events.send(Event::Error("Spotify could not load this track. Try another song or reconnect playback.".into()));
                        playing = false;
                    }
                    _ => continue,
                }
                active_flag.store(active, Ordering::Relaxed);
                let actions = if item["uri"].as_str().is_some() {
                    json!({"play":{},"pause":{},"seek":{},"skip_next":{},"skip_prev":{},"add_to_queue":{},"shuffle":{},"set_repeat":{}})
                } else { json!({"play":{}}) };
                let _ = events.send(Event::Payload(generation,json!({"type":"playback_state","loading":loading,"play_request_id":request_id,"status":if playing {"playing"} else {"paused"},"item":item,"is_active":active,"position":{"position_ms":position,"speed":if playing {1} else {0}},"volume":volume,"options":{"shuffle":shuffled,"repeat_context":repeated},"available_actions":actions})));
            }
        }));
        Ok((engine, reusable))
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn has_ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    pub fn command(&self, payload: &Value) -> Result<(), String> {
        let spirc = self.spirc.as_ref().ok_or("Playback is not connected.")?;
        let result = match payload["command"].as_str().unwrap_or_default() {
            "play" | "jam_load" => {
                if let Some(uri) = payload["uri"].as_str() {
                    spirc.activate().map_err(|_| "Could not activate Grove.")?;
                    let options = LoadRequestOptions {
                        start_playing: payload["playing"].as_bool().unwrap_or(true),
                        seek_to: payload["position_ms"]
                            .as_u64()
                            .unwrap_or(0)
                            .min(u32::MAX as u64) as u32,
                        ..LoadRequestOptions::default()
                    };
                    let request = if uri.starts_with("spotify:track:")
                        || uri.starts_with("spotify:episode:")
                    {
                        LoadRequest::from_tracks(vec![uri.to_owned()], options)
                    } else {
                        LoadRequest::from_context_uri(uri.to_owned(), options)
                    };
                    spirc.load(request)
                } else {
                    spirc.play()
                }
            }
            "pause" => spirc.pause(),
            "skip_next" => spirc.next(),
            "skip_prev" => spirc.prev(),
            "seek" => spirc.set_position_ms(
                payload["position_ms"]
                    .as_u64()
                    .unwrap_or(0)
                    .min(u32::MAX as u64) as u32,
            ),
            "set_volume" => spirc.set_volume(
                (payload["volume"].as_f64().unwrap_or(35.0).clamp(0.0, 100.0) / 100.0
                    * u16::MAX as f64)
                    .round() as u16,
            ),
            "activate" => spirc.activate(),
            "set_shuffle" => spirc.shuffle(payload["enabled"].as_bool().unwrap_or(false)),
            "set_repeat_context" => spirc.repeat(payload["enabled"].as_bool().unwrap_or(false)),
            _ => return Ok(()),
        };
        result.map_err(|_| "Spotify playback command failed. Reconnect playback and retry.".into())
    }
}
