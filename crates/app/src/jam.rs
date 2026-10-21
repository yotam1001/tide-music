//! Native controls for an opt-in developer build. Normal releases do not link
//! this module, the network worker or the Jam protocol dependency.
use super::*;
use crate::jam_session::{Admin, Event, Session, Start};
use tide_jam_lab::{
    endpoint::Endpoint,
    protocol::*,
    sync::{Action, Follower, Local},
};

#[derive(Default)]
pub struct Jam {
    session: Option<Session>,
    retiring: Option<Session>,
    guest: bool,
    terminal: bool,
    connected: bool,
    original: Option<(Queue, bool)>,
    follower: Option<Follower>,
    view: Option<(View, u64, Instant)>,
    began: Option<Instant>,
    accepted_request: u64,
}
pub fn following(s: &State) -> bool {
    s.jam.guest
}
pub fn request_source(s: &State, u: &AppWindow, source: &str) {
    match video(source) {
        Ok(video) => request_enqueue(
            s,
            u,
            &Track {
                artists: Vec::new(),
                title: String::new(),
                artist: String::new(),
                album: String::new(),
                source: video.playback_url(),
                duration_seconds: None,
                artwork_url: None,
                recommended: false,
            },
        ),
        Err(message) => u.set_jam_queue_notice(message.into()),
    }
}
pub fn request_enqueue(s: &State, u: &AppWindow, track: &Track) {
    let result = video(&track.source).and_then(|video| {
        s.jam
            .session
            .as_ref()
            .filter(|_| s.jam.connected && !s.jam.terminal)
            .filter(|session| session.enqueue(video.clone()))
            .map(|_| ())
            .ok_or("Wait for the Jam connection or the previous queue requests")
    });
    u.set_jam_queue_notice(
        match result {
            Ok(()) => "Sending song to the shared queue…",
            Err(message) => message,
        }
        .into(),
    );
}
fn accept_requests(s: &mut State, u: &AppWindow, requests: Vec<QueueRequest>) {
    let mut changed = false;
    for request in requests {
        if request.id > s.jam.accepted_request {
            // Count the shared window, not old local playback history.
            let current = s.settings.queue.current.unwrap_or(0);
            if s.settings.queue.tracks.len().saturating_sub(current) >= MAX_QUEUE {
                // Leave the request pending until there is room; never drop it silently.
                break;
            }
            let source = request.video.playback_url();
            let track = s
                .search_tracks
                .iter()
                .chain(s.detail_tracks.iter())
                .chain(s.settings.queue.tracks.iter())
                .find(|t| t.source == source)
                .cloned()
                .unwrap_or(Track {
                    artists: Vec::new(),
                    title: format!("YouTube track · {}", request.video.as_str()),
                    artist: "Jam request".into(),
                    album: String::new(),
                    source,
                    duration_seconds: None,
                    artwork_url: None,
                    recommended: false,
                });
            // A guest addition belongs after manually queued songs and before autoplay.
            if !s.settings.queue.insert_manual(track, false) {
                break;
            }
            s.jam.accepted_request = request.id;
            changed = true;
        }
        if let Some(session) = &s.jam.session {
            session.admin(Admin::Acknowledge(request.id));
        }
    }
    if changed {
        refresh(u, s);
    }
}
fn ready(s: &State) -> Result<(), &'static str> {
    if s.jam.session.is_some() || s.jam.guest {
        return Err("Leave the current Jam first");
    }
    if s.jam.retiring.is_some() {
        return Err("Previous Jam is closing; try again shortly");
    }
    if s.library_worker.is_some()
        || s.search_worker.is_some()
        || s.pending_saved_connection.is_some()
        || s.connection_resume.is_some()
    {
        return Err("Wait for the current music/account request to finish");
    }
    if s.engine.is_none() {
        return Err("Playback runtime is unavailable");
    }
    if s.provider.as_ref().is_some_and(|p| p.is_connected()) && !s.account_verified {
        return Err("Verify your local YouTube Music account before joining a Jam");
    }
    Ok(())
}
fn video(source: &str) -> Result<VideoId, &'static str> {
    let url = url::Url::parse(source).map_err(|_| "Jams share YouTube tracks only")?;
    if !is_youtube_source(source) {
        return Err("Jams share YouTube tracks only");
    }
    let id = if url.host_str() == Some("youtu.be") {
        url.path().trim_start_matches('/').to_owned()
    } else {
        url.query_pairs()
            .find(|(key, _)| key == "v")
            .ok_or("Jams require a YouTube song link")?
            .1
            .into_owned()
    };
    VideoId::parse(&id)
}
fn millis(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        (value * 1000.0).min(MAX_POSITION_MS as f64) as u64
    } else {
        0
    }
}
fn host_timeline(s: &State) -> Result<Timeline, &'static str> {
    let current = s
        .settings
        .queue
        .current
        .ok_or("Play a YouTube song before hosting")?;
    let queue = s
        .settings
        .queue
        .tracks
        .iter()
        .skip(current)
        .take(MAX_QUEUE)
        .map(|track| video(&track.source))
        .collect::<Result<Vec<_>, _>>()?;
    let p = s
        .engine
        .as_ref()
        .ok_or("Playback runtime unavailable")?
        .snapshot();
    let timeline = Timeline {
        revision: 1,
        play_id: s.generation,
        queue,
        index: 0,
        position_ms: if p.generation == s.generation {
            millis(p.position)
        } else {
            0
        },
        paused: p.paused || !s.active_playback || p.error.is_some(),
        loading: p.loading
            || p.generation != s.generation
            || !s.active_playback
            || p.error.is_some(),
    };
    timeline.validate()?;
    Ok(timeline)
}
// Safety transitions must survive a full/closed command queue. If the ordinary
// command cannot be accepted, dropping the engine signals its separate shutdown
// channel and joins owned helpers before reporting stopped playback.
fn stop_confirmed(s: &mut State) {
    if let Some(engine) = &s.engine {
        let _ = engine.prepare_next(None);
        if engine.send(PlaybackCommand::Stop).is_err() {
            s.pending_cookie_updates
                .extend(engine.take_cookie_updates());
            drop(s.engine.take());
        }
    }
    s.active_playback = false;
}
fn restore_engine(s: &mut State) -> Result<(), &'static str> {
    if s.engine.is_none() {
        let engine = tide_playback::start_with_account(
            s.cookie_lease.as_ref().map(|lease| lease.path().to_owned()),
            s.saved_connection
                .as_ref()
                .map_or(0, |connection| connection.account_index),
        )
        .map_err(|_| "Playback stopped safely; restart Tide to restore its runtime")?;
        engine
            .send(PlaybackCommand::SetVolume(
                s.settings.volume.unwrap_or(70.0),
            ))
            .map_err(|_| "Playback stopped safely; restart Tide to restore its runtime")?;
        s.engine = Some(engine);
    }
    Ok(())
}
fn pause_guest(s: &mut State, u: &AppWindow) {
    if s.engine
        .as_ref()
        .is_some_and(|engine| engine.send(PlaybackCommand::SetPaused(true)).is_err())
    {
        stop_confirmed(s);
        s.jam.terminal = true;
        u.set_jam_status("Playback stopped safely · leave Jam to restore the local player".into());
    }
}
fn begin_guest(s: &mut State, u: &AppWindow) -> Result<(), &'static str> {
    stop_confirmed(s);
    restore_engine(s)?;
    if s.volume_save_pending {
        save(&s.settings);
        s.volume_save_pending = false;
    }
    s.jam.original = Some((s.settings.queue.clone(), s.settings.transient));
    s.settings.transient = true;
    s.settings.queue = Queue::default();
    s.jam.guest = true;
    s.jam.follower = Some(Follower::starting_after(s.generation));
    s.connection_resume = None;
    s.radio_resume_when_ready = false;
    s.radio_requests.reset();
    s.library_retry = None;
    s.shuffle_request = false;
    s.active_playback = false;
    u.set_settings_open(false);
    u.set_account_open(false);
    u.set_queue_open(false);
    u.set_jam_guest(true);
    u.set_position(0.0);
    u.set_duration(0.0);
    u.set_paused(true);
    refresh(u, s);
    Ok(())
}
pub fn leave(s: &mut State, u: &AppWindow, message: &str) {
    if let Some(session) = s.jam.session.take() {
        session.cancel();
        s.jam.retiring = Some(session);
    }
    if s.jam.guest {
        stop_confirmed(s);
        if let Some((original, transient)) = s.jam.original.take() {
            s.settings.queue = original;
            s.settings.transient = transient;
        }
        s.active_playback = false;
        s.connection_resume = None;
        s.radio_resume_when_ready = false;
        s.jam.guest = false;
        u.set_position(0.0);
        u.set_duration(0.0);
        u.set_paused(true);
        refresh(u, s);
        save(&s.settings);
        if let Err(error) = restore_engine(s) {
            u.set_status_text(error.into());
        }
    }
    s.jam.follower = None;
    s.jam.view = None;
    s.jam.began = None;
    s.jam.terminal = false;
    s.jam.connected = false;
    s.jam.accepted_request = 0;
    u.set_jam_queue_notice("".into());
    u.set_jam_invite_code("".into());
    u.set_jam_members(slint::ModelRc::default());
    u.set_jam_locked(false);
    u.set_jam_listener_id(0);
    u.set_jam_active(false);
    u.set_jam_guest(false);
    u.set_jam_status(message.into());
    if u.get_collapsed() {
        u.window().set_size(slint::LogicalSize::new(700.0, 132.0));
    }
}
pub fn account_changing(s: &mut State, u: &AppWindow) {
    if s.jam.session.is_some() || s.jam.guest {
        leave(s, u, "Jam ended because the local account changed");
    }
}
pub fn install(state: &Rc<RefCell<State>>, ui: &AppWindow, picker: &Rc<PickerState>) {
    ui.set_jam_available(true);
    ui.set_jam_status("Choose a shared or self-hosted Jam server".into());
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_jam_leave(move || {
            if let Some(u) = weak.upgrade() {
                leave(
                    &mut state.borrow_mut(),
                    &u,
                    "Left Jam · your queue is restored and paused",
                );
            }
        });
    }
    install_code_controls(state, ui, picker);
    for host in [true, false] {
        let state = state.clone();
        let weak = ui.as_weak();
        let picker = picker.clone();
        let callback = move || {
            let Some(u) = weak.upgrade() else {
                return;
            };
            if let Err(error) = ready(&state.borrow()) {
                u.set_jam_status(error.into());
                return;
            }
            let endpoint = u.get_jam_endpoint().to_string();
            if host {
                if Endpoint::parse(&endpoint).is_err() {
                    u.set_jam_status(
                        "Enter a canonical HTTPS relay or http://127.0.0.1:port".into(),
                    );
                    return;
                }
                if let Err(error) = host_timeline(&state.borrow()) {
                    u.set_jam_status(error.into());
                    return;
                }
            }
            let state = state.clone();
            let weak = weak.clone();
            let selected = picker.clone();
            let result = spawn_picker(&picker, async move {
                let request = if host {
                    let remote =
                        Endpoint::parse(&endpoint).is_ok_and(|endpoint| endpoint.is_remote());
                    let operator = if remote {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .set_title("Choose this relay's private operator key")
                            .add_filter("Jam operator key", &["jam-operator-key"])
                            .pick_file()
                            .await
                        else {
                            return;
                        };
                        Some(file.path().to_owned())
                    } else {
                        None
                    };
                    let Some(file) = rfd::AsyncFileDialog::new()
                        .set_title(
                            "Save a new private guest invite (never overwrite an old invite)",
                        )
                        .set_file_name("friends.jam-invite.json")
                        .save_file()
                        .await
                    else {
                        return;
                    };
                    let Some(state_ui) = weak.upgrade() else {
                        return;
                    };
                    let s = state.borrow();
                    if let Err(error) = ready(&s) {
                        state_ui.set_jam_status(error.into());
                        return;
                    }
                    let timeline = match host_timeline(&s) {
                        Ok(timeline) => timeline,
                        Err(error) => {
                            state_ui.set_jam_status(error.into());
                            return;
                        }
                    };
                    Start::Host {
                        endpoint,
                        operator,
                        invite: file.path().to_owned(),
                        timeline,
                    }
                } else {
                    let Some(file) = rfd::AsyncFileDialog::new()
                        .set_title("Open a private invite from someone you trust")
                        .add_filter("Jam invite", &["json"])
                        .pick_file()
                        .await
                    else {
                        return;
                    };
                    Start::Guest {
                        invite: file.path().to_owned(),
                    }
                };
                let Some(u) = weak.upgrade() else {
                    return;
                };
                let mut s = state.borrow_mut();
                if let Err(error) = ready(&s) {
                    u.set_jam_status(error.into());
                    return;
                }
                match Session::start(request) {
                    Ok(session) => {
                        selected.cancel_account();
                        if !host {
                            if let Err(error) = begin_guest(&mut s, &u) {
                                session.cancel();
                                s.jam.retiring = Some(session);
                                u.set_jam_status(error.into());
                                return;
                            }
                        }
                        s.jam.session = Some(session);
                        s.jam.began = Some(Instant::now());
                        s.jam.terminal = false;
                        s.jam.connected = false;
                        u.set_jam_active(true);
                        u.set_jam_status(
                            if host {
                                "Creating room and saving private invite…"
                            } else {
                                "Joining · original queue preserved"
                            }
                            .into(),
                        );
                    }
                    Err(error) => u.set_jam_status(error.into()),
                }
            });
            if let Err(error) = result {
                u.set_jam_status(error.into());
            }
        };
        if host {
            ui.on_jam_host(callback);
        } else {
            ui.on_jam_join(callback);
        }
    }
}
fn install_code_controls(state: &Rc<RefCell<State>>, ui: &AppWindow, picker: &Rc<PickerState>) {
    for host in [true, false] {
        let state = state.clone();
        let weak = ui.as_weak();
        let picker = picker.clone();
        let callback = move || {
            let Some(u) = weak.upgrade() else {
                return;
            };
            let mut s = state.borrow_mut();
            if let Err(error) = ready(&s) {
                u.set_jam_status(error.into());
                return;
            }
            let request = if host {
                let endpoint = u.get_jam_endpoint().to_string();
                let endpoint = match Endpoint::parse(&endpoint) {
                    Ok(endpoint) => endpoint,
                    Err(_) => {
                        u.set_jam_status("Enter a trusted HTTPS Jam server".into());
                        return;
                    }
                };
                let timeline = match host_timeline(&s) {
                    Ok(timeline) => timeline,
                    Err(error) => {
                        u.set_jam_status(error.into());
                        return;
                    }
                };
                Start::HostCode {
                    endpoint: endpoint.as_str().into(),
                    timeline,
                }
            } else {
                let text = zeroize::Zeroizing::new(u.get_jam_code_input().to_string());
                if let Err(error) = Invite::from_code(&text) {
                    u.set_jam_status(error.into());
                    return;
                }
                Start::GuestCode { code: text }
            };
            match Session::start(request) {
                Ok(session) => {
                    picker.cancel_account();
                    if !host {
                        if let Err(error) = begin_guest(&mut s, &u) {
                            session.cancel();
                            s.jam.retiring = Some(session);
                            u.set_jam_status(error.into());
                            return;
                        }
                        u.set_jam_code_input("".into());
                    }
                    s.jam.session = Some(session);
                    s.jam.began = Some(Instant::now());
                    s.jam.terminal = false;
                    s.jam.connected = false;
                    u.set_jam_invite_code("".into());
                    u.set_jam_active(true);
                    u.set_jam_status(
                        if host {
                            "Starting Jam…"
                        } else {
                            "Joining Jam…"
                        }
                        .into(),
                    );
                }
                Err(error) => u.set_jam_status(error.into()),
            }
        };
        if host {
            ui.on_jam_host_code(callback);
        } else {
            ui.on_jam_join_code(callback);
        }
    }
    for kind in 0..3 {
        let state = state.clone();
        let weak = ui.as_weak();
        let callback = move |id: i32| {
            let Some(u) = weak.upgrade() else {
                return;
            };
            let s = state.borrow();
            if s.jam.guest || !s.jam.connected {
                return;
            }
            let Some(session) = s.jam.session.as_ref() else {
                return;
            };
            let command = match kind {
                0 => Admin::Admission(u.get_jam_locked()),
                1 => Admin::RotateInvite,
                _ if id > 0 => Admin::Remove(id as u32),
                _ => return,
            };
            if session.admin(command) {
                if kind != 0 {
                    u.set_jam_invite_code("".into());
                }
                u.set_jam_status("Updating room…".into());
            } else {
                u.set_jam_status("Wait for the previous room change, then try again".into());
            }
        };
        match kind {
            0 => ui.on_jam_toggle_lock(move || callback(0)),
            1 => ui.on_jam_rotate_invite(move || callback(0)),
            _ => ui.on_jam_remove_member(callback),
        }
    }
    let weak = ui.as_weak();
    ui.on_jam_copy_invite(move || {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let code = zeroize::Zeroizing::new(u.get_jam_invite_code().to_string());
        if code.is_empty() {
            return;
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(code.as_str()))
        {
            Ok(()) => u.set_jam_status("Invite copied — send it to your friends".into()),
            Err(_) => u.set_jam_status(
                "Could not copy. Select the invite code and copy it manually.".into(),
            ),
        }
    });
}

struct GuestUpdate {
    actions: Vec<Action>,
    adopt_queue: bool,
    status: &'static str,
}
fn guest_update(
    follower: &mut Follower,
    view: &View,
    rtt_ms: u64,
    local: &Local,
    now: u64,
    failed: bool,
) -> Result<GuestUpdate, &'static str> {
    let adopt_queue = follower.accepts_timeline(view, rtt_ms);
    let mut actions = follower.decide(view, rtt_ms, local, now)?;
    let loading_new_track = actions
        .iter()
        .any(|action| matches!(action, Action::Load { .. }));
    // A regional/unavailable song is a per-track failure. Keep the membership
    // and host's authority, without repeatedly extracting the failed song.
    // A newer host play may Load even while the old engine error is visible.
    let waiting_for_track = failed && !loading_new_track;
    if waiting_for_track && !local.paused && !actions.contains(&Action::Pause(true)) {
        actions.push(Action::Pause(true));
    }
    let status = match view.timeline.as_ref().filter(|_| adopt_queue) {
        Some(_) if waiting_for_track => {
            "This song is unavailable locally. Waiting for the host's next song…"
        }
        Some(timeline) if timeline.loading => "Host is buffering · waiting together",
        Some(_) if !local.ready || loading_new_track => "Loading host's song locally…",
        Some(_) => "Following host · volume stays local",
        None => "Waiting for a fresh host state · playback stays paused if unavailable",
    };
    Ok(GuestUpdate {
        actions,
        adopt_queue,
        status,
    })
}
pub fn tick(s: &mut State, u: &AppWindow) {
    if let Some(notice) = s.jam.session.as_ref().and_then(Session::take_notice) {
        u.set_jam_queue_notice(notice.into());
    }
    if s.jam.retiring.as_ref().is_some_and(Session::finished) {
        s.jam.retiring.take();
    }
    let event = s.jam.session.as_ref().and_then(Session::take_event);
    match event {
        Some(Event::Hosting {
            guests,
            invite_code,
            members,
            open,
            requests,
        }) => {
            accept_requests(s, u, requests);
            s.jam.connected = true;
            u.set_jam_invite_code(invite_code.as_str().into());
            u.set_jam_locked(!open);
            let rows: Vec<_> = members
                .into_iter()
                .map(|id| JamMemberRow {
                    id: id as i32,
                    name: format!("Listener {id}").into(),
                })
                .collect();
            u.set_jam_members(slint::ModelRc::new(slint::VecModel::from(rows)));
            u.set_jam_status(
                format!(
                    "Hosting · {guests} guest(s){}",
                    if open { "" } else { " · room locked" }
                )
                .into(),
            )
        }
        Some(Event::View {
            view,
            rtt_ms,
            received,
            listener,
        }) => {
            s.jam.connected = true;
            u.set_jam_listener_id(listener.unwrap_or(0) as i32);
            s.jam.view = Some((view, rtt_ms, received));
        }
        Some(Event::Problem { message, terminal }) => {
            if terminal && !s.jam.connected {
                leave(s, u, message);
                return;
            }
            s.jam.terminal |= terminal;
            u.set_jam_status(message.into());
            if terminal && s.jam.guest {
                pause_guest(s, u);
            }
        }
        None => {}
    }
    if s.jam.session.is_none() || s.jam.terminal {
        return;
    }
    if !s.jam.guest {
        match host_timeline(s) {
            Ok(timeline) => s.jam.session.as_ref().unwrap().publish(timeline),
            Err(_) => leave(
                s,
                u,
                "Jam ended · the host queue must contain YouTube tracks",
            ),
        }
        return;
    }
    let Some(p) = s.engine.as_ref().map(|engine| engine.snapshot()) else {
        return;
    };
    let local = Local {
        generation: p.generation,
        position_ms: millis(p.position),
        duration_ms: millis(p.duration),
        paused: p.paused,
        ready: !p.loading
            && !p.ended
            && p.error.is_none()
            && p.duration.is_finite()
            && p.duration > 0.0,
    };
    let Some((mut view, rtt, received)) = s.jam.view.clone() else {
        return;
    };
    if received.elapsed() >= Duration::from_secs(3) {
        if !p.paused {
            pause_guest(s, u);
        }
        u.set_jam_status("Jam connection interrupted · paused while reconnecting".into());
        return;
    }
    view.age_ms = view
        .age_ms
        .saturating_add(received.elapsed().as_millis() as u64);
    let now = s
        .jam
        .began
        .map_or(0, |began| began.elapsed().as_millis() as u64);
    let update = match guest_update(
        s.jam.follower.as_mut().unwrap(),
        &view,
        rtt,
        &local,
        now,
        p.error.is_some() && p.generation == s.generation,
    ) {
        Ok(update) => update,
        Err(_) => {
            s.jam.terminal = true;
            pause_guest(s, u);
            u.set_jam_status("Invalid Jam state · playback paused".into());
            return;
        }
    };
    if let Some(timeline) = view.timeline.as_ref().filter(|_| update.adopt_queue) {
        let changed = timeline.queue.len() != s.settings.queue.tracks.len()
            || s.settings.queue.current != Some(timeline.index)
            || timeline
                .queue
                .iter()
                .zip(&s.settings.queue.tracks)
                .any(|(id, track)| id.playback_url() != track.source);
        if changed {
            let tracks = timeline
                .queue
                .iter()
                .map(|id| {
                    let source = id.playback_url();
                    s.settings
                        .queue
                        .tracks
                        .iter()
                        .chain(
                            s.jam
                                .original
                                .as_ref()
                                .into_iter()
                                .flat_map(|(queue, _)| queue.tracks.iter()),
                        )
                        .find(|track| track.source == source)
                        .cloned()
                        .unwrap_or(Track {
                            artists: Vec::new(),
                            title: format!("YouTube track · {}", id.as_str()),
                            artist: "Jam".into(),
                            album: String::new(),
                            source,
                            duration_seconds: None,
                            artwork_url: None,
                            recommended: false,
                        })
                })
                .collect();
            s.settings.queue = Queue {
                tracks,
                current: Some(timeline.index),
            };
            refresh(u, s);
        }
    }
    u.set_jam_status(update.status.into());
    for action in update.actions {
        let command = match action {
            Action::Load { video, generation } => {
                s.generation = generation;
                s.active_playback = true;
                PlaybackCommand::Load {
                    source: video.playback_url(),
                    generation,
                }
            }
            Action::Pause(value) => PlaybackCommand::SetPaused(value),
            Action::Seek(value) => PlaybackCommand::Seek(value as f64 / 1000.0),
        };
        if s.engine.as_ref().unwrap().send(command).is_err() {
            s.jam.terminal = true;
            pause_guest(s, u);
            u.set_jam_status("Jam playback command failed · leave to retry".into());
            break;
        }
    }
    if let Some(title) = p
        .title
        .filter(|title| !title.is_empty() && p.generation == s.generation)
    {
        if let Some(track) = s
            .settings
            .queue
            .current
            .and_then(|index| s.settings.queue.tracks.get_mut(index))
        {
            if track.title.starts_with("YouTube track · ") {
                track.title = title;
                refresh(u, s);
            }
        }
    }
}

/// Explicit offline verification, before Window::show: no account/provider,
/// files, network, SMTC or native Load is used. Exercises the registered native
/// UI/media callbacks, including paths whose disabled widgets can be bypassed.
pub fn ownership_smoke(state: &Rc<RefCell<State>>, u: &AppWindow, pickers: &Rc<PickerState>) {
    fn track(id: &str) -> Track {
        Track {
            artists: Vec::new(),
            title: "Synthetic ownership fixture".into(),
            artist: String::new(),
            album: String::new(),
            source: VideoId::parse(id).unwrap().playback_url(),
            artwork_url: None,
            duration_seconds: None,
            recommended: false,
        }
    }
    let original = Queue {
        tracks: vec![track("original001"), track("original002")],
        current: Some(1),
    };
    {
        let mut s = state.borrow_mut();
        s.settings.queue = original.clone();
        let generation = s.generation;
        let request = QueueRequest {
            id: 1,
            video: VideoId::parse("fixture0003").unwrap(),
        };
        accept_requests(&mut s, u, vec![request.clone()]);
        accept_requests(&mut s, u, vec![request]);
        assert_eq!(
            s.settings.queue.tracks.len(),
            3,
            "a repeated delivery duplicated the song"
        );
        assert_eq!(s.settings.queue.current, Some(1));
        assert_eq!(
            s.generation, generation,
            "a queue request changed playback generation"
        );
        s.jam.accepted_request = 0;
    }
    {
        let mut s = state.borrow_mut();
        assert!(s.provider.is_none() && s.settings.transient);
        s.settings.queue = original.clone();
        s.settings.autoplay = false;
        s.generation = 41;
        begin_guest(&mut s, u).unwrap();
        s.settings.queue = Queue {
            tracks: vec![track("fixture0001"), track("fixture0002")],
            current: Some(0),
        };
    }
    let before = serde_json::to_string(&state.borrow().settings).unwrap();
    u.invoke_jam_host_code();
    u.invoke_jam_join_code();
    u.invoke_jam_toggle_lock();
    u.invoke_jam_rotate_invite();
    u.invoke_jam_remove_member(1);
    u.invoke_toggle_pause();
    u.invoke_next_track();
    u.invoke_previous_track();
    u.invoke_shuffle_queue();
    u.invoke_toggle_repeat();
    u.invoke_toggle_autoplay();
    u.invoke_clear_queue();
    u.invoke_remove_queue_track(0);
    u.invoke_reorder_queue_track(1, 0);
    u.invoke_requeue_track(1, true);
    u.invoke_select_track(1);
    u.invoke_seek(90.0);
    u.invoke_play_radio();
    u.invoke_start_track_radio(0, 0);
    u.invoke_add_source("https://www.youtube.com/watch?v=fixture0003".into());
    u.invoke_open_file();
    u.invoke_connect_account();
    u.invoke_import_account();
    u.invoke_switch_account(1);
    u.invoke_disconnect_account();
    u.invoke_refresh_library();
    u.invoke_refresh_recommendations();
    u.invoke_retry_error();
    for action in 0..7 {
        u.invoke_media_command(action, 30.0);
    }
    assert_eq!(
        before,
        serde_json::to_string(&state.borrow().settings).unwrap(),
        "a local callback changed the guest's controlled state"
    );
    assert_eq!(
        state.borrow().generation,
        41,
        "a local callback loaded media"
    );
    assert!(
        !pickers.busy.get(),
        "a blocked local callback opened a picker"
    );
    assert!(state.borrow().library_worker.is_none() && state.borrow().search_worker.is_none());
    u.invoke_change_volume(23.0);
    assert_eq!(
        u.get_volume(),
        23.0,
        "guest volume should stay local and usable"
    );
    {
        let mut s = state.borrow_mut();
        leave(&mut s, u, "Fixture left");
        assert_eq!(
            serde_json::to_string(&s.settings.queue).unwrap(),
            serde_json::to_string(&original).unwrap()
        );
        assert!(s.settings.transient && !s.active_playback && local_controls_allowed(&s));
        assert!(u.get_paused() && u.get_position() == 0.0 && !u.get_jam_guest());
    }
    println!("PASS offline Jam ownership: native callbacks/media keys blocked, volume local, original queue restored paused; window never shown");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(play_id: u64) -> View {
        View {
            timeline: Some(Timeline {
                revision: play_id,
                play_id,
                queue: vec![VideoId::parse("fixture0001").unwrap()],
                index: 0,
                position_ms: 10_000,
                paused: false,
                loading: false,
            }),
            age_ms: 0,
            members: 1,
        }
    }

    #[test]
    fn unavailable_song_waits_without_retrying_and_next_host_play_recovers() {
        let mut follower = Follower::default();
        let first =
            guest_update(&mut follower, &view(1), 100, &Local::default(), 0, false).unwrap();
        assert!(matches!(
            first.actions[0],
            Action::Load { generation: 1, .. }
        ));
        let failed = Local {
            generation: 1,
            paused: false,
            ..Local::default()
        };
        let waiting = guest_update(&mut follower, &view(1), 100, &failed, 1000, true)
            .expect("an unavailable song must not end the Jam");
        assert_eq!(waiting.actions, vec![Action::Pause(true)]);
        assert!(waiting.status.contains("Waiting for the host"));
        let paused = Local {
            paused: true,
            ..failed
        };
        for now in [1100, 2000, 4000] {
            let waiting = guest_update(&mut follower, &view(1), 100, &paused, now, true).unwrap();
            assert!(
                waiting.actions.is_empty(),
                "failed songs must not reload in a loop"
            );
        }
        let next = guest_update(&mut follower, &view(2), 100, &paused, 5000, true).unwrap();
        assert!(matches!(
            next.actions[0],
            Action::Load { generation: 2, .. }
        ));
        assert_eq!(next.status, "Loading host's song locally…");
        let ready = Local {
            generation: 2,
            duration_ms: 60_000,
            ready: true,
            paused: true,
            ..Local::default()
        };
        let resumed = guest_update(&mut follower, &view(2), 100, &ready, 6000, false).unwrap();
        assert_eq!(
            resumed.actions,
            vec![Action::Seek(10_050), Action::Pause(false)]
        );
    }

    #[test]
    fn failed_song_cannot_recover_from_stale_invalid_or_disconnected_state() {
        let mut follower = Follower::default();
        guest_update(&mut follower, &view(1), 0, &Local::default(), 0, false).unwrap();
        let failed = Local {
            generation: 1,
            paused: true,
            ..Local::default()
        };
        let mut stale = view(2);
        stale.age_ms = STALE_MS;
        for (view, rtt) in [(stale, 0), (view(2), 2001)] {
            let update = guest_update(&mut follower, &view, rtt, &failed, 4000, true).unwrap();
            assert!(!update.adopt_queue);
            assert!(update.actions.is_empty());
        }
        let mut invalid = view(2);
        invalid.timeline.as_mut().unwrap().queue.clear();
        assert!(guest_update(&mut follower, &invalid, 0, &failed, 4000, true).is_err());
        follower.disconnect();
        let update = guest_update(&mut follower, &view(2), 0, &failed, 5000, true).unwrap();
        assert!(!update.adopt_queue);
        assert!(update.actions.is_empty());
    }
}
