//! Experimental nearby Tide/Grove rooms. Foreign recordings require confirmation.
use super::*;
use tide_jam::{jam, matching::Pairings, sync as follow, transport as wire};

/// Offline native review: real pairing callbacks, no account, search or audio backend.
pub fn review(out: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        std::process::exit(1);
    }));
    std::fs::create_dir_all(&out)?;
    let window = TideJamWindow::new()?;
    window.set_address("127.0.0.1".into());
    let session = Rc::new(RefCell::new(Session::default()));
    let s = session.clone();
    let weak = window.as_weak();
    window.on_pair(move |index| {
        if let Some(w) = weak.upgrade() {
            s.borrow_mut().pair(&w, index);
        }
    });
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    let passed = Rc::new(Cell::new(false));
    let done = passed.clone();
    let mut step = 0;
    timer.start(slint::TimerMode::Repeated,Duration::from_millis(250),move || {
        let Some(w)=weak.upgrade() else { return; };
        let capture=|name:&str| {
            let p=w.window().take_snapshot().unwrap();
            image::RgbaImage::from_raw(p.width(),p.height(),p.as_bytes().to_vec()).unwrap().save(out.join(format!("{name}.png"))).unwrap();
        };
        match step {
            0 => {},
            1 => {
                capture("tide-jam-setup");w.set_connected(true);w.set_host(false);w.set_following(true);w.set_members("Grove host · Tide guest".into());
                let mut s=session.borrow_mut();s.room="review-room".into();s.pairs.set_room("review-room");
                let remote=jam::Playback{uri:"spotify:track:aaaaaaaaaaaaaaaaaaaaaa".into(),title:"Forest floor".into(),artist:"Review artist".into(),position_ms:12000,playing:true,sampled_at_ms:jam::unix_ms()};
                assert!(s.resolve(&remote,&w).is_none());assert!(s.job.is_none(),"Review launched network search");
                s.choices=vec![Track{title:"Forest floor".into(),artist:"Review artist".into(),source:"https://www.youtube.com/watch?v=abcdefghijk".into(),album:String::new(),duration_seconds:Some(210),artwork_url:None,recommended:false}];
                w.set_choices(ModelRc::new(VecModel::from(vec![JamChoice{title:"Forest floor".into(),artist:"Review artist".into(),duration:"3:30".into()}])));
                w.set_status("Choose the matching song to follow Grove.".into());
            }
            2 => { capture("tide-jam-matching");w.invoke_pair(0);assert_eq!(session.borrow().pairs.get("spotify:track:aaaaaaaaaaaaaaaaaaaaaa"),Some("youtube:abcdefghijk"));w.window().set_size(slint::LogicalSize::new(460.0,540.0)); }
            3 => {
                capture("tide-jam-matching-narrow");w.set_match_link("https://evil.test/watch?v=abcdefghijk".into());w.invoke_pair(-1);
                assert_eq!(session.borrow().pairs.get("spotify:track:aaaaaaaaaaaaaaaaaaaaaa"),Some("youtube:abcdefghijk"));
                w.set_match_link("https://music.youtube.com/watch?v=bcdefghijkl".into());w.invoke_pair(-1);
                assert_eq!(session.borrow().pairs.get("spotify:track:aaaaaaaaaaaaaaaaaaaaaa"),Some("youtube:bcdefghijkl"));
                let mut s=session.borrow_mut();s.pairs.set_room("another-room");assert!(s.pairs.get("spotify:track:aaaaaaaaaaaaaaaaaaaaaa").is_none());
                done.set(true);eprintln!("PASS: Tide native cross-service pairing, manual links, invalid-source and room-isolation guards");slint::quit_event_loop().unwrap();
            }
            _ => unreachable!(),
        }
        step+=1;
    });
    window.run()?;
    timer.stop();
    if !passed.get() {
        return Err("Tide cross-jam review did not finish".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guest_queue_is_temporary_and_restores_selection_without_losing_preferences() {
        let mut session = Session::default();
        let track = Track {
            title: "Original song".into(),
            artist: "Artist".into(),
            source: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            album: String::new(),
            duration_seconds: Some(210),
            artwork_url: None,
            recommended: false,
        };
        let mut settings = Settings {
            queue: Queue {
                tracks: vec![track.clone(), track],
                current: Some(1),
            },
            ..Settings::default()
        };
        session.enter_guest(&mut settings);
        assert!(settings.transient);
        settings.queue = Queue::default();
        settings.light_mode = true;
        assert!(session.restore_queue(&mut settings));
        assert_eq!(settings.queue.tracks.len(), 2);
        assert_eq!(settings.queue.current, Some(1));
        assert!(!settings.transient);
        assert!(settings.light_mode);
        assert!(!session.restore_queue(&mut settings));
        settings.transient = true;
        session.enter_guest(&mut settings);
        session.restore_queue(&mut settings);
        assert!(
            settings.transient,
            "A transient review must never gain permission to save settings"
        );
    }
}

struct SearchJob {
    remote: String,
    room: String,
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<Vec<Track>, String>>,
    handle: std::thread::JoinHandle<()>,
}
#[derive(Default)]
struct Session {
    allow_search: bool,
    target: Option<follow::Target>,
    follower: follow::Follower,
    pairs: Pairings,
    room: String,
    revision: u64,
    remote: String,
    choices: Vec<Track>,
    job: Option<SearchJob>,
    retired: Vec<SearchJob>,
    original: Option<(Queue, bool)>,
    initial_seek: Option<(String, u64)>,
    quiet: bool,
    was_following: bool,
}
impl Session {
    fn enter_guest(&mut self, settings: &mut Settings) {
        self.original = Some((settings.queue.clone(), settings.transient));
        settings.transient = true;
    }
    fn restore_queue(&mut self, settings: &mut Settings) -> bool {
        if let Some((queue, transient)) = self.original.take() {
            settings.queue = queue;
            settings.transient = transient;
            true
        } else {
            false
        }
    }
    fn cancel_search(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Release);
            self.retired.push(job);
        }
        let mut pending = Vec::new();
        for job in self.retired.drain(..) {
            if job.handle.is_finished() {
                let _ = job.handle.join();
            } else {
                pending.push(job);
            }
        }
        self.retired = pending;
    }
    fn reset(&mut self, main: &AppWindow, state: &mut State, window: &TideJamWindow) {
        self.cancel_search();
        if self.original.is_some() {
            main.set_jam_applying(true);
            send(state, main, PlaybackCommand::Stop);
            main.set_jam_applying(false);
            state.active_playback = false;
            self.restore_queue(&mut state.settings);
            refresh(main, state);
            main.set_paused(true);
            save(&state.settings);
        }
        main.set_following_jam(false);
        self.target = None;
        self.follower = Default::default();
        self.pairs = Default::default();
        self.room.clear();
        self.revision = 0;
        self.remote.clear();
        self.choices.clear();
        self.initial_seek = None;
        self.quiet = false;
        self.was_following = true;
        window.set_connected(false);
        window.set_busy(false);
        window.set_matching(false);
        window.set_choices(ModelRc::default());
        window.set_invitation("".into());
        window.set_members("".into());
        window.set_match_link("".into());
    }
    fn pair(&mut self, window: &TideJamWindow, index: i32) {
        if !window.get_connected()
            || window.get_host()
            || !window.get_following()
            || self.remote.is_empty()
        {
            return;
        }
        let local = if index >= 0 {
            self.choices
                .get(index as usize)
                .and_then(|t| tide_jam::track_uri(&t.source).ok())
        } else {
            tide_jam::track_uri(&window.get_match_link()).ok()
        };
        let Some(local) = local.filter(|s| s.starts_with("youtube:")) else {
            window.set_message("Choose a result or paste the matching YouTube link.".into());
            return;
        };
        if self.pairs.remember(&self.remote, &local).is_ok() {
            self.follower = Default::default();
            window.set_message(
                "Song matched for this jam. Choose another if the recording differs.".into(),
            );
        }
    }
    fn resolve(&mut self, remote: &jam::Playback, window: &TideJamWindow) -> Option<String> {
        if remote.uri.starts_with("youtube:") {
            window.set_matching(false);
            return Some(remote.uri.clone());
        }
        if !remote.uri.starts_with("spotify:track:") {
            return None;
        }
        window.set_matching(true);
        if self.remote != remote.uri {
            self.cancel_search();
            self.remote = remote.uri.clone();
            self.choices.clear();
            window.set_choices(ModelRc::default());
            window.set_match_link("".into());
            window.set_song(format!("{} · {}", remote.title, remote.artist).into());
            if self.allow_search && self.pairs.get(&remote.uri).is_none() && self.retired.len() < 2
            {
                let query: String = format!("{} {} audio", remote.title, remote.artist)
                    .chars()
                    .take(200)
                    .collect();
                let cancel = Arc::new(AtomicBool::new(false));
                let worker_cancel = cancel.clone();
                let (tx, rx) = mpsc::sync_channel(1);
                let handle = std::thread::spawn(move || {
                    let _ = tx.send(search(query, worker_cancel));
                });
                self.job = Some(SearchJob {
                    remote: remote.uri.clone(),
                    room: self.room.clone(),
                    cancel,
                    result: rx,
                    handle,
                });
                window.set_message("Finding this song on YouTube…".into());
            }
        }
        self.pairs.get(&remote.uri).map(str::to_owned)
    }
}

pub struct Controller {
    window: TideJamWindow,
    timer: slint::Timer,
    sender: tokio::sync::mpsc::UnboundedSender<wire::Request>,
    worker: Option<std::thread::JoinHandle<()>>,
    session: Rc<RefCell<Session>>,
    state: Rc<RefCell<State>>,
    main: slint::Weak<AppWindow>,
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.timer.stop();
        if let Some(main) = self.main.upgrade() {
            self.session
                .borrow_mut()
                .reset(&main, &mut self.state.borrow_mut(), &self.window);
        }
        let mut session = self.session.borrow_mut();
        session.cancel_search();
        for job in session.retired.drain(..) {
            let _ = job.handle.join();
        }
        let _ = self.sender.send(wire::Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = self.window.hide();
    }
}

pub fn install(
    main: &AppWindow,
    state: Rc<RefCell<State>>,
) -> Result<Controller, slint::PlatformError> {
    let window = TideJamWindow::new()?;
    window.set_address(wire::suggested_address().into());
    let session = Rc::new(RefCell::new(Session {
        allow_search: true,
        ..Default::default()
    }));
    let (sender, events, worker) = wire::worker();
    let weak = window.as_weak();
    main.on_open_jam(move || {
        if let Some(w) = weak.upgrade() {
            let _ = w.show();
        }
    });
    let tx = sender.clone();
    let weak = window.as_weak();
    window.on_host_room(move || {
        if let Some(w) = weak.upgrade() {
            if w.get_busy() || w.get_connected() {
                return;
            }
            w.set_busy(true);
            w.set_message("Starting a nearby jam…".into());
            let _ = tx.send(wire::Request::Host {
                name: w.get_name().into(),
                address: w.get_address().into(),
            });
        }
    });
    let tx = sender.clone();
    let weak = window.as_weak();
    window.on_join(move || {
        if let Some(w) = weak.upgrade() {
            if w.get_busy() || w.get_connected() {
                return;
            }
            w.set_busy(true);
            w.set_message("Connecting to the host…".into());
            let _ = tx.send(wire::Request::Join {
                name: w.get_name().into(),
                invite: w.get_invite_input().into(),
            });
        }
    });
    let tx = sender.clone();
    window.on_leave(move || {
        let _ = tx.send(wire::Request::Leave);
    });
    let s = session.clone();
    let weak = window.as_weak();
    window.on_pair(move |index| {
        if let Some(w) = weak.upgrade() {
            s.borrow_mut().pair(&w, index);
        }
    });
    // Closing the companion window ends participation, so a hidden guest never keeps controlling playback.
    let tx = sender.clone();
    window.window().on_close_requested(move || {
        let _ = tx.send(wire::Request::Leave);
        slint::CloseRequestResponse::HideWindow
    });
    let weak = window.as_weak();
    let weak_main = main.as_weak();
    let s = session.clone();
    let app = state.clone();
    let tx = sender.clone();
    let mut published = Instant::now() - Duration::from_secs(2);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let (Some(w), Some(main)) = (weak.upgrade(), weak_main.upgrade()) else {
                return;
            };
            let mut s = s.borrow_mut();
            let mut app = app.borrow_mut();
            while let Ok(event) = events.try_recv() {
                match event {
                    wire::Event::Joined { invite, host, .. } => {
                        s.reset(&main, &mut app, &w);
                        if !host {
                            s.enter_guest(&mut app.settings);
                        }
                        w.set_connected(true);
                        w.set_host(host);
                        w.set_following(true);
                        w.set_invitation(invite.into());
                        w.set_message("".into());
                    }
                    wire::Event::Snapshot {
                        state,
                        host_sent_ms,
                        transit_ms,
                    } => {
                        if !s.room.is_empty() && s.room != state.room_id {
                            continue;
                        }
                        if state.revision <= s.revision {
                            continue;
                        }
                        s.room = state.room_id.clone();
                        s.revision = state.revision;
                        s.pairs.set_room(&state.room_id);
                        w.set_members(
                            state
                                .members
                                .iter()
                                .map(|m| {
                                    format!("{}{}", m.name, if m.host { " (host)" } else { "" })
                                })
                                .collect::<Vec<_>>()
                                .join(" · ")
                                .into(),
                        );
                        s.target = state.playback.map(|playback| follow::Target {
                            playback,
                            host_sent_ms,
                            received: Instant::now(),
                            transit_ms,
                        });
                    }
                    wire::Event::Left(message) => {
                        s.reset(&main, &mut app, &w);
                        w.set_message(message.into());
                    }
                    wire::Event::Error(message) => {
                        w.set_busy(false);
                        w.set_message(message.into());
                    }
                    wire::Event::Play { id, .. } => {
                        let _ = tx.send(wire::Request::Complete { id, success: false });
                        w.set_message(
                            "Choose music directly in Tide for cross-service rooms.".into(),
                        );
                    }
                }
            }
            if !w.get_connected() {
                main.set_following_jam(false);
                return;
            }
            if let Some(job) = &s.job {
                if let Ok(result) = job.result.try_recv() {
                    let current = job.remote == s.remote && job.room == s.room;
                    let job = s.job.take().unwrap();
                    let _ = job.handle.join();
                    if current {
                        match result {
                            Ok(tracks) => {
                                s.choices = tracks;
                                w.set_choices(ModelRc::new(VecModel::from(
                                    s.choices
                                        .iter()
                                        .map(|t| JamChoice {
                                            title: t.title.clone().into(),
                                            artist: t.artist.clone().into(),
                                            duration: metadata::format_duration(t.duration_seconds)
                                                .into(),
                                        })
                                        .collect::<Vec<_>>(),
                                )));
                                w.set_message(
                                    "Choose the same recording, or paste its YouTube link.".into(),
                                );
                            }
                            Err(_) => w.set_message(
                                "Couldn't find matches. Paste the YouTube link for this song."
                                    .into(),
                            ),
                        }
                    }
                }
            }
            if w.get_host() {
                main.set_following_jam(false);
                if published.elapsed() < Duration::from_secs(1) {
                    return;
                }
                published = Instant::now();
                let snapshot = app.engine.as_ref().map(|e| e.snapshot());
                let frame = app
                    .settings
                    .queue
                    .current
                    .and_then(|i| app.settings.queue.tracks.get(i))
                    .and_then(|t| {
                        let p = snapshot.as_ref()?;
                        let uri = tide_jam::track_uri(&t.source)
                            .ok()
                            .filter(|u| u.starts_with("youtube:"))?;
                        if !app.active_playback
                            || p.generation != app.generation
                            || p.loading
                            || p.ended
                            || p.error.is_some()
                        {
                            return None;
                        }
                        Some(jam::Playback {
                            uri,
                            title: t.title.chars().take(300).collect(),
                            artist: t.artist.chars().take(300).collect(),
                            position_ms: (p.position.max(0.0) * 1000.0) as u64,
                            playing: !p.paused,
                            sampled_at_ms: jam::unix_ms(),
                        })
                    });
                w.set_status(
                    if frame.is_some() {
                        "Sharing your music. Friends follow on their own devices."
                    } else {
                        "Play a YouTube song in Tide to start listening together."
                    }
                    .into(),
                );
                let _ = tx.send(wire::Request::Publish(frame));
                return;
            }
            main.set_following_jam(w.get_following());
            if s.was_following != w.get_following() {
                s.follower = Default::default();
                s.was_following = w.get_following();
                s.quiet = false;
            }
            if !w.get_following() {
                w.set_status("Following stopped. Your controls are independent.".into());
                return;
            }
            let target = s.target.as_ref().map(|t| follow::Target {
                playback: t.playback.clone(),
                host_sent_ms: t.host_sent_ms,
                received: t.received,
                transit_ms: t.transit_ms,
            });
            let Some(mut target) = target else {
                w.set_matching(false);
                w.set_status("Waiting for the host to play a song.".into());
                if !s.quiet {
                    main.set_jam_applying(true);
                    send(&mut app, &main, PlaybackCommand::SetPaused(true));
                    main.set_jam_applying(false);
                    s.quiet = true;
                }
                return;
            };
            if target.stale(Instant::now()) {
                w.set_status("Host updates delayed. Waiting to sync.".into());
                return;
            }
            let Some(uri) = s.resolve(&target.playback, &w) else {
                w.set_status("Choose the matching YouTube song to follow Grove.".into());
                if !s.quiet {
                    main.set_jam_applying(true);
                    send(&mut app, &main, PlaybackCommand::SetPaused(true));
                    main.set_jam_applying(false);
                    s.quiet = true;
                }
                return;
            };
            target.playback.uri = uri.clone();
            s.quiet = false;
            let Some(snapshot) = app.engine.as_ref().map(|e| e.snapshot()) else {
                w.set_status("Tide's playback runtime is unavailable.".into());
                return;
            };
            let local = app
                .settings
                .queue
                .current
                .and_then(|i| app.settings.queue.tracks.get(i))
                .and_then(|t| tide_jam::track_uri(&t.source).ok())
                .unwrap_or_default();
            if let Some((expected, generation)) = &s.initial_seek {
                if *expected == uri
                    && *generation == snapshot.generation
                    && !snapshot.loading
                    && snapshot.duration > 0.0
                {
                    main.set_jam_applying(true);
                    send(
                        &mut app,
                        &main,
                        PlaybackCommand::Seek(target.position(Instant::now()) as f64 / 1000.0),
                    );
                    send(
                        &mut app,
                        &main,
                        PlaybackCommand::SetPaused(!target.playback.playing),
                    );
                    main.set_jam_applying(false);
                    s.initial_seek = None;
                } else if *expected != uri {
                    s.initial_seek = None;
                }
            }
            if s.follower.exhausted(&uri, Instant::now()) && (!app.active_playback || local != uri)
            {
                w.set_status(
                    "Couldn't load this recording. Stop and restart following to retry.".into(),
                );
                return;
            }
            w.set_status(
                format!(
                    "Following {} · {}",
                    target.playback.title,
                    if target.playback.playing {
                        "Playing"
                    } else {
                        "Paused"
                    }
                )
                .into(),
            );
            let correction = s.follower.plan(
                &target,
                follow::Local {
                    uri: if app.active_playback && !snapshot.ended {
                        &local
                    } else {
                        ""
                    },
                    position_ms: (snapshot.position.max(0.0) * 1000.0) as u64,
                    playing: !snapshot.paused,
                    ready: !snapshot.loading,
                },
                Instant::now(),
            );
            main.set_jam_applying(true);
            match correction {
                Some(follow::Correction::Load { .. }) => {
                    let source = format!(
                        "https://www.youtube.com/watch?v={}",
                        uri.strip_prefix("youtube:").unwrap_or_default()
                    );
                    let track = s
                        .choices
                        .iter()
                        .find(|t| t.source == source)
                        .cloned()
                        .unwrap_or(Track {
                            title: target.playback.title.clone(),
                            artist: target.playback.artist.clone(),
                            album: String::new(),
                            source,
                            duration_seconds: None,
                            artwork_url: None,
                            recommended: false,
                        });
                    // Guest playback uses a temporary queue; the original returns on leave.
                    app.settings.queue = Queue {
                        tracks: vec![track],
                        current: Some(0),
                    };
                    if play(&mut app, &main) {
                        s.initial_seek = Some((uri, app.generation));
                    }
                }
                Some(follow::Correction::Seek(ms)) => {
                    send(&mut app, &main, PlaybackCommand::Seek(ms as f64 / 1000.0));
                }
                Some(follow::Correction::Play) => {
                    send(&mut app, &main, PlaybackCommand::SetPaused(false));
                }
                Some(follow::Correction::Pause) => {
                    send(&mut app, &main, PlaybackCommand::SetPaused(true));
                }
                None => {}
            }
            main.set_jam_applying(false);
        },
    );
    Ok(Controller {
        window,
        timer,
        sender,
        worker: Some(worker),
        session,
        state,
        main: main.as_weak(),
    })
}
