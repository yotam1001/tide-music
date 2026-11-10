#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod artwork;
mod auth;
mod beta;
mod credentials;
mod engine;
mod features;
mod jam;
mod jam_sync;
mod jam_transport;
mod library;
mod local;
mod media;
mod preferences;
mod spotify;
mod state;
mod ui_review;
mod web_api;

use serde_json::json;
use slint::{ComponentHandle, ModelRc, VecModel};
use spotify::{command, Event, Request, RequestSender};
use state::{clock, State};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
slint::include_modules!();

fn send(tx: &RequestSender, payload: serde_json::Value) {
    let _ = tx.send(Request::Command(payload));
}

fn refresh(ui: &GroveWindow, state: &State) {
    ui.set_connected(state.connected);
    ui.set_authenticated(state.authenticated);
    ui.set_playing(state.playing);
    ui.set_can_play(state.allows("play"));
    ui.set_can_pause(state.allows("pause"));
    ui.set_can_seek(state.allows("seek"));
    ui.set_can_next(state.allows("skip_next"));
    ui.set_can_previous(state.allows("skip_prev"));
    ui.set_can_queue(state.allows("add_to_queue"));
    ui.set_can_shuffle(state.allows("shuffle"));
    ui.set_can_repeat(state.allows("set_repeat"));
    ui.set_shuffle(state.shuffle);
    ui.set_repeat(state.repeat);
    ui.set_track_title(
        if state.track.title.is_empty() {
            "Nothing playing yet"
        } else {
            &state.track.title
        }
        .into(),
    );
    ui.set_artist(
        if !state.authenticated {
            if ui.get_library_ready() {
                "Spotify library connected"
            } else {
                "Connect Spotify to start listening"
            }
        } else if state.track.artist.is_empty() {
            "Spotify"
        } else {
            &state.track.artist
        }
        .into(),
    );
    ui.set_context_name(
        if state.context.is_empty() {
            ""
        } else {
            &state.context
        }
        .into(),
    );
    ui.set_position(state.position());
    ui.set_duration(state.track.duration);
    ui.set_volume(state.volume);
    ui.set_elapsed(clock(state.position()).into());
    ui.set_total(clock(state.track.duration).into());
    ui.set_status(
        if !state.connected {
            if ui.get_library_ready() {
                "Library connected · playback off"
            } else {
                "Connect Spotify"
            }
        } else if !state.authenticated {
            "Finish connecting Spotify"
        } else if !state.active {
            "Ready to play"
        } else if state.playing {
            "Playing on Grove"
        } else {
            "Connected · paused"
        }
        .into(),
    );
}

fn refresh_queue(ui: &GroveWindow, state: &State) {
    ui.set_tracks(ModelRc::new(VecModel::from(
        state
            .queue
            .iter()
            .map(|track| GroveTrack {
                title: track.title.clone().into(),
                artist: track.artist.clone().into(),
                uri: track.uri.clone().into(),
                duration: clock(track.duration).into(),
            })
            .collect::<Vec<_>>(),
    )));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if beta::command_line()? {
        return Ok(());
    }
    std::env::set_var("SLINT_BACKEND", "winit-software");
    let ui = GroveWindow::new()?;
    ui.set_app_version(format!("Grove {} · Private beta", env!("CARGO_PKG_VERSION")).into());
    if std::env::args().any(|a| a == "--connect-account") {
        ui.set_page(2);
    }
    let smoke = std::env::args().position(|s| s == "--smoke-ui");
    let review_path = std::env::args()
        .position(|s| s == "--review-ui")
        .map(|index| {
            std::env::args()
                .nth(index + 1)
                .ok_or("--review-ui needs an output directory")
        })
        .transpose()?;
    let feature_review_path = std::env::args()
        .position(|s| s == "--review-features")
        .map(|i| {
            std::env::args()
                .nth(i + 1)
                .ok_or("--review-features needs an output directory")
        })
        .transpose()?;
    let local_review_path = std::env::args()
        .position(|s| s == "--review-local-audio")
        .map(|i| {
            std::env::args()
                .nth(i + 1)
                .ok_or("--review-local-audio needs an output directory")
        })
        .transpose()?;
    let offline = smoke.is_some()
        || review_path.is_some()
        || feature_review_path.is_some()
        || local_review_path.is_some();
    if [
        smoke.is_some(),
        review_path.is_some(),
        feature_review_path.is_some(),
        local_review_path.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count()
        > 1
    {
        return Err("Choose one UI review mode.".into());
    }
    let state = Rc::new(RefCell::new(State::default()));
    let (tx, events, handle) = spotify::worker();
    let (jam_tx, jam_events, jam_handle) = jam_transport::worker();
    let features = features::Features::install(&ui, tx.clone(), offline);
    beta::install(&ui, state.clone(), offline);
    let live_check = beta::live_check(&ui, state.clone(), offline)?;
    if !offline {
        ui.set_jam_address(jam_transport::suggested_address().into());
    }
    let (local, local_timer) =
        local::install(&ui, tx.clone(), state.clone(), features.clone(), offline);
    let weak = ui.as_weak();
    let sender = jam_tx.clone();
    ui.on_jam_create(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_local_active() {
                ui.set_jam_message("Return to Spotify before starting a jam.".into());
                return;
            }
            if ui.get_jam_busy() || ui.get_jam_connected() {
                return;
            }
            ui.set_jam_busy(true);
            ui.set_jam_message("Starting a nearby jam…".into());
            let _ = sender.send(jam_transport::Request::Host {
                name: ui.get_jam_name().to_string(),
                address: ui.get_jam_address().to_string(),
            });
        }
    });
    let weak = ui.as_weak();
    let sender = jam_tx.clone();
    ui.on_jam_join(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_local_active() {
                ui.set_jam_message("Return to Spotify before joining a jam.".into());
                return;
            }
            if ui.get_jam_busy() || ui.get_jam_connected() {
                return;
            }
            ui.set_jam_busy(true);
            ui.set_jam_message("Connecting to the host…".into());
            let _ = sender.send(jam_transport::Request::Join {
                name: ui.get_jam_name().to_string(),
                invite: ui.get_jam_join_invite().to_string(),
            });
        }
    });
    let sender = jam_tx.clone();
    ui.on_jam_leave(move || {
        let _ = sender.send(jam_transport::Request::Leave);
    });
    let weak = ui.as_weak();
    let sender = jam_tx.clone();
    let feature_state = features.clone();
    ui.on_jam_add(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_jam_tide_host() {
                ui.set_jam_message("Choose the next song in Tide on the host's device.".into());
                return;
            }
            match spotify::spotify_uri(&ui.get_jam_track_link()) {
                Ok(uri) if uri.starts_with("spotify:track:") => {
                    if ui.get_jam_track_title().trim().is_empty() {
                        feature_state
                            .borrow_mut()
                            .resolve_jam(&ui, uri, sender.clone());
                        return;
                    }
                    ui.set_jam_message(String::new().into());
                    let _ = sender.send(jam_transport::Request::Add {
                        uri,
                        title: ui.get_jam_track_title().to_string(),
                    });
                }
                _ => ui.set_jam_message("Add an individual Spotify track link.".into()),
            }
        }
    });
    let sender = jam_tx.clone();
    ui.on_jam_remove(move |id| {
        let _ = sender.send(jam_transport::Request::Remove(id.to_string()));
    });
    let sender = jam_tx.clone();
    ui.on_jam_next(move || {
        let _ = sender.send(jam_transport::Request::Next);
    });

    let sender = tx.clone();
    let weak = ui.as_weak();
    let feature_state = features.clone();
    ui.on_connect_engine(move || {
        if let Some(ui) = weak.upgrade() {
            feature_state.borrow_mut().invalidate(&ui);
        }
        let _ = sender.send(Request::StartPlayback(false));
    });
    let weak = ui.as_weak();
    let sender = tx.clone();
    let feature_state = features.clone();
    ui.on_sign_in_library(move || {
        if let Some(ui) = weak.upgrade() {
            feature_state.borrow_mut().invalidate(&ui);
            let _ = sender.send(Request::SignInLibrary(ui.get_client_id().to_string()));
        }
    });
    let sender = tx.clone();
    ui.on_sign_out(move || {
        let _ = sender.send(Request::SignOut);
    });
    let sender = tx.clone();
    let weak = ui.as_weak();
    let feature_state = features.clone();
    ui.on_cancel_sign_in(move || {
        if let Some(ui) = weak.upgrade() {
            feature_state.borrow_mut().invalidate(&ui);
        }
        let _ = sender.send(Request::Cancel);
    });
    let sender = tx.clone();
    let weak = ui.as_weak();
    let feature_state = features.clone();
    ui.on_approve_playback(move || {
        if let Some(ui) = weak.upgrade() {
            feature_state.borrow_mut().invalidate(&ui);
        }
        let _ = sender.send(Request::StartPlayback(true));
    });
    let weak = ui.as_weak();
    let sender = tx.clone();
    ui.on_refresh_library(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_message("Refreshing your playlists…".into());
            let _ = sender.send(Request::RefreshLibrary);
        }
    });
    let weak = ui.as_weak();
    let sender = tx.clone();
    let files = local.clone();
    let playback_state = state.clone();
    ui.on_play_uri(move |uri| {
        if let Some(ui) = weak.upgrade() {
            if ui.get_jam_connected() && !ui.get_jam_host() && ui.get_jam_following() {
                ui.set_message("Stop following the jam before choosing your own music.".into());
                return;
            }
            if !ui.get_authenticated() {
                ui.set_page(2);
                return;
            }
            match spotify::spotify_uri(&uri) {
                Ok(uri) => {
                    if files.borrow().active() {
                        files.borrow_mut().stop(&ui, &playback_state.borrow());
                    }
                    send(
                        &sender,
                        json!({"type":"command","command":"play","uri":uri}),
                    );
                }
                Err(error) => ui.set_message(error.into()),
            }
        }
    });
    let sender = tx.clone();
    ui.on_disconnect_engine(move || {
        let _ = sender.send(Request::Disconnect);
    });

    let weak = ui.as_weak();
    let sender = tx.clone();
    let files = local.clone();
    let playback_state = state.clone();
    ui.on_play_link(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_jam_connected() && !ui.get_jam_host() && ui.get_jam_following() {
                ui.set_message("Stop following the jam before choosing your own music.".into());
                return;
            }
            if !ui.get_authenticated() {
                ui.set_page(2);
                return;
            }
            match spotify::spotify_uri(&ui.get_link()) {
                Ok(uri) => {
                    if files.borrow().active() {
                        files.borrow_mut().stop(&ui, &playback_state.borrow());
                    }
                    send(
                        &sender,
                        json!({"type":"command","command":"play","uri":uri}),
                    );
                    ui.set_message("Playback requested. Waiting for Spotify…".into());
                }
                Err(error) => ui.set_message(error.into()),
            }
        }
    });
    let weak = ui.as_weak();
    let feature_state = features.clone();
    ui.on_enqueue_link(move || {
        if let Some(ui) = weak.upgrade() {
            feature_state.borrow_mut().enqueue_link(&ui, &ui.get_link());
        }
    });
    let sender = tx.clone();
    let current = state.clone();
    let feature_state = features.clone();
    let weak = ui.as_weak();
    let files = local.clone();
    ui.on_transport(move |name| {
        if let Some(ui)=weak.upgrade(){if files.borrow_mut().transport(&ui,&name){return;}}
        if let Some(ui)=weak.upgrade(){if ui.get_jam_connected()&&!ui.get_jam_host()&&ui.get_jam_following(){return;}}
        if matches!(name.as_str(),"shuffle"|"repeat") {
            let s=current.borrow();let is_shuffle=name=="shuffle";
            if s.allows(if is_shuffle{"shuffle"}else{"set_repeat"}){send(&sender,json!({"command":if is_shuffle{"set_shuffle"}else{"set_repeat_context"},"enabled":if is_shuffle{!s.shuffle}else{!s.repeat}}));}return;
        }
        let action = match name.as_str() {
            "skip_next" => "skip_next",
            "skip_prev" => "skip_prev",
            "play" => "play",
            "pause" => "pause",
            _ => return,
        };
        if let Some(ui)=weak.upgrade() {
            if ui.get_jam_connected() && !ui.get_jam_host() && ui.get_jam_following(){return;}
            if action=="skip_next" && feature_state.borrow_mut().next(&ui,&current.borrow()){return;}
        }
        if current.borrow().allows(action) {
            send(&sender, command(action));
        }
    });
    let sender = tx.clone();
    let current = state.clone();
    let weak = ui.as_weak();
    let files = local.clone();
    ui.on_seek(move |position| {
        if let Some(ui)=weak.upgrade(){if files.borrow().seek(&ui,position){return;}}
        if let Some(ui)=weak.upgrade(){if ui.get_jam_connected()&&!ui.get_jam_host()&&ui.get_jam_following(){return;}}
        let state = current.borrow();
        if state.allows("seek") { send(&sender, json!({"type":"command","command":"seek","position_ms": position.clamp(0.0, state.track.duration) as u64})); }
    });
    let sender = tx.clone();
    let current = state.clone();
    let feature_state = features.clone();
    let weak = ui.as_weak();
    let files = local.clone();
    ui.on_set_volume(move |volume| {
        if let Some(ui)=weak.upgrade(){feature_state.borrow_mut().volume(&ui,volume.clamp(0.0,100.0));if files.borrow_mut().volume(&ui,volume){return;}}
        if current.borrow().authenticated { send(&sender, json!({"type":"command","command":"set_volume","volume":volume.clamp(0.0,100.0).round() as u32})); }
    });
    let weak = ui.as_weak();
    let expanded_size = Rc::new(Cell::new(slint::PhysicalSize::new(1200, 800)));
    ui.on_switch_mini(move || {
        if let Some(ui) = weak.upgrade() {
            let mini = !ui.get_mini();
            if mini {
                expanded_size.set(ui.window().size());
            }
            ui.set_mini(mini);
            if mini {
                ui.window().set_size(slint::LogicalSize::new(600.0, 104.0));
            } else {
                ui.window().set_size(expanded_size.get());
            }
        }
    });

    let poll = slint::Timer::default();
    let weak = ui.as_weak();
    let current = state.clone();
    let playback_generation = Rc::new(Cell::new(0u64));
    let room_sender = jam_tx.clone();
    let feature_state = features.clone();
    let sender = tx.clone();
    let files = local.clone();
    poll.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut state = current.borrow_mut();
            let mut changed = false;
            while let Ok(event) = events.try_recv() {
                changed = true;
                match event {
                    Event::Feature { id, data } => feature_state.borrow_mut().response(&ui,id,data),
                    Event::Connected(generation) => {
                        playback_generation.set(generation);
                        *state = State::default();
                        state.connected = true;
                        refresh_queue(&ui, &state);
                        ui.set_connecting(false);
                        ui.set_message(String::new().into());
                    }
                    Event::Disconnected(reason) => {
                        playback_generation.set(0);
                        *state = State::default();
                        refresh_queue(&ui, &state);
                        ui.set_connecting(false);
                        ui.set_message(reason.into());
                    }
                    Event::PlaybackStopped(generation) => {
                        if generation != playback_generation.get() {continue;}
                        playback_generation.set(0); *state=State::default(); refresh_queue(&ui,&state);
                        ui.set_message("Playback stopped. Reconnect Spotify from Account.".into());
                    }
                    Event::Payload(generation, payload) => {
                        if generation != playback_generation.get() {
                            continue;
                        }
                        let event_type = payload["type"].as_str().unwrap_or_default();
                        if event_type=="loading"{state.apply(&payload);ui.set_message("Loading Spotify audio…".into());continue;}
                        if event_type=="auth_state"&&payload["logged_in"]==true{send(&sender,json!({"command":"set_volume","volume":feature_state.borrow().prefs.volume.round() as u32}));}
                        if event_type=="end_of_track" {feature_state.borrow_mut().ended(&ui,&state,payload["play_request_id"].as_u64());continue;}
                        if event_type == "error" {
                            ui.set_message(
                                payload["message"]
                                    .as_str()
                                    .unwrap_or("Spotify rejected the command.")
                                    .into(),
                            );
                            continue;
                        }
                        state.apply(&payload);
                        if state.playing&&files.borrow().playing_file(){files.borrow_mut().stop(&ui,&state);ui.set_message("Spotify playback resumed.".into());}
                        if event_type == "queue_changed" || !state.active {
                            refresh_queue(&ui, &state);
                        }
                        if state.playing && (ui.get_message().starts_with("Playback requested")||ui.get_message().starts_with("Loading Spotify")) {
                            ui.set_message(String::new().into());
                        }
                    }
                    Event::Busy(busy, message) => {
                        ui.set_connecting(busy);
                        if !message.is_empty() {
                            ui.set_message(message.into());
                        } else if [
                            "Checking Spotify",
                            "Continue in your browser",
                            "Setting up local",
                            "Connecting Spotify",
                            "Spotify is busy. Keeping your approval",
                        ]
                        .iter()
                        .any(|prefix| ui.get_message().starts_with(prefix))
                        {
                            ui.set_message(String::new().into());
                        }
                    }
                    Event::Error(error) => {
                        ui.set_message(error.into());
                    }
                    Event::SignedOut => {
                        if files.borrow().active(){files.borrow_mut().stop(&ui,&state);}
                        feature_state.borrow_mut().clear(&ui);
                        let _=room_sender.send(jam_transport::Request::Leave);
                        ui.set_library_ready(false);
                        ui.set_account_name("Not signed in".into());
                        ui.set_playlists(ModelRc::default());
                        ui.set_owned_playlists(ModelRc::default());
                        ui.set_collection_uri(String::new().into());
                        ui.set_library_note(String::new().into());
                    }
                    Event::Library { name, playlists } => {
                        ui.set_library_ready(true);
                        ui.set_feature_message("".into());
                        feature_state.borrow_mut().grant_ready(&ui);
                        if ui.get_jam_name()=="Listener"{ui.set_jam_name(name.clone().into());}
                        ui.set_account_name(name.into());
                        ui.set_owned_playlists(ModelRc::new(VecModel::from(playlists["items"].as_array().into_iter().flatten().filter(|p|p["owner"]["id"].is_string()&&p["owner"]["id"]==playlists["account"]).filter_map(|p|Some(GrovePlaylist{title:p["name"].as_str()?.into(),uri:p["uri"].as_str()?.into(),subtitle:"Your playlist".into()})).collect::<Vec<_>>())));
                        let rows = playlists["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| {
                                Some(GrovePlaylist {
                                    title: p["name"].as_str()?.into(),
                                    subtitle: p["owner"]["display_name"]
                                        .as_str()
                                        .unwrap_or("Playlist")
                                        .into(),
                                    uri: p["uri"].as_str()?.into(),
                                })
                            })
                            .collect::<Vec<_>>();
                        ui.set_playlists(ModelRc::new(VecModel::from(rows)));
                        ui.set_playlists_more(!playlists["next"].is_null());
                        ui.set_library_note(
                            if playlists["load_failed"] == true {
                                "Playlist loading failed. Use Refresh to retry."
                            } else if !playlists["next"].is_null() {
                                "Showing the first 50 playlists."
                            } else {
                                "Your saved playlists"
                            }
                            .into(),
                        );
                        if ui.get_message().starts_with("Refreshing") {
                            ui.set_message(String::new().into());
                        }
                    }

                }
            }
            if files.borrow().active() {
                // The local timer publishes its independent backend state.
            } else if changed {
                refresh(&ui, &state);
            } else if state.playing {
                ui.set_position(state.position());
                ui.set_elapsed(clock(state.position()).into());
            }
            feature_state.borrow_mut().tick(&ui,&state);
        },
    );
    let media = media::install(&ui, state.clone(), offline);

    let jam_timer = slint::Timer::default();
    let weak = ui.as_weak();
    let current = state.clone();
    let sender = jam_tx.clone();
    let spotify_sender = tx.clone();
    let jam_features = features.clone();
    let mut member_id = String::new();
    let mut room_id = String::new();
    let mut revision = 0u64;
    let mut target: Option<jam_sync::Target> = None;
    let mut follower = jam_sync::Follower::default();
    let mut published = Instant::now() - Duration::from_secs(2);
    let mut was_following = true;
    let mut quiet = false;
    let mut pending: Option<(String, String, Option<u64>, Instant)> = None;
    jam_timer.start(slint::TimerMode::Repeated,Duration::from_millis(100),move||{
        let Some(ui)=weak.upgrade()else{return;};let state=current.borrow();
        while let Ok(event)=jam_events.try_recv(){
            match event {
                jam_transport::Event::Joined{invite,member_id:member,host}=>{
                    jam_features.borrow_mut().reset_jam(&ui);
                    ui.set_jam_busy(false);
                    member_id=member;room_id.clear();revision=0;target=None;follower=jam_sync::Follower::default();pending=None;quiet=false;was_following=true;
                    ui.set_jam_connected(true);ui.set_jam_host(host);ui.set_jam_following(true);ui.set_jam_invite(invite.into());ui.set_jam_message(String::new().into());
                }
                jam_transport::Event::Snapshot{state:room,host_sent_ms,transit_ms}=>{
                    if !room_id.is_empty() && room.room_id!=room_id{continue;}
                    if room.revision<=revision{continue;}room_id=room.room_id.clone();revision=room.revision;
                    if let Some(playback)=&room.playback { ui.set_jam_tide_host(playback.uri.starts_with("youtube:")); }
                    ui.set_jam_members(room.members.iter().map(|m|format!("{}{}",m.name,if m.host{" (host)"}else{""})).collect::<Vec<_>>().join(" · ").into());
                    ui.set_jam_tracks(ModelRc::new(VecModel::from(room.queue.iter().map(|entry|GroveJamTrack{
                        id:entry.id.clone().into(),title:entry.title.clone().into(),
                        subtitle:format!("Added by {} · {}",entry.member_name,match entry.delivery{jam::Delivery::Waiting=>"Waiting",jam::Delivery::Sending=>"Starting on host…",jam::Delivery::Unconfirmed=>"Unconfirmed · check host playback"}).into(),
                        removable:entry.delivery!=jam::Delivery::Sending && (ui.get_jam_host()||entry.member_id==member_id),
                    }).collect::<Vec<_>>())));
                    target=room.playback.map(|playback|jam_sync::Target{playback,host_sent_ms,received:Instant::now(),transit_ms});
                    if target.is_some(){quiet=false;}
                }
                jam_transport::Event::Play{id,uri}=>{
                    if !state.authenticated {let _=sender.send(jam_transport::Request::Complete{id,success:false});ui.set_jam_message("Enable your Spotify playback before starting a shared track.".into());continue;}
                    pending=Some((id,uri.clone(),state.play_request_id,Instant::now()));
                    send(&spotify_sender,json!({"command":"play","uri":uri}));
                }
                jam_transport::Event::Left(message)=>{
                    jam_features.borrow_mut().reset_jam(&ui);
                    ui.set_jam_busy(false);
                    if ui.get_jam_connected()&&!ui.get_jam_host()&&ui.get_jam_following()&&state.playing{send(&spotify_sender,command("pause"));}
                    ui.set_jam_connected(false);ui.set_jam_host(false);ui.set_jam_following(true);ui.set_jam_invite(String::new().into());ui.set_jam_members(String::new().into());ui.set_jam_tracks(ModelRc::default());ui.set_jam_message(message.into());ui.set_jam_sync_status("Not in a jam".into());
                    target=None;pending=None;room_id.clear();revision=0;follower=jam_sync::Follower::default();
                }
                jam_transport::Event::Error(message)=>{ui.set_jam_busy(false);ui.set_jam_message(message.into());},
            }
        }
        if !ui.get_jam_connected(){return;}
        if ui.get_jam_host(){
            let shareable=state.authenticated&&state.active&&state.track.uri.starts_with("spotify:track:");
            ui.set_jam_sync_status(if shareable{"Sharing host playback · listeners follow on their own devices"}else if state.authenticated&&state.active&&!state.track.uri.is_empty(){"Choose a Spotify track on the host · other audio types cannot be shared yet"}else{"Connect Spotify and play a track to start listening together"}.into());
            if published.elapsed()>=Duration::from_secs(1){
                published=Instant::now();
                let frame=if shareable{
                    Some(jam::Playback{uri:state.track.uri.clone(),title:state.track.title.clone(),artist:state.track.artist.clone(),position_ms:state.position().max(0.0) as u64,playing:state.playing,sampled_at_ms:jam::unix_ms()})
                }else{None};let _=sender.send(jam_transport::Request::Publish(frame));
            }
            if let Some((id,uri,before,started))=&pending{
                let confirmed=state.track.uri==*uri&&state.playing&&state.play_request_id!=*before;
                if confirmed||started.elapsed()>Duration::from_secs(15){
                    let _=sender.send(jam_transport::Request::Complete{id:id.clone(),success:confirmed});
                    if !confirmed{ui.set_jam_message("Playback was not confirmed. Check the host player before removing the unconfirmed entry.".into());}
                    pending=None;
                }
            }
        }else{
            if was_following!=ui.get_jam_following(){follower=jam_sync::Follower::default();quiet=false;was_following=ui.get_jam_following();}
            if !ui.get_jam_following(){ui.set_jam_sync_status("Following stopped · your playback is independent".into());return;}
            if !state.authenticated{ui.set_jam_sync_status("Enable Spotify playback in Account to listen".into());return;}
            let Some(remote)=&target else{
                ui.set_jam_matching(false);
                ui.set_jam_sync_status("Waiting for the host to play a track".into());
                if !quiet&&state.playing{send(&spotify_sender,command("pause"));quiet=true;}return;
            };
            if remote.stale(Instant::now()) { ui.set_jam_sync_status("Host updates delayed · waiting to sync".into()); return; }
            let Some(uri)=jam_features.borrow_mut().paired_host(&ui,&remote.playback) else {
                ui.set_jam_sync_status("Choose the matching Spotify song to follow Tide".into());
                if !quiet && state.playing { send(&spotify_sender,command("pause")); quiet=true; }
                return;
            };
            quiet=false;
            let mapped=jam_sync::Target{playback:jam::Playback{uri,..remote.playback.clone()},host_sent_ms:remote.host_sent_ms,received:remote.received,transit_ms:remote.transit_ms};
            let remote=&mapped;
            if (!state.active||state.track.uri!=remote.playback.uri)&&follower.exhausted(&remote.playback.uri,Instant::now()){
                ui.set_jam_sync_status("Could not load the host track. Check Connection, then stop and restart following to retry.".into());return;
            }
            ui.set_jam_sync_status(if remote.stale(Instant::now()){"Host updates delayed · waiting to sync".into()}else if !state.active||state.track.uri!=remote.playback.uri{format!("Loading host track · {}",remote.playback.title).into()}else if state.playing!=remote.playback.playing{"Synchronizing play/pause with the host…".into()}else{format!("Following {} · {}",remote.playback.title,if remote.playback.playing{"Playing"}else{"Paused"}).into()});
            if let Some(correction)=follower.plan(remote,jam_sync::Local{uri:if state.active{&state.track.uri}else{""},position_ms:state.position().max(0.0) as u64,playing:state.playing,ready:state.authenticated},Instant::now()){
                let payload=match correction {
                    jam_sync::Correction::Load{uri,position_ms,playing}=>json!({"command":"jam_load","uri":uri,"position_ms":position_ms,"playing":playing}),
                    jam_sync::Correction::Seek(position_ms)=>json!({"command":"seek","position_ms":position_ms}),
                    jam_sync::Correction::Play=>command("play"),jam_sync::Correction::Pause=>command("pause"),
                };send(&spotify_sender,payload);
            }
        }
    });

    let smoke_timer = slint::Timer::default();
    if let Some(index) = smoke {
        ui.set_page(0);
        poll.stop();
        jam_timer.stop();
        let output = std::env::args()
            .nth(index + 1)
            .ok_or("--smoke-ui needs an output directory")?;
        std::fs::create_dir_all(&output)?;
        let weak = ui.as_weak();
        let mut step = 0;
        smoke_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(600),
            move || {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                if step == 0 {
                    step += 1;
                    return;
                }
                let name = match step {
                    1 => "listen",
                    2 => "connection",
                    3 => "queue",
                    4 => "compact",
                    5 => "mini",
                    6 => "restored",
                    7 => "native-reference",
                    8 => "library",
                    9 => "liked-songs",
                    10 => "connection-narrow",
                    11 => "minimum",
                    12 => "jam-setup",
                    13 => "jam-host",
                    14 => "jam-guest",
                    15 => "jam-narrow",
                    16 => "playing",
                    17 => "playing-narrow",
                    18 => "welcome",
                    _ => "welcome-narrow",
                };
                let pixels = ui.window().take_snapshot().expect("Native snapshot");
                image::RgbaImage::from_raw(
                    pixels.width(),
                    pixels.height(),
                    pixels.as_bytes().to_vec(),
                )
                .unwrap()
                .save(std::path::Path::new(&output).join(format!("{name}.png")))
                .unwrap();
                match step {
                    1 => ui.set_page(2),
                    2 => {
                        ui.set_page(1);
                        ui.set_tracks(ModelRc::new(VecModel::from(vec![GroveTrack {
                            title: "Native test track".into(),
                            artist: "Synthetic fixture · playback inactive".into(),
                            duration: "3:30".into(),
                            uri: String::new().into(),
                        }])));
                    }
                    3 => {
                        ui.set_page(0);
                        ui.set_tracks(ModelRc::default());
                        ui.window().set_size(slint::LogicalSize::new(760.0, 640.0));
                    }
                    4 => ui.invoke_switch_mini(),
                    5 => {
                        ui.invoke_switch_mini();
                        ui.set_page(0);
                    }
                    6 => ui
                        .window()
                        .set_size(slint::LogicalSize::new(1536.0, 1024.0)),
                    7 => {
                        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
                        ui.set_page(3);
                        ui.set_library_ready(true);
                        ui.set_account_name("Visual test library · synthetic metadata".into());
                        ui.set_library_note("Synthetic fixtures · playback inactive".into());
                        ui.set_playlists(ModelRc::new(VecModel::from(vec![GrovePlaylist {
                            title: "Evening rotation".into(),
                            subtitle: "Synthetic playlist".into(),
                            uri: "spotify:playlist:2JRo0gjbX4GrCqBYdRohoo".into(),
                        }])));
                    }
                    8 => {
                        ui.set_collection_uri("liked".into());
                        ui.set_collection_title("Liked Songs".into());
                        ui.set_feature_tracks(ModelRc::new(VecModel::from(vec![
                            GroveFeatureTrack {
                                title: "Native test track".into(),
                                artist: "Synthetic fixture · playback inactive".into(),
                                duration: "3:30".into(),
                                uri: "spotify:track:2JRo0gjbX4GrCqBYdRohoo".into(),
                                artist_uri: Default::default(),
                                cover: Default::default(),
                            },
                        ])));
                    }
                    9 => {
                        ui.set_page(2);
                        ui.set_library_ready(false);
                        ui.set_account_name("Not signed in".into());
                        ui.window().set_size(slint::LogicalSize::new(700.0, 540.0));
                    }
                    10 => {
                        ui.set_page(0);
                    }
                    11 => {
                        ui.set_page(4);
                        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
                    }
                    12 => {
                        ui.set_jam_connected(true);
                        ui.set_jam_host(true);
                        ui.set_jam_invite(
                            "grove-jam://127.0.0.1:9876/visual-test-invitation-only".into(),
                        );
                        ui.set_jam_members("You (host) · Alice · Bob · Synthetic fixtures".into());
                        ui.set_jam_sync_status(
                            "Sharing host playback · listeners follow on their own devices".into(),
                        );
                        ui.set_jam_tracks(ModelRc::new(VecModel::from(vec![GroveJamTrack {
                            id: "fixture".into(),
                            title: "Evening song · synthetic fixture".into(),
                            subtitle: "Added by Alice · Waiting".into(),
                            removable: true,
                        }])));
                    }
                    13 => {
                        ui.set_jam_host(false);
                        ui.set_jam_sync_status(
                            "Following Evening song · Playing · Synthetic fixture".into(),
                        );
                    }
                    14 => ui.window().set_size(slint::LogicalSize::new(700.0, 540.0)),
                    15 => {
                        ui.set_page(0);
                        ui.set_jam_connected(false);
                        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
                        ui.set_authenticated(true);
                        ui.set_playing(true);
                        ui.set_can_pause(true);
                        ui.set_can_previous(true);
                        ui.set_can_next(true);
                        ui.set_can_seek(true);
                        ui.set_can_repeat(true);
                        ui.set_repeat(true);
                        ui.set_library_ready(true);
                        ui.set_current_uri("spotify:track:aaaaaaaaaaaaaaaaaaaaaa".into());
                        ui.set_current_saved(true);
                        ui.set_track_title("Native playback fixture".into());
                        ui.set_artist("Visual test · no Spotify session".into());
                        ui.set_status("Visual fixture".into());
                        ui.set_position(45000.0);
                        ui.set_duration(210000.0);
                        ui.set_elapsed("0:45".into());
                        ui.set_total("3:30".into());
                        ui.set_volume(65.0);
                    }
                    16 => ui.window().set_size(slint::LogicalSize::new(700.0, 540.0)),
                    17 => {
                        ui.set_page(5);
                        ui.set_library_ready(false);
                        ui.set_authenticated(false);
                        ui.set_feature_tracks(ModelRc::default());
                        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
                    }
                    18 => ui.window().set_size(slint::LogicalSize::new(700.0, 540.0)),
                    _ => {
                        slint::quit_event_loop().unwrap();
                    }
                }
                step += 1;
            },
        );
    }
    let review = review_path
        .map(|path| ui_review::start(&ui, path.into()))
        .transpose()?;
    let feature_review = feature_review_path
        .map(|path| features::review::start(&ui, features.clone(), path.into()))
        .transpose()?;
    let local_review = local_review_path
        .map(|p| local::review(&ui, local.clone(), state.clone(), p.into()))
        .transpose()?;
    if !offline {
        let _ = tx.send(Request::Restore);
    }
    ui.run()?;
    drop(live_check);
    drop(review);
    drop(feature_review);
    drop(local_review);
    drop(media);
    local_timer.stop();
    drop(local_timer);
    // Release all UI/timer sender clones before joining the worker.
    poll.stop();
    jam_timer.stop();
    let _ = jam_tx.send(jam_transport::Request::Shutdown);
    let _ = tx.send(Request::Shutdown);
    drop(poll);
    drop(ui);
    drop(tx);
    drop(smoke_timer);
    drop(jam_timer);
    drop(jam_tx);
    let _ = jam_handle.join();
    let _ = handle.join();
    Ok(())
}
