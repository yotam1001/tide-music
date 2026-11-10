//! UI-facing feature controller. Request IDs prevent stale pages replacing navigation.
pub mod review;
use crate::{
    artwork,
    library::Action,
    preferences::Preferences,
    spotify::{self, Request, RequestSender},
    state::{self, Track},
    GroveFeatureTrack, GrovePlaylist, GroveWindow,
};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone)]
enum Pending {
    Page {
        epoch: u64,
        append: bool,
    },
    Playlists,
    Mutation,
    Metadata(String),
    Saved(String),
    Enqueue,
    JamMatch {
        invite: String,
        remote: String,
    },
    JamAdd {
        uri: String,
        invite: String,
        sender: tokio::sync::mpsc::UnboundedSender<crate::jam_transport::Request>,
    },
}
struct PreviousPage {
    page: i32,
    rows: Vec<Track>,
    source: Option<Action>,
    title: slint::SharedString,
    more: bool,
}
pub struct Features {
    pub prefs: Preferences,
    offline: bool,
    tx: RequestSender,
    serial: u64,
    epoch: u64,
    view_page: i32,
    pending: HashMap<u64, Pending>,
    source: Option<Action>,
    pub rows: Vec<Track>,
    art: artwork::Cache,
    metadata: Track,
    pub queue: Vec<Track>,
    starting: Option<(String, Option<u64>, Instant)>,
    rotation: Vec<Track>,
    played: Option<(String, Instant)>,
    last_tick: Instant,
    listened: Duration,
    pub history: Vec<Track>,
    previous: Option<PreviousPage>,
    jam_pairs: tide_jam::matching::Pairings,
    jam_remote: String,
    jam_candidates: Vec<Track>,
}
impl Features {
    pub fn install(ui: &GroveWindow, tx: RequestSender, offline: bool) -> Rc<RefCell<Self>> {
        let (prefs, error) = if offline {
            (Preferences::default(), None)
        } else {
            match Preferences::load() {
                Ok(p) => (p, None),
                Err(e) => (Preferences::default(), Some(e)),
            }
        };
        ui.set_light_mode(prefs.light);
        ui.set_compact_mode(prefs.compact);
        ui.set_autoplay(prefs.autoplay);
        ui.set_media_keys(prefs.media_keys);
        ui.set_local_history(prefs.local_history);
        if prefs.compact {
            ui.window().set_size(slint::LogicalSize::new(760.0, 640.0));
        }
        if let Some(error) = error {
            ui.set_message(error.into());
        }
        let this = Rc::new(RefCell::new(Self {
            prefs,
            offline,
            tx,
            serial: 0,
            epoch: 0,
            view_page: 0,
            pending: HashMap::new(),
            source: None,
            rows: vec![],
            art: artwork::Cache::new(offline),
            metadata: Track::default(),
            queue: vec![],
            starting: None,
            rotation: vec![],
            played: None,
            last_tick: Instant::now(),
            listened: Duration::ZERO,
            history: vec![],
            previous: None,
            jam_pairs: Default::default(),
            jam_remote: String::new(),
            jam_candidates: vec![],
        }));
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_jam_pair(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut f = f.borrow_mut();
            if !ui.get_jam_connected()
                || ui.get_jam_host()
                || !ui.get_jam_following()
                || f.jam_remote.is_empty()
            {
                return;
            }
            let uri = if index >= 0 {
                f.jam_candidates.get(index as usize).map(|t| t.uri.clone())
            } else {
                spotify::spotify_uri(&ui.get_jam_match_link()).ok()
            };
            let Some(uri) = uri.filter(|u| u.starts_with("spotify:track:")) else {
                ui.set_jam_message("Choose a song or paste its Spotify track link.".into());
                return;
            };
            let remote = f.jam_remote.clone();
            if f.jam_pairs.remember(&remote, &uri).is_ok() {
                ui.set_jam_match_confirmed(true);
                ui.set_jam_message(
                    "Song matched for this jam. Choose another if the recording differs.".into(),
                );
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_page_changed(move |page| {
            if let Some(ui) = weak.upgrade() {
                let Ok(mut f) = f.try_borrow_mut() else {
                    return;
                };
                if f.view_page == page {
                    return;
                }
                f.view_page = page;
                f.epoch += 1;
                ui.set_feature_busy(false);
                ui.set_feature_message("".into());
                match page {
                    3 if !ui.get_collection_uri().is_empty() => f.load(
                        &ui,
                        Action::Collection {
                            uri: ui.get_collection_uri().into(),
                            offset: 0,
                        },
                        false,
                    ),
                    5 => f.load(&ui, Action::Home, false),
                    6 => {
                        f.rows.clear();
                        f.source = None;
                        f.render(&ui);
                    }
                    9 => {
                        f.rows = f.history.clone();
                        f.source = None;
                        f.render(&ui);
                    }
                    _ => {}
                }
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_feature_back(move || {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                if let Some(previous) = f.previous.take() {
                    f.epoch += 1;
                    f.view_page = previous.page;
                    f.rows = previous.rows;
                    f.source = previous.source;
                    ui.set_page(previous.page);
                    ui.set_feature_title(previous.title);
                    ui.set_feature_more(previous.more);
                    ui.set_feature_busy(false);
                    ui.set_feature_message("".into());
                    f.render(&ui);
                }
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_search(move |query| {
            if let Some(ui) = weak.upgrade() {
                if let Ok(uri) = spotify::spotify_uri(&query) {
                    ui.invoke_play_uri(uri.into());
                    return;
                }
                f.borrow_mut().load(
                    &ui,
                    Action::Search {
                        query: query.into(),
                        offset: 0,
                    },
                    false,
                );
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_feature_refresh(move || {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                if let Some(action) = f.source.clone() {
                    f.load(&ui, first_page(action), false);
                }
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_feature_next(move || {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                if let Some(action) = f.source.clone() {
                    if let Some(next) = next_page(action) {
                        f.load(&ui, next, true);
                    }
                }
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_browse(move |uri, title| {
            if let Some(ui) = weak.upgrade() {
                ui.set_collection_uri(uri.clone());
                ui.set_collection_owned(ui.get_owned_playlists().iter().any(|p| p.uri == uri));
                ui.set_collection_title(title);
                f.borrow_mut().load(
                    &ui,
                    Action::Collection {
                        uri: uri.into(),
                        offset: 0,
                    },
                    false,
                );
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_song_action(move |action, uri| {
            if let Some(ui) = weak.upgrade() {
                if action == "play" {
                    ui.invoke_play_uri(uri);
                    return;
                }
                f.borrow_mut().song(&ui, &action, &uri);
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_appearance(move |action| {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                match action.as_str() {
                    "theme" => f.prefs.light = !f.prefs.light,
                    "compact" => {
                        f.prefs.compact = !f.prefs.compact;
                        ui.window().set_size(slint::LogicalSize::new(
                            if f.prefs.compact { 760.0 } else { 1200.0 },
                            if f.prefs.compact { 640.0 } else { 800.0 },
                        ));
                    }
                    "autoplay" => f.prefs.autoplay = !f.prefs.autoplay,
                    "media" => f.prefs.media_keys = !f.prefs.media_keys,
                    "history" => f.prefs.local_history = !f.prefs.local_history,
                    "clear-history" => {
                        f.history.clear();
                        ui.set_feature_message("Session listening history cleared.".into());
                    }
                    "reset-hidden" => f.prefs.hidden.clear(),
                    _ => return,
                }
                ui.set_light_mode(f.prefs.light);
                ui.set_compact_mode(f.prefs.compact);
                ui.set_autoplay(f.prefs.autoplay);
                ui.set_media_keys(f.prefs.media_keys);
                ui.set_local_history(f.prefs.local_history);
                f.save(&ui);
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_playlist_edit(move |remove| {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                let playlist = match spotify::spotify_uri(&ui.get_edit_playlist()) {
                    Ok(p) if p.starts_with("spotify:playlist:") => p,
                    _ => {
                        ui.set_feature_message(
                            "Paste a Spotify playlist link owned by your account.".into(),
                        );
                        return;
                    }
                };
                let uri = ui.get_edit_track().to_string();
                let action = if remove {
                    Action::PlaylistRemove { playlist, uri }
                } else {
                    Action::PlaylistAdd { playlist, uri }
                };
                if f.request(action, Pending::Mutation).is_some() {
                    ui.set_feature_message(
                        "Waiting for Spotify to confirm the playlist change…".into(),
                    );
                    ui.set_edit_track("".into());
                }
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_library_more(move || {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                let offset = ui.get_playlists().row_count() as u32;
                f.request(Action::Playlists { offset }, Pending::Playlists);
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_queue_edit(move |action, index| {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                let i = index.max(0) as usize;
                if f.starting.is_some() {
                    ui.set_message(
                        "Wait for the requested track to start before editing the queue.".into(),
                    );
                    return;
                }
                match action.as_str() {
                    "remove" if i < f.queue.len() => {
                        f.queue.remove(i);
                    }
                    "up" if i > 0 && i < f.queue.len() => f.queue.swap(i, i - 1),
                    "down" if i + 1 < f.queue.len() => f.queue.swap(i, i + 1),
                    "clear" => f.queue.clear(),
                    _ => {}
                }
                f.render_queue(&ui);
            }
        });
        let f = this.clone();
        let weak = ui.as_weak();
        ui.on_play_selection(move |shuffle| {
            if let Some(ui) = weak.upgrade() {
                let mut f = f.borrow_mut();
                if !ui.get_authenticated() || ui.get_jam_connected() {
                    ui.set_message(
                        "Enable playback and leave the jam before playing this selection.".into(),
                    );
                    return;
                }
                let mut rows: Vec<_> = f
                    .rows
                    .iter()
                    .filter(|t| !f.prefs.hidden.contains(&t.uri))
                    .take(500)
                    .cloned()
                    .collect();
                if shuffle {
                    shuffle_tracks(&mut rows);
                }
                if let Some(first) = rows.first().cloned() {
                    f.queue = rows.into_iter().skip(1).collect();
                    f.render_queue(&ui);
                    drop(f);
                    ui.invoke_play_uri(first.uri.into());
                }
            }
        });
        this
    }
    fn save(&self, ui: &GroveWindow) {
        if !self.offline {
            if let Err(e) = self.prefs.save() {
                ui.set_message(e.into());
            }
        }
    }
    pub fn volume(&mut self, ui: &GroveWindow, value: f32) {
        self.prefs.volume = value;
        self.save(ui);
    }
    fn request(&mut self, action: Action, pending: Pending) -> Option<u64> {
        if self.pending.len() >= 8 {
            return None;
        }
        if matches!(pending, Pending::Playlists)
            && self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Playlists))
        {
            return None;
        }
        self.serial = self.serial.wrapping_add(1);
        let id = self.serial;
        self.pending.insert(id, pending);
        if !self.offline {
            let _ = self.tx.send(Request::Feature { id, action });
        }
        Some(id)
    }
    fn load(&mut self, ui: &GroveWindow, action: Action, append: bool) {
        if !ui.get_library_ready() {
            ui.set_feature_message("Connect Spotify to browse your music.".into());
            return;
        }
        if append && ui.get_feature_busy() {
            return;
        }
        self.view_page = ui.get_page();
        // A new initial page invalidates an older request, including search A -> B.
        if !append {
            self.epoch += 1;
            self.art.clear();
            self.rows.clear();
            ui.set_feature_more(false);
            self.render(ui);
        }
        let pending = Pending::Page {
            epoch: self.epoch,
            append,
        };
        if self.request(action.clone(), pending).is_some() {
            self.source = Some(action);
            ui.set_feature_busy(true);
            ui.set_feature_message("Loading from Spotify…".into());
        } else {
            ui.set_feature_message("Wait for the current requests to finish.".into());
        }
    }
    fn song(&mut self, ui: &GroveWindow, action: &str, uri: &str) {
        let track = self
            .rows
            .iter()
            .chain(self.queue.iter())
            .find(|t| t.uri == uri)
            .cloned()
            .or_else(|| (self.metadata.uri == uri).then(|| self.metadata.clone()));
        match action {
            "Next" | "Queue" => {
                if self.starting.is_some() {
                    ui.set_message(
                        "Wait for the requested track to start before adding more songs.".into(),
                    );
                    return;
                }
                if ui.get_jam_connected() {
                    ui.set_message("Use the shared jam queue while in a jam.".into());
                    return;
                }
                if let Some(t) = track {
                    if self.queue.len() >= 500 {
                        ui.set_message("Grove's queue is full (500 tracks).".into());
                        return;
                    }
                    if action == "Next" {
                        self.queue.insert(0, t);
                    } else {
                        self.queue.push(t);
                    }
                    self.render_queue(ui);
                    ui.set_message("Added to your queue.".into());
                }
            }
            "Radio" => {
                self.remember(ui);
                ui.set_page(7);
                ui.set_feature_title("Song radio".into());
                self.load(ui, Action::Radio { uri: uri.into() }, false);
            }
            "Save" | "Unsave" => {
                self.request(
                    Action::Save {
                        uri: uri.into(),
                        saved: action == "Save",
                    },
                    Pending::Mutation,
                );
            }
            "Hide" => {
                if self.prefs.hidden.len() < 2000 {
                    self.prefs.hidden.insert(uri.into());
                    self.save(ui);
                    self.render(ui);
                    ui.set_feature_message(
                        "Song hidden. You can bring it back in Settings.".into(),
                    );
                }
            }
            "Artist" => {
                if let Some(t) = track.filter(|t| !t.artist_uri.is_empty()) {
                    self.remember(ui);
                    ui.set_page(7);
                    ui.set_feature_title(t.artist.into());
                    self.load(ui, Action::Artist { uri: t.artist_uri }, false);
                } else {
                    ui.set_feature_message(
                        "Artist information is not available for this track.".into(),
                    );
                }
            }
            "Playlist" | "Remove" => {
                ui.set_removing_from_playlist(action == "Remove");
                ui.set_edit_track(uri.into());
                ui.set_edit_playlist(
                    if ui.get_collection_uri().starts_with("spotify:playlist:") {
                        ui.get_collection_uri()
                    } else {
                        "".into()
                    },
                );
            }
            _ => {}
        }
    }
    pub fn response(&mut self, ui: &GroveWindow, id: u64, result: Result<Value, String>) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        if matches!(pending,Pending::Page{epoch,..} if epoch!=self.epoch) {
            return;
        }
        if matches!(&pending,Pending::Metadata(uri)|Pending::Saved(uri) if ui.get_current_uri().as_str()!=uri)
        {
            return;
        }
        if matches!(&pending,Pending::JamAdd{invite,..}|Pending::JamMatch{invite,..} if !ui.get_jam_connected() || invite!=ui.get_jam_invite().as_str())
        {
            return;
        }
        if matches!(&pending,Pending::JamMatch{remote,..} if remote!=&self.jam_remote) {
            return;
        }
        if matches!(pending, Pending::Page { .. }) {
            ui.set_feature_busy(false);
        }
        let data = match result {
            Ok(d) => d,
            Err(e) => {
                if matches!(pending, Pending::JamAdd { .. } | Pending::JamMatch { .. }) {
                    ui.set_jam_message(e.into());
                } else if matches!(pending, Pending::Enqueue) {
                    ui.set_message(e.into());
                } else {
                    ui.set_feature_message(e.into());
                }
                return;
            }
        };
        match pending {
            Pending::JamMatch { .. } => {
                self.jam_candidates = data["tracks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| {
                        v["uri"]
                            .as_str()
                            .is_some_and(|u| u.starts_with("spotify:track:"))
                    })
                    .take(10)
                    .map(|v| Track::from_entity(&crate::web_api::entity(v)))
                    .collect();
                ui.set_jam_matches(ModelRc::new(VecModel::from(
                    self.jam_candidates
                        .iter()
                        .map(|t| GroveFeatureTrack {
                            title: t.title.clone().into(),
                            artist: t.artist.clone().into(),
                            duration: state::clock(t.duration).into(),
                            uri: t.uri.clone().into(),
                            artist_uri: Default::default(),
                            cover: Default::default(),
                        })
                        .collect::<Vec<_>>(),
                )));
                ui.set_jam_message(
                    if self.jam_candidates.is_empty() {
                        "No matches found. Paste the Spotify link for this song."
                    } else {
                        "Choose the same recording. Timing may differ between services."
                    }
                    .into(),
                );
            }
            Pending::Enqueue => {
                if self.queue.len() >= 500 {
                    ui.set_message(
                        "Your queue is full. Remove a song before adding another.".into(),
                    );
                    return;
                }
                let track = Track::from_entity(&crate::web_api::entity(&data["track"]));
                if spotify::spotify_uri(&track.uri).is_err() {
                    ui.set_message("Spotify couldn't find this song.".into());
                    return;
                }
                self.queue.push(track);
                self.render_queue(ui);
                ui.set_message("Added to your queue.".into());
            }
            Pending::JamAdd {
                uri,
                invite,
                sender,
            } => {
                if ui.get_jam_connected() && ui.get_jam_invite().as_str() == invite {
                    let title = data["track"]["name"]
                        .as_str()
                        .unwrap_or("Spotify song")
                        .to_owned();
                    let _ = sender.send(crate::jam_transport::Request::Add { uri, title });
                    ui.set_jam_message("".into());
                }
            }
            Pending::Page { append, .. } => {
                let collection = data["kind"] == "collection_page";
                let items = if collection {
                    &data["items"]
                } else {
                    &data["tracks"]
                };
                let rows = items
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        let item = if collection {
                            v.get("item").or_else(|| v.get("track"))?
                        } else {
                            v
                        };
                        if item["uri"]
                            .as_str()
                            .is_some_and(|u| u.starts_with("spotify:track:"))
                        {
                            Some(Track::from_entity(&crate::web_api::entity(item)))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if !append {
                    self.rows.clear();
                }
                self.rows.extend(rows);
                self.rows.truncate(2000);
                if data["kind"] == "home" {
                    self.rotation = self.rows.clone();
                }
                if let Some(title) = data["title"].as_str() {
                    ui.set_feature_title(title.into());
                }
                ui.set_feature_more(
                    data["has_more"] == true
                        && self.rows.len() < 2000
                        && self.source.clone().and_then(next_page).is_some(),
                );
                ui.set_feature_message(
                    if self.rows.is_empty() {
                        "No playable tracks were returned."
                    } else {
                        data["note"].as_str().unwrap_or("")
                    }
                    .into(),
                );
                self.render(ui);
            }
            Pending::Playlists => {
                let mut rows: Vec<_> = ui.get_playlists().iter().collect();
                let mut owned: Vec<_> = ui.get_owned_playlists().iter().collect();
                for p in data["items"].as_array().into_iter().flatten() {
                    if let (Some(title), Some(uri)) = (p["name"].as_str(), p["uri"].as_str()) {
                        if p["owner"]["id"].is_string()
                            && p["owner"]["id"] == data["account"]
                            && !owned.iter().any(|p| p.uri.as_str() == uri)
                        {
                            owned.push(GrovePlaylist {
                                title: title.into(),
                                uri: uri.into(),
                                subtitle: "Your playlist".into(),
                            });
                        }
                        rows.push(GrovePlaylist {
                            title: title.into(),
                            uri: uri.into(),
                            subtitle: p["owner"]["display_name"]
                                .as_str()
                                .unwrap_or("Playlist")
                                .into(),
                        });
                    }
                }
                rows.truncate(2000);
                let has_more = data["has_more"] == true && rows.len() < 2000;
                owned.truncate(2000);
                ui.set_owned_playlists(ModelRc::new(VecModel::from(owned)));
                ui.set_playlists(ModelRc::new(VecModel::from(rows)));
                ui.set_playlists_more(has_more);
                ui.set_library_note("".into());
            }
            Pending::Mutation => {
                ui.set_feature_message(
                    data["message"]
                        .as_str()
                        .unwrap_or("Spotify confirmed the change.")
                        .into(),
                );
                if data["uri"].as_str() == Some(ui.get_current_uri().as_str())
                    && data["saved"].is_boolean()
                {
                    ui.set_current_saved(data["saved"] == true);
                }
                if data["saved"] == false
                    && matches!(&self.source,Some(Action::Collection{uri,..}) if uri=="liked")
                {
                    self.rows
                        .retain(|t| Some(t.uri.as_str()) != data["uri"].as_str());
                    self.render(ui);
                }
            }
            Pending::Metadata(uri) => {
                if ui.get_current_uri().as_str() == uri {
                    self.metadata = Track::from_entity(&crate::web_api::entity(&data["track"]));
                    ui.set_current_cover(self.art.get(&self.metadata.artwork_url));
                }
            }
            Pending::Saved(uri) => {
                if ui.get_current_uri().as_str() == uri {
                    ui.set_current_saved(data["saved"] == true);
                }
            }
        }
    }
    fn render(&mut self, ui: &GroveWindow) {
        let previous: HashMap<_, _> = ui
            .get_feature_tracks()
            .iter()
            .filter(|t| t.cover.size().width > 0)
            .map(|t| (t.uri, t.cover))
            .collect();
        let hidden = &self.prefs.hidden;
        let art = &mut self.art;
        let rows = self
            .rows
            .iter()
            .filter(|t| !hidden.contains(&t.uri))
            .map(|t| GroveFeatureTrack {
                title: t.title.clone().into(),
                artist: t.artist.clone().into(),
                duration: state::clock(t.duration).into(),
                uri: t.uri.clone().into(),
                artist_uri: t.artist_uri.clone().into(),
                cover: previous
                    .get(&slint::SharedString::from(t.uri.as_str()))
                    .cloned()
                    .unwrap_or_else(|| art.get(&t.artwork_url)),
            })
            .collect::<Vec<_>>();
        ui.set_feature_tracks(ModelRc::new(VecModel::from(rows)));
    }
    fn render_queue(&self, ui: &GroveWindow) {
        ui.set_local_queue(ModelRc::new(VecModel::from(
            self.queue
                .iter()
                .map(|t| crate::GroveTrack {
                    title: t.title.clone().into(),
                    artist: t.artist.clone().into(),
                    uri: t.uri.clone().into(),
                    duration: state::clock(t.duration).into(),
                })
                .collect::<Vec<_>>(),
        )));
    }
    pub fn next(&mut self, ui: &GroveWindow, state: &state::State) -> bool {
        if self.queue.is_empty() || !state.authenticated || ui.get_jam_connected() {
            return false;
        }
        if self.starting.is_some() {
            return true;
        }
        let uri = self.queue[0].uri.clone();
        self.starting = Some((uri.clone(), state.play_request_id, Instant::now()));
        if !self.offline {
            let _ = self
                .tx
                .send(Request::Command(json!({"command":"play","uri":uri})));
        }
        true
    }
    pub fn ended(&mut self, ui: &GroveWindow, state: &state::State, id: Option<u64>) {
        if id != state.play_request_id
            || id.is_none()
            || ui.get_jam_connected()
            || ui.get_local_active()
            || state.loading
        {
            return;
        }
        if self.queue.is_empty() && self.prefs.autoplay {
            if let Some(index) = self
                .rotation
                .iter()
                .position(|t| t.uri != state.track.uri && !self.prefs.hidden.contains(&t.uri))
            {
                let track = self.rotation.remove(index);
                self.rotation.push(track.clone());
                self.queue.push(track);
                self.render_queue(ui);
            }
        }
        self.next(ui, state);
    }
    pub fn tick(&mut self, ui: &GroveWindow, state: &state::State) {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_tick)
            .min(Duration::from_millis(500));
        self.last_tick = now;
        if ui.get_current_uri().as_str() != state.track.uri {
            ui.set_current_uri(state.track.uri.clone().into());
            ui.set_current_cover(self.art.get(&state.track.artwork_url));
            ui.set_current_saved(false);
            self.metadata = state.track.clone();
            self.listened = Duration::ZERO;
            self.played = None;
            if state.track.uri.starts_with("spotify:track:") && ui.get_library_ready() {
                self.request(
                    Action::TrackInfo {
                        uri: state.track.uri.clone(),
                    },
                    Pending::Metadata(state.track.uri.clone()),
                );
                self.request(
                    Action::Contains {
                        uri: state.track.uri.clone(),
                    },
                    Pending::Saved(state.track.uri.clone()),
                );
            }
        }
        if let Some((uri, old, since)) = &self.starting {
            if state.track.uri == *uri
                && state.play_request_id.is_some()
                && state.play_request_id != *old
                && state.active
            {
                if self.queue.first().is_some_and(|t| t.uri == *uri) {
                    self.queue.remove(0);
                }
                self.starting = None;
                self.render_queue(ui);
            } else if since.elapsed() > Duration::from_secs(15) {
                self.starting = None;
                ui.set_message(
                    "Playback was not confirmed. The Grove queue was preserved; retry Next.".into(),
                );
            }
        }
        if state.playing
            && state.active
            && !state.loading
            && self.prefs.local_history
            && !state.track.uri.is_empty()
        {
            self.listened += elapsed;
            if self.listened >= Duration::from_secs(30) && self.played.is_none() {
                self.history.insert(0, state.track.clone());
                self.history.truncate(100);
                self.played = Some((state.track.uri.clone(), now));
            }
        }
        if self.art.poll() {
            self.render(ui);
            ui.set_current_cover(self.art.get(&self.metadata.artwork_url));
        }
    }
    pub fn clear(&mut self, ui: &GroveWindow) {
        self.reset_jam(ui);
        self.epoch += 1;
        self.pending.clear();
        self.rows.clear();
        self.queue.clear();
        self.rotation.clear();
        self.history.clear();
        self.starting = None;
        self.source = None;
        self.previous = None;
        self.art.clear();
        self.metadata = Track::default();
        ui.set_current_cover(slint::Image::default());
        ui.set_current_uri("".into());
        ui.set_current_saved(false);
        ui.set_feature_busy(false);
        ui.set_feature_more(false);
        ui.set_feature_message("".into());
        ui.set_edit_track("".into());
        self.render(ui);
        self.render_queue(ui);
    }
    pub fn grant_ready(&mut self, ui: &GroveWindow) {
        self.invalidate(ui);
        if ui.get_page() == 5 {
            self.load(ui, Action::Home, false);
            return;
        }
        if matches!(ui.get_page(), 5..=7)
            || (ui.get_page() == 3 && !ui.get_collection_uri().is_empty())
        {
            if let Some(action) = self.source.clone() {
                self.load(ui, first_page(action), false);
            }
        }
    }
    pub fn reset_jam(&mut self, ui: &GroveWindow) {
        self.jam_pairs = Default::default();
        self.jam_remote.clear();
        self.jam_candidates.clear();
        self.pending
            .retain(|_, p| !matches!(p, Pending::JamMatch { .. }));
        ui.set_jam_matching(false);
        ui.set_jam_tide_host(false);
        ui.set_jam_match_confirmed(false);
        ui.set_jam_matches(ModelRc::default());
        ui.set_jam_match_link("".into());
    }
    /// Resolve only to an explicitly confirmed Spotify recording; never guess a cover.
    pub fn paired_host(
        &mut self,
        ui: &GroveWindow,
        remote: &crate::jam::Playback,
    ) -> Option<String> {
        if remote.uri.starts_with("spotify:track:") {
            ui.set_jam_matching(false);
            return Some(remote.uri.clone());
        }
        if !remote.uri.starts_with("youtube:") {
            return None;
        }
        ui.set_jam_tide_host(true);
        self.jam_pairs.set_room(&ui.get_jam_invite());
        ui.set_jam_matching(true);
        if self.jam_remote != remote.uri {
            self.jam_remote = remote.uri.clone();
            self.jam_candidates.clear();
            ui.set_jam_matches(ModelRc::default());
            ui.set_jam_match_link("".into());
            ui.set_jam_match_title(format!("{} · {}", remote.title, remote.artist).into());
            ui.set_jam_match_confirmed(self.jam_pairs.get(&remote.uri).is_some());
            if ui.get_library_ready() {
                ui.set_jam_message("Finding this song on Spotify…".into());
                let requested = self.request(
                    Action::Search {
                        query: format!("{} {}", remote.title, remote.artist)
                            .chars()
                            .take(200)
                            .collect(),
                        offset: 0,
                    },
                    Pending::JamMatch {
                        invite: ui.get_jam_invite().into(),
                        remote: remote.uri.clone(),
                    },
                );
                if requested.is_none() {
                    ui.set_jam_message(
                        "Spotify is busy. Paste the matching song link to continue.".into(),
                    );
                }
            }
        }
        self.jam_pairs.get(&remote.uri).map(str::to_owned)
    }
    pub fn enqueue_link(&mut self, ui: &GroveWindow, input: &str) {
        if ui.get_jam_connected() {
            ui.set_message("Add this song to the shared jam instead.".into());
            return;
        }
        match spotify::spotify_uri(input) {
            Ok(uri) if uri.starts_with("spotify:track:") => {
                if self
                    .request(Action::TrackInfo { uri }, Pending::Enqueue)
                    .is_none()
                {
                    ui.set_message("Wait for the current request, then try again.".into());
                } else {
                    ui.set_message("Finding your song…".into());
                }
            }
            _ => ui.set_message("Paste a Spotify song link.".into()),
        }
    }
    pub fn resolve_jam(
        &mut self,
        ui: &GroveWindow,
        uri: String,
        sender: tokio::sync::mpsc::UnboundedSender<crate::jam_transport::Request>,
    ) {
        self.pending.retain(
            |_, p| !matches!(p,Pending::JamAdd{invite,..} if invite!=ui.get_jam_invite().as_str()),
        );
        if self
            .pending
            .values()
            .any(|p| matches!(p, Pending::JamAdd { .. }))
        {
            return;
        }
        let invite = ui.get_jam_invite().to_string();
        if self
            .request(
                Action::TrackInfo { uri: uri.clone() },
                Pending::JamAdd {
                    uri,
                    invite,
                    sender,
                },
            )
            .is_some()
        {
            ui.set_jam_message("Finding your song…".into());
        } else {
            ui.set_jam_message("Wait a moment, then try again.".into());
        }
    }
    pub fn invalidate(&mut self, ui: &GroveWindow) {
        self.epoch += 1;
        self.pending.clear();
        ui.set_feature_busy(false);
    }
    fn remember(&mut self, ui: &GroveWindow) {
        if ui.get_page() != 7 {
            self.previous = Some(PreviousPage {
                page: ui.get_page(),
                rows: self.rows.clone(),
                source: self.source.clone(),
                title: ui.get_feature_title(),
                more: ui.get_feature_more(),
            });
        }
    }
}
fn first_page(action: Action) -> Action {
    match action {
        Action::Search { query, .. } => Action::Search { query, offset: 0 },
        Action::Collection { uri, .. } => Action::Collection { uri, offset: 0 },
        other => other,
    }
}
fn next_page(action: Action) -> Option<Action> {
    match action {
        Action::Search { query, offset } if offset < 1000 => Some(Action::Search {
            query,
            offset: offset + 10,
        }),
        Action::Collection { uri, offset } if offset < 100_000 => Some(Action::Collection {
            uri,
            offset: offset + 50,
        }),
        _ => None,
    }
}
fn shuffle_tracks(tracks: &mut [Track]) {
    use std::hash::{Hash, Hasher};
    let salt = crate::jam::id();
    tracks.sort_by_cached_key(|t| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        salt.hash(&mut h);
        t.uri.hash(&mut h);
        h.finish()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_preserves_collection_and_search() {
        assert!(
            matches!(next_page(Action::Search{query:"a & b".into(),offset:20}),Some(Action::Search{query,offset:30}) if query=="a & b")
        );
        assert!(next_page(Action::Home).is_none());
        assert!(next_page(Action::Search {
            query: "a".into(),
            offset: 1000
        })
        .is_none());
    }
}
