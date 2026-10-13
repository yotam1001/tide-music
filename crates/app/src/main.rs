#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tide_core::{PlaybackCommand, Queue, Track};
mod artwork;
mod listening;
mod radio;
slint::include_modules!();

#[derive(Serialize, Deserialize)]
struct Settings {
    queue: Queue,
    light_mode: bool,
    #[serde(default)]
    compact_mode: bool,
    #[serde(default = "enabled_by_default")]
    autoplay: bool,
    volume: Option<f64>,
    repeat: bool,
    #[serde(skip)]
    transient: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            queue: Queue::default(),
            light_mode: false,
            compact_mode: false,
            autoplay: true,
            volume: None,
            repeat: false,
            transient: false,
        }
    }
}
fn enabled_by_default() -> bool {
    true
}
#[derive(Clone, Default)]
enum LibraryAction {
    #[default]
    Replace,
    Connect,
    Shuffle,
    Preview {
        title: String,
    },
    Radio {
        seed: String,
        generation: u64,
        replace_upcoming: bool,
    },
}
impl LibraryAction {
    fn accepts_response(&self, generation: u64, seed: Option<&str>, autoplay: bool) -> bool {
        match self {
            Self::Radio {
                seed: requested_seed,
                generation: requested_generation,
                replace_upcoming,
            } => {
                *requested_generation == generation
                    && seed == Some(requested_seed.as_str())
                    && (autoplay || *replace_upcoming)
            }
            _ => true,
        }
    }
}
struct State {
    settings: Settings,
    engine: Option<tide_playback::PlaybackHandle>,
    generation: u64,
    active_playback: bool,
    command_error: Option<String>,
    search_generation: u64,
    search_cancel: Arc<AtomicBool>,
    search_worker: Option<std::thread::JoinHandle<()>>,
    provider: Option<tide_provider::Provider>,
    cookie_lease: Option<tide_provider::CookieLease>,
    pending_cookie_updates: std::collections::VecDeque<tide_playback::CookieUpdate>,
    cookie_update_retry_at: Option<Instant>,
    cookie_updates_applied: u64,
    cookie_updates_not_applied: u64,
    library_worker: Option<std::thread::JoinHandle<()>>,
    library_cancel: Arc<AtomicBool>,
    library_playlists: Vec<tide_provider::Playlist>,
    recommendations: Vec<tide_provider::Card>,
    detail_tracks: Vec<Track>,
    search_tracks: Vec<Track>,
    account_error: bool,
    account_verified: bool,
    library_action: LibraryAction,
    library_request: Option<tide_provider::Request>,
    library_retry: Option<(tide_provider::Request, bool)>,
    shuffle_request: bool,
    radio_requests: radio::RadioRequests,
    radio_resume_when_ready: bool,
    volume_save_pending: bool,
    artwork: RefCell<artwork::ArtworkCache>,
}

type LibraryResult = Result<tide_provider::Response, String>;
async fn select_zen_session(index: i32) -> Option<tide_provider::Request> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Connect your Zen session: select cookies.sqlite in your profile")
        .add_filter("Zen cookie database", &["sqlite"])
        .set_file_name("cookies.sqlite");
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dialog = dialog.set_directory(PathBuf::from(appdata).join("zen/Profiles"));
    }
    dialog
        .pick_file()
        .await
        .map(|database| tide_provider::Request::Connect {
            database: database.path().to_owned(),
            index: index.clamp(0, 9) as u32,
        })
}
// Native pickers run on rfd's COM worker, so the Slint event loop keeps
// advancing the queue and delivering media commands while selection is open.
#[derive(Default)]
struct PickerState {
    busy: Cell<bool>,
    account_revision: Cell<u64>,
}
impl PickerState {
    fn cancel_account(&self) {
        self.account_revision
            .set(self.account_revision.get().wrapping_add(1));
    }
}
struct PickerPermit(Rc<PickerState>);
impl Drop for PickerPermit {
    fn drop(&mut self) {
        self.0.busy.set(false);
    }
}
fn spawn_picker(
    busy: &Rc<PickerState>,
    future: impl std::future::Future<Output = ()> + 'static,
) -> Result<(), String> {
    if busy.busy.replace(true) {
        return Err("A file picker is already open".into());
    }
    let permit = PickerPermit(busy.clone());
    slint::spawn_local(async move {
        let _permit = permit;
        future.await;
    })
    .map(|_| ())
    .map_err(|_| "Could not open the file picker".into())
}
fn connect_picker(
    busy: &Rc<PickerState>,
    state: Rc<RefCell<State>>,
    ui: &AppWindow,
    tx: mpsc::SyncSender<LibraryResult>,
) {
    let weak = ui.as_weak();
    let index = ui.get_account_index();
    let revision = busy.account_revision.get();
    let selection = busy.clone();
    let state = Rc::downgrade(&state);
    if let Err(error) = spawn_picker(busy, async move {
        let request = select_zen_session(index).await;
        if selection.account_revision.get() != revision {
            return;
        }
        if let (Some(request), Some(u), Some(state)) = (request, weak.upgrade(), state.upgrade()) {
            if state.borrow().library_worker.is_some() {
                u.set_account_status(
                    "A request is still in progress. Try connecting again.".into(),
                );
                return;
            }
            request_library(&mut state.borrow_mut(), &u, &tx, request);
        }
    }) {
        ui.set_account_status(error.into());
    }
}
fn request_library(
    s: &mut State,
    u: &AppWindow,
    tx: &mpsc::SyncSender<LibraryResult>,
    request: tide_provider::Request,
) {
    if s.library_worker.is_some() {
        return;
    }
    let connecting = matches!(
        request,
        tide_provider::Request::Connect { .. } | tide_provider::Request::SwitchAccount { .. }
    );
    let action = match &request {
        tide_provider::Request::Connect { .. } | tide_provider::Request::SwitchAccount { .. } => {
            LibraryAction::Connect
        }
        tide_provider::Request::Radio { video_id } => LibraryAction::Radio {
            seed: video_id.clone(),
            generation: s.generation,
            replace_upcoming: false,
        },
        tide_provider::Request::Playlist { .. } if s.shuffle_request => LibraryAction::Shuffle,
        tide_provider::Request::Playlist { id } => LibraryAction::Preview {
            title: s
                .library_playlists
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.title.clone())
                .or_else(|| {
                    s.recommendations
                        .iter()
                        .find(|p| p.id == *id)
                        .map(|p| p.title.clone())
                })
                .unwrap_or_else(|| "Playlist".into()),
        },
        tide_provider::Request::Liked => LibraryAction::Preview {
            title: "Liked Songs".into(),
        },
        _ => LibraryAction::Replace,
    };
    let Some(provider) = s
        .provider
        .clone()
        .filter(|p| connecting || p.is_connected())
    else {
        u.set_account_status("Import a local signed-in YouTube Music session first".into());
        return;
    };
    s.library_cancel.store(false, Ordering::Release);
    if let LibraryAction::Preview { title } = &action {
        s.detail_tracks.clear();
        u.set_detail_title(title.clone().into());
        u.set_detail_open(true);
        u.set_detail_busy(true);
        u.set_search_open(false);
        u.set_detail_rows(ModelRc::new(VecModel::default()));
    }
    s.library_action = action;
    s.library_request = Some(request.clone());
    s.shuffle_request = false;
    let cancel = s.library_cancel.clone();
    let tx = tx.clone();
    u.set_account_busy(true);
    u.set_account_status(
        if connecting {
            "Verifying your selected YouTube Music account…"
        } else {
            "Fetching YouTube Music…"
        }
        .into(),
    );
    s.library_worker = Some(std::thread::spawn(move || {
        let _ = tx.try_send(provider.execute(request, cancel));
    }));
}

fn current_video_id(s: &State) -> Option<String> {
    let track = s
        .settings
        .queue
        .current
        .and_then(|index| s.settings.queue.tracks.get(index))?;
    let url = url::Url::parse(&track.source).ok()?;
    if !is_youtube_source(&track.source) {
        return None;
    }
    let id = if url.host_str() == Some("youtu.be") {
        url.path().trim_start_matches('/').to_string()
    } else {
        url.query_pairs()
            .find(|(key, _)| key == "v")?
            .1
            .into_owned()
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
    .then_some(id)
}
fn request_radio(s: &mut State, u: &AppWindow, tx: &mpsc::SyncSender<LibraryResult>, force: bool) {
    if s.library_worker.is_some() || !s.account_verified || (!force && !s.settings.autoplay) {
        return;
    }
    let Some(index) = s.settings.queue.current else {
        return;
    };
    if !force
        && (s.settings.queue.tracks.len().saturating_sub(index + 1) > 2
            || !s.radio_requests.ready(s.generation, Instant::now()))
    {
        return;
    }
    let Some(seed) = current_video_id(s) else {
        return;
    };
    if force {
        s.radio_requests.reset();
    }
    s.radio_requests.started(s.generation);
    request_library(s, u, tx, tide_provider::Request::Radio { video_id: seed });
    if force {
        if let LibraryAction::Radio {
            replace_upcoming, ..
        } = &mut s.library_action
        {
            *replace_upcoming = true;
        }
    }
}

fn library_failure(s: &mut State, u: &AppWindow, authentication: bool) {
    if !s.library_action.accepts_response(
        s.generation,
        current_video_id(s).as_deref(),
        s.settings.autoplay,
    ) {
        return;
    }
    // Candidate verification does not replace the encrypted session on failure.
    // Keep the previously verified account and its playback intact.
    if matches!(s.library_action, LibraryAction::Connect) && s.account_verified {
        u.set_account_status(
            "Selected account could not be verified. Your connected account is unchanged.".into(),
        );
        return;
    }
    if authentication {
        s.library_retry = None;
        s.account_verified = false;
        u.set_account_verified(false);
        s.radio_resume_when_ready = false;
        s.account_error = true;
        u.set_error_is_account(true);
        u.set_error_message("Reconnect your YouTube Music account".into());
        u.set_account_status("YouTube rejected this session. Reconnect using the signed-in Zen profile and selected account.".into());
        return;
    }
    if let LibraryAction::Radio { generation, .. } = &s.library_action {
        if *generation != s.generation {
            return;
        }
        s.library_retry = None;
        if s.radio_requests.failed(Instant::now()) {
            u.set_account_status(
                "Recommendations unavailable temporarily; retrying shortly".into(),
            );
            if s.radio_resume_when_ready {
                u.set_status_text("Reconnecting recommendations…".into());
            }
            return;
        }
        s.radio_resume_when_ready = false;
        s.account_error = false;
        u.set_error_is_account(false);
        u.set_error_retryable(true);
        u.set_error_message("Recommendations unavailable. Retry to continue.".into());
    } else if matches!(s.library_action, LibraryAction::Connect) {
        s.library_retry = None;
        s.account_error = true;
        u.set_error_is_account(true);
        u.set_error_retryable(false);
        u.set_error_message(
            "Account verification failed temporarily. Open Account to retry.".into(),
        );
    } else {
        s.library_retry = s
            .library_request
            .as_ref()
            .and_then(content_retry_request)
            .map(|request| (request, matches!(s.library_action, LibraryAction::Shuffle)));
        s.account_error = false;
        u.set_error_is_account(false);
        u.set_error_retryable(s.library_retry.is_some());
        u.set_error_message(
            "YouTube Music unavailable temporarily. Check your connection and retry.".into(),
        );
    }
    u.set_account_status("Request failed; check your connection and retry".into());
}

fn content_retry_request(request: &tide_provider::Request) -> Option<tide_provider::Request> {
    match request {
        tide_provider::Request::Library
        | tide_provider::Request::Home
        | tide_provider::Request::Playlist { .. }
        | tide_provider::Request::Liked => Some(request.clone()),
        // Account selection needs explicit account UI; radio retries must retain
        // their generation and seed checks through request_radio.
        _ => None,
    }
}

fn sync_playback_cookies(s: &mut State) -> bool {
    if s.library_worker.is_some()
        || s.cookie_update_retry_at
            .is_some_and(|at| Instant::now() < at)
    {
        return false;
    }
    let (Some(provider), Some(engine)) = (&s.provider, &s.engine) else {
        return false;
    };
    if s.pending_cookie_updates.is_empty() {
        s.pending_cookie_updates
            .extend(engine.take_cookie_updates());
    }
    if s.pending_cookie_updates.is_empty() && s.cookie_update_retry_at.is_none() {
        return false;
    }
    let mut refresh_needed = s.cookie_update_retry_at.is_some();
    let mut session_updated = false;
    while let Some(update) = s.pending_cookie_updates.front() {
        match provider.merge_playback_cookies(
            update.account_index,
            update.baseline(),
            update.updated(),
        ) {
            Ok(changed) => {
                refresh_needed |= changed;
                session_updated |= changed;
                if changed {
                    s.cookie_updates_applied = s.cookie_updates_applied.saturating_add(1);
                } else {
                    s.cookie_updates_not_applied = s.cookie_updates_not_applied.saturating_add(1);
                }
                s.pending_cookie_updates.pop_front();
            }
            Err(_) => {
                // Keep the bounded credential batch for the next attempt;
                // transient file sharing must not silently discard rotation.
                s.cookie_update_retry_at = Some(Instant::now() + Duration::from_secs(2));
                return session_updated;
            }
        }
    }
    if let Some(lease) = s.cookie_lease.as_ref().filter(|_| refresh_needed) {
        if provider.refresh_cookie_lease(lease).is_err() {
            s.cookie_update_retry_at = Some(Instant::now() + Duration::from_secs(2));
            return session_updated;
        }
    }
    s.cookie_update_retry_at = None;
    session_updated
}

fn configure_account_playback(s: &mut State, u: &AppWindow) -> bool {
    s.library_request = None;
    s.library_retry = None;
    s.pending_cookie_updates.clear();
    s.cookie_update_retry_at = None;
    s.engine.take(); // Drop/join helpers before releasing their cookie lease.
    s.cookie_updates_applied = 0;
    s.cookie_updates_not_applied = 0;
    s.cookie_lease.take();
    s.active_playback = false;
    s.radio_resume_when_ready = false;
    s.radio_requests.reset();
    s.detail_tracks.clear();
    u.set_detail_open(false);
    u.set_detail_busy(false);
    u.set_detail_title("".into());
    u.set_detail_rows(ModelRc::new(VecModel::default()));
    s.artwork.borrow_mut().clear();
    s.account_verified = false;
    u.set_account_verified(false);
    s.account_error = false;
    u.set_error_is_account(false);
    u.set_error_message("".into());
    u.set_paused(true);
    u.set_position(0.0);
    u.set_duration(0.0);
    u.set_status_text("Ready".into());
    let lease = s
        .provider
        .as_ref()
        .map(|p| p.cookie_lease())
        .transpose()
        .map(|value| value.flatten());
    let result = lease.and_then(|lease| {
        let engine = tide_playback::start_with_account(
            lease.as_ref().map(|l| l.path().to_path_buf()),
            s.provider
                .as_ref()
                .map(|provider| provider.account_index())
                .unwrap_or(0),
        )?;
        s.cookie_lease = lease;
        Ok(engine)
    });
    match result {
        Ok(engine) => {
            s.engine = Some(engine);
            s.command_error = None;
            send(s, u, PlaybackCommand::SetVolume(u.get_volume() as f64));
            true
        }
        Err(error) => {
            report_error(s, u, error);
            false
        }
    }
}

fn refresh_library_models(s: &State, u: &AppWindow) {
    let mut artwork = s.artwork.borrow_mut();
    let playlists: Vec<PlaylistRow> = s
        .library_playlists
        .iter()
        .enumerate()
        .map(|(index, p)| PlaylistRow {
            id: p.id.clone().into(),
            title: p.title.clone().into(),
            subtitle: p.subtitle.clone().into(),
            artwork: p
                .artwork_url
                .as_deref()
                .filter(|_| {
                    index < 12
                        && u.get_library_open()
                        && !u.get_detail_open()
                        && !u.get_search_open()
                })
                .map(|url| artwork.get_or_request(url))
                .unwrap_or_default(),
        })
        .collect();
    u.set_library_playlists(ModelRc::new(VecModel::from(playlists)));
    let cards: Vec<RecommendationRow> = s
        .recommendations
        .iter()
        .enumerate()
        .map(|(index, p)| RecommendationRow {
            id: p.id.clone().into(),
            title: p.title.clone().into(),
            subtitle: p.subtitle.clone().into(),
            kind: p.kind.clone().into(),
            shelf: p.shelf.clone().into(),
            artwork: p
                .artwork_url
                .as_deref()
                .filter(|_| {
                    index < 12
                        && !u.get_library_open()
                        && !u.get_detail_open()
                        && !u.get_search_open()
                })
                .map(|url| artwork.get_or_request(url))
                .unwrap_or_default(),
        })
        .collect();
    u.set_recommendations(ModelRc::new(VecModel::from(cards)));
}
fn settings_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("TideMusic/settings.json")
}
fn save(settings: &Settings) {
    if settings.transient {
        return;
    }
    let path = settings_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(settings) {
        let _ = std::fs::write(path, bytes);
    }
}
fn sanitize_queue(queue: &mut Queue) -> bool {
    let old_current = queue.current;
    let mut changed = false;
    queue.current = None;
    for (index, mut track) in std::mem::take(&mut queue.tracks).into_iter().enumerate() {
        let source = track.source.trim();
        if source.contains("://") || source.starts_with("http:") || source.starts_with("https:") {
            match tide_playback::validate_playback_source(source) {
                Ok(validated) => {
                    changed |= validated != track.source;
                    track.source = validated;
                }
                Err(_) => {
                    changed = true;
                    continue;
                }
            }
        }
        if old_current == Some(index) {
            queue.current = Some(queue.tracks.len());
        }
        queue.tracks.push(track);
    }
    changed |= queue.current != old_current;
    changed
}
fn content_rows(tracks: &[Track], s: &State, visible: bool) -> ModelRc<TrackRow> {
    let mut artwork = s.artwork.borrow_mut();
    ModelRc::new(VecModel::from(
        tracks
            .iter()
            .enumerate()
            .map(|(index, t)| TrackRow {
                title: t.title.clone().into(),
                artist: t.artist.clone().into(),
                album: t.album.clone().into(),
                duration: "—".into(),
                recommended: t.recommended,
                artwork: t
                    .artwork_url
                    .as_deref()
                    .filter(|_| visible && index < 12)
                    .map(|url| artwork.get_or_request(url))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>(),
    ))
}
fn refresh_content_models(s: &State, u: &AppWindow) {
    u.set_detail_rows(content_rows(&s.detail_tracks, s, u.get_detail_open()));
    u.set_search_rows(content_rows(&s.search_tracks, s, u.get_search_open()));
}
fn recommendation_track(card: &tide_provider::Card) -> Track {
    Track {
        title: card.title.clone(),
        artist: card.subtitle.clone(),
        album: String::new(),
        source: format!("https://www.youtube.com/watch?v={}", card.id),
        artwork_url: card.artwork_url.clone(),
        recommended: false,
    }
}
fn start_tracks(
    s: &mut State,
    u: &AppWindow,
    mut tracks: Vec<Track>,
    index: usize,
    shuffle: bool,
) -> bool {
    tracks.truncate(tide_core::MAX_QUEUE_TRACKS);
    if index >= tracks.len() {
        return false;
    }
    for t in &mut tracks {
        t.recommended = false;
    }
    let previous = s.settings.queue.clone();
    s.settings.queue = Queue {
        tracks,
        current: Some(index),
    };
    if shuffle {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        s.settings.queue.shuffle_all(seed);
    }
    if play(s, u) {
        u.set_error_message("".into());
        true
    } else {
        s.settings.queue = previous;
        refresh(u, s);
        false
    }
}
fn queue_choice(s: &mut State, u: &AppWindow, track: Track, play_next: bool) {
    if s.settings.queue.insert_manual(track, play_next) {
        refresh(u, s);
        save(&s.settings);
        u.set_status_text(
            if play_next {
                "Will play next"
            } else {
                "Added to your queue"
            }
            .into(),
        );
    } else {
        u.set_status_text("Queue limit reached (2,000 tracks)".into());
    }
}

fn refresh(ui: &AppWindow, state: &State) {
    let mut artwork = state.artwork.borrow_mut();
    let current = state.settings.queue.current.unwrap_or(0);
    let current_art = state
        .settings
        .queue
        .tracks
        .get(current)
        .and_then(|track| track.artwork_url.as_deref())
        .map(|url| artwork.get_or_request(url))
        .unwrap_or_default();
    let rows: Vec<TrackRow> = state
        .settings
        .queue
        .tracks
        .iter()
        .enumerate()
        .map(|(index, t)| TrackRow {
            title: t.title.clone().into(),
            artist: t.artist.clone().into(),
            album: t.album.clone().into(),
            duration: "—".into(),
            recommended: t.recommended,
            artwork: t
                .artwork_url
                .as_deref()
                .filter(|_| index.abs_diff(current) <= 8)
                .map(|url| artwork.get_or_request(url))
                .unwrap_or_default(),
        })
        .collect();
    ui.set_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_current_index(state.settings.queue.current.map_or(-1, |i| i as i32));
    if let Some(t) = state
        .settings
        .queue
        .current
        .and_then(|i| state.settings.queue.tracks.get(i))
    {
        ui.set_track_title(t.title.clone().into());
        ui.set_track_artist(t.artist.clone().into());
        ui.set_track_album(t.album.clone().into());
        ui.set_track_artwork(current_art);
    } else {
        ui.set_track_title("Nothing playing".into());
        ui.set_track_artist("Add a file or paste a music link".into());
        ui.set_track_album("".into());
        ui.set_track_artwork(slint::Image::default());
    }
}
fn report_error(state: &mut State, ui: &AppWindow, error: String) {
    state.library_retry = None;
    ui.set_error_retryable(false);
    state.command_error = Some(error.clone());
    ui.set_error_message(error.clone().into());
    ui.set_status_text(error.into());
}

fn start_window_media(ui: &AppWindow) -> Result<tide_windows::MediaSession, String> {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let window = ui.window().window_handle();
        let handle = window
            .window_handle()
            .map_err(|_| "Native window is not ready".to_string())?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("Windows media controls need a native window".into());
        };
        let weak = ui.as_weak();
        tide_windows::start(handle.hwnd.get() as *mut std::ffi::c_void, move |action| {
            let (code, value) = match action {
                tide_windows::MediaAction::Play => (0, 0.0),
                tide_windows::MediaAction::Pause => (1, 0.0),
                tide_windows::MediaAction::Toggle => (2, 0.0),
                tide_windows::MediaAction::Next => (3, 0.0),
                tide_windows::MediaAction::Previous => (4, 0.0),
                tide_windows::MediaAction::Stop => (5, 0.0),
                tide_windows::MediaAction::Seek(seconds) => (6, seconds as f32),
            };
            let _ = weak.upgrade_in_event_loop(move |u| u.invoke_media_command(code, value));
        })
    }
    #[cfg(not(windows))]
    {
        let _ = ui;
        Err("Windows media controls are unavailable on this platform".into())
    }
}

fn preparation_eligible(active: bool, generation: u64, p: &tide_core::PlaybackSnapshot) -> bool {
    // EOF is a handoff: the queue timer still needs to consume the prepared
    // next stream. Clearing it here makes timer ordering trigger extraction again.
    active && p.generation == generation && !p.loading && p.error.is_none()
}

fn upcoming_source(queue: &Queue, repeat: bool) -> Option<&str> {
    let current = queue.current?;
    if current >= queue.tracks.len() {
        return None;
    }
    let next = current + 1;
    queue
        .tracks
        .get(next)
        .or_else(|| repeat.then(|| queue.tracks.first()).flatten())
        .map(|track| track.source.as_str())
}

fn send(state: &mut State, ui: &AppWindow, cmd: PlaybackCommand) -> bool {
    let result = state
        .engine
        .as_ref()
        .ok_or_else(|| "Playback runtime unavailable. See setup instructions.".to_string())
        .and_then(|engine| engine.send(cmd));
    match result {
        Ok(()) => {
            state.command_error = None;
            true
        }
        Err(error) => {
            report_error(state, ui, error);
            false
        }
    }
}

fn submit_load(
    generation: &mut u64,
    active: &mut bool,
    submit: impl FnOnce(u64) -> Result<(), String>,
) -> Result<(), String> {
    let next = generation
        .checked_add(1)
        .ok_or("Playback generation limit reached")?;
    submit(next)?;
    *generation = next;
    *active = true;
    Ok(())
}

fn play(state: &mut State, ui: &AppWindow) -> bool {
    let Some(source) = state
        .settings
        .queue
        .current
        .and_then(|i| state.settings.queue.tracks.get(i))
        .map(|track| track.source.clone())
    else {
        return false;
    };
    if is_youtube_source(&source)
        && state.provider.as_ref().is_some_and(|p| p.is_connected())
        && !state.account_verified
    {
        state.account_error = true;
        ui.set_error_is_account(true);
        report_error(
            state,
            ui,
            "Open Account settings to verify or reconnect YouTube Music".into(),
        );
        return false;
    }
    let engine = &state.engine;
    let result = submit_load(
        &mut state.generation,
        &mut state.active_playback,
        |generation| {
            engine
                .as_ref()
                .ok_or_else(|| "Playback runtime unavailable. See setup instructions.".to_string())?
                .send(PlaybackCommand::Load { source, generation })
        },
    );
    if let Err(error) = result {
        report_error(state, ui, error);
        return false;
    }
    state.command_error = None;
    state.radio_resume_when_ready = false;
    refresh(ui, state);
    ui.set_status_text("Loading…".into());
    ui.set_position(0.0);
    ui.set_duration(0.0);
    ui.set_paused(false);
    save(&state.settings);
    true
}

fn is_youtube_source(source: &str) -> bool {
    let Some((scheme, rest)) = source.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com")
}

fn select_and_play(state: &mut State, ui: &AppWindow, index: usize) {
    let previous = state.settings.queue.current;
    if state.settings.queue.select(index).is_some() && !play(state, ui) {
        state.settings.queue.current = previous;
        refresh(ui, state);
    }
}

fn add(state: &mut State, ui: &AppWindow, source: String) -> bool {
    let source = match tide_playback::validate_playback_source(&source) {
        Ok(source) => source,
        Err(error) => {
            report_error(state, ui, error);
            return false;
        }
    };
    let title = if source.starts_with("http://") || source.starts_with("https://") {
        "Linked track".into()
    } else {
        Path::new(&source)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Local track")
            .into()
    };
    start_tracks(
        state,
        ui,
        vec![Track {
            artwork_url: None,
            recommended: false,
            title,
            artist: String::new(),
            album: String::new(),
            source,
        }],
        0,
        false,
    )
}
#[cfg(windows)]
fn executable(name: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os("TIDE_RUNTIME_DIR") {
        let p = PathBuf::from(dir).join(name);
        if p.is_file() {
            return p;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("tools").join(name);
            if p.is_file() {
                return p;
            }
        }
    }
    let p = PathBuf::from(".tools/runtime").join(name);
    if p.is_file() {
        return p;
    }
    name.into()
}
#[cfg(not(windows))]
fn search(_query: String, _cancel: Arc<AtomicBool>) -> Result<Vec<Track>, String> {
    Err("Contained YouTube search is currently supported on Windows only.".into())
}

#[cfg(windows)]
fn search(query: String, cancel: Arc<AtomicBool>) -> Result<Vec<Track>, String> {
    use std::{
        io::Read,
        process::{Command, Stdio},
    };
    let exe = executable("yt-dlp.exe");
    let mut cmd = Command::new(&exe);
    cmd.args([
        "--ignore-config",
        "--flat-playlist",
        "--dump-single-json",
        "--skip-download",
        "--no-warnings",
    ])
    .arg(format!("ytsearch8:{query}"))
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    if let Some(dir) = exe.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(old) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&old));
        }
        if let Ok(path) = std::env::join_paths(paths) {
            cmd.env("PATH", path);
        }
    }
    let mut child = tide_playback::OwnedProcess::spawn(&mut cmd).map_err(|_| {
        "Could not start contained search. Check yt-dlp in the runtime tools directory.".to_string()
    })?;
    let stdout = child.take_stdout().ok_or("Search output unavailable")?;
    let stderr = child
        .take_stderr()
        .ok_or("Search diagnostics unavailable")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(64 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None)
                if began.elapsed() < Duration::from_secs(45) && !cancel.load(Ordering::Acquire) =>
            {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                child
                    .terminate_tree()
                    .map_err(|error| format!("Could not stop the search process tree: {error}"))?;
                let _ = out.join();
                let _ = err.join();
                return Err(
                    "Search cancelled or timed out. Try again or paste a track link.".into(),
                );
            }
        }
    };
    // A launcher may exit while a descendant still owns a pipe writer. Close
    // the complete tree before joining readers on successful exits as well.
    child
        .terminate_tree()
        .map_err(|error| format!("Could not close the search process tree: {error}"))?;
    let bytes = out.join().map_err(|_| "Search output failed")?;
    let _ = err.join();
    if !status.success() {
        return Err("YouTube search failed. Check the connection and update yt-dlp.".into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Unsupported search response")?;
    let entries = json["entries"].as_array().ok_or("No search results")?;
    Ok(entries
        .iter()
        .take(8)
        .filter_map(|e| {
            let id = e["id"].as_str()?;
            if id.len() != 11
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                return None;
            }
            Some(Track {
                artwork_url: None,
                recommended: false,
                title: e["title"].as_str().unwrap_or("Untitled").into(),
                artist: e["uploader"].as_str().unwrap_or("").into(),
                album: String::new(),
                source: format!("https://www.youtube.com/watch?v={id}"),
            })
        })
        .collect())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--smoke-ui" || arg.starts_with("--smoke-early-skip")) {
        let panic_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            panic_hook(info);
            // A failed assertion in a native event callback must fail the smoke
            // process, rather than leave it waiting inside the event loop.
            std::process::exit(1);
        }));
    }
    let ui = AppWindow::new()?;
    let mut settings: Settings = std::fs::read(settings_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let smoke_autoplay_depletion = std::env::args().any(|a| a == "--smoke-autoplay-depletion");
    let smoke_autoplay =
        smoke_autoplay_depletion || std::env::args().any(|a| a == "--smoke-autoplay");
    let local_listening_source = std::env::args()
        .skip_while(|a| a != "--smoke-local-listening")
        .nth(2);
    if let Some(source) = &local_listening_source {
        let source = tide_playback::validate_playback_source(source)?;
        if !Path::new(&source).is_file() {
            return Err("Local listening verification requires an existing audio file".into());
        }
    }
    let listening_seconds = std::env::args()
        .skip_while(|a| a != "--smoke-listening-session" && a != "--smoke-local-listening")
        .nth(1)
        .map(|s| s.parse::<u64>())
        .transpose()?
        // Allow an overnight continuity run without restarting the player each
        // hour. This limit applies only to the opt-in muted verification mode.
        .map(|seconds| seconds.clamp(300, 21_600));
    let smoke_skip_session = std::env::args().any(|a| a == "--smoke-early-skip-session");
    let smoke_early_skip =
        smoke_skip_session || std::env::args().any(|a| a == "--smoke-early-skip");
    if smoke_autoplay
        || listening_seconds.is_some()
        || smoke_early_skip
        || std::env::args().any(|a| a == "--smoke-ui" || a == "--benchmark-mode")
    {
        settings = Settings {
            transient: true,
            volume: (smoke_autoplay || smoke_early_skip || listening_seconds.is_some())
                .then_some(0.0),
            ..Settings::default()
        };
    }
    settings.queue.tracks.truncate(2000);
    if settings
        .queue
        .current
        .is_some_and(|i| i >= settings.queue.tracks.len())
    {
        settings.queue.current = None;
    }
    if sanitize_queue(&mut settings.queue) {
        save(&settings);
    }
    let test_mode = (smoke_early_skip && !smoke_skip_session)
        || local_listening_source.is_some()
        || std::env::args().any(|a| a == "--smoke-ui" || a == "--benchmark-mode");
    let provider = if test_mode {
        None
    } else {
        tide_provider::Provider::new().ok()
    };
    let lease_result = provider
        .as_ref()
        .map(|p| p.cookie_lease())
        .transpose()
        .map(|value| value.flatten());
    let (cookie_lease, credential_error) = match lease_result {
        Ok(lease) => (lease, None),
        Err(error) => (None, Some(error)),
    };
    let (engine, error) = match credential_error.map_or_else(
        || {
            tide_playback::start_with_account(
                cookie_lease.as_ref().map(|l| l.path().to_path_buf()),
                provider
                    .as_ref()
                    .map(|provider| provider.account_index())
                    .unwrap_or(0),
            )
        },
        Err,
    ) {
        Ok(e) => (Some(e), None),
        Err(e) => (None, Some(e)),
    };
    let state = Rc::new(RefCell::new(State {
        settings,
        engine,
        generation: 0,
        active_playback: false,
        command_error: None,
        search_generation: 0,
        search_cancel: Arc::new(AtomicBool::new(false)),
        search_worker: None,
        provider,
        cookie_lease,
        pending_cookie_updates: std::collections::VecDeque::new(),
        cookie_update_retry_at: None,
        cookie_updates_applied: 0,
        cookie_updates_not_applied: 0,
        library_worker: None,
        library_cancel: Arc::new(AtomicBool::new(false)),
        library_playlists: Vec::new(),
        recommendations: Vec::new(),
        detail_tracks: Vec::new(),
        search_tracks: Vec::new(),
        account_error: false,
        account_verified: false,
        library_action: LibraryAction::default(),
        library_request: None,
        library_retry: None,
        shuffle_request: false,
        radio_requests: radio::RadioRequests::default(),
        radio_resume_when_ready: false,
        volume_save_pending: false,
        artwork: RefCell::new(if test_mode {
            artwork::ArtworkCache::new_disabled()
        } else {
            artwork::ArtworkCache::new()
        }),
    }));
    ui.set_account_connected(
        state
            .borrow()
            .provider
            .as_ref()
            .is_some_and(|p| p.is_connected()),
    );
    ui.set_account_status(
        if ui.get_account_connected() {
            "Saved session · refresh to verify the account"
        } else {
            "Connect your YouTube Music account locally"
        }
        .into(),
    );
    ui.set_light_mode(state.borrow().settings.light_mode);
    ui.set_compact_mode(state.borrow().settings.compact_mode);
    ui.set_autoplay_enabled(state.borrow().settings.autoplay);
    ui.set_volume(state.borrow().settings.volume.unwrap_or(70.0) as f32);
    ui.set_repeat_enabled(state.borrow().settings.repeat);
    ui.set_paused(true);
    refresh(&ui, &state.borrow());
    ui.set_status_text(
        error
            .unwrap_or_else(|| "Ready · paste a link, search, or open a file".into())
            .into(),
    );
    send(
        &mut state.borrow_mut(),
        &ui,
        PlaybackCommand::SetVolume(ui.get_volume() as f64),
    );
    let picker_busy = Rc::new(PickerState::default());
    let (tx, rx) = mpsc::sync_channel(2);
    let (library_tx, library_rx) = mpsc::sync_channel::<LibraryResult>(2);
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_play_radio(move || {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                s.settings.autoplay = true;
                u.set_autoplay_enabled(true);
                save(&s.settings);
                request_radio(&mut s, &u, &tx, true);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_shuffle_playlist(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if s.library_worker.is_some() {
                    return;
                }
                if let Some(playlist) = usize::try_from(index)
                    .ok()
                    .and_then(|index| s.library_playlists.get(index))
                    .cloned()
                {
                    s.shuffle_request = true;
                    request_library(
                        &mut s,
                        &u,
                        &tx,
                        tide_provider::Request::Playlist { id: playlist.id },
                    );
                }
            }
        });
    }
    ui.set_account_index(
        state
            .borrow()
            .provider
            .as_ref()
            .map(|provider| provider.account_index() as i32)
            .unwrap_or(0),
    );
    let startup_account = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "--account-index")
        .and_then(|pair| pair[1].parse::<u32>().ok())
        .filter(|index| *index < 10);
    if ui.get_account_connected()
        && !std::env::args().any(|a| a == "--connect-account")
        && startup_account.is_none()
    {
        request_library(
            &mut state.borrow_mut(),
            &ui,
            &library_tx,
            tide_provider::Request::Library,
        );
    }
    if let Some(index) = startup_account.filter(|_| !test_mode) {
        ui.set_settings_open(true);
        request_library(
            &mut state.borrow_mut(),
            &ui,
            &library_tx,
            tide_provider::Request::SwitchAccount { index },
        );
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        let picker_busy = picker_busy.clone();
        ui.on_switch_account(move |index| {
            if let Some(u) = weak.upgrade() {
                if (0..10).contains(&index) {
                    picker_busy.cancel_account();
                    request_library(
                        &mut state.borrow_mut(),
                        &u,
                        &tx,
                        tide_provider::Request::SwitchAccount {
                            index: index as u32,
                        },
                    );
                }
            }
        });
    }
    if std::env::args().any(|a| a == "--connect-account") && !test_mode {
        ui.set_settings_open(true);
        connect_picker(&picker_busy, state.clone(), &ui, library_tx.clone());
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        let picker_busy = picker_busy.clone();
        ui.on_connect_account(move || {
            if let Some(u) = weak.upgrade() {
                connect_picker(&picker_busy, state.clone(), &u, tx.clone());
            }
        });
    }
    {
        let state = state.clone();
        let picker_busy = picker_busy.clone();
        ui.on_cancel_account(move || {
            picker_busy.cancel_account();
            state.borrow().library_cancel.store(true, Ordering::Release);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        let picker_busy = picker_busy.clone();
        ui.on_import_account(move || {
            let state = Rc::downgrade(&state);
            let revision = picker_busy.account_revision.get();
            let selection = picker_busy.clone();
            let weak = weak.clone();
            let tx = tx.clone();
            let error_ui = weak.upgrade();
            let result = spawn_picker(&picker_busy, async move {
                let file = rfd::AsyncFileDialog::new()
                    .set_title("Import a local YouTube Music session")
                    .add_filter(
                        "Browser request headers / authentication JSON",
                        &["txt", "json"],
                    )
                    .pick_file()
                    .await;
                if selection.account_revision.get() != revision {
                    return;
                }
                let (Some(file), Some(u), Some(state)) = (file, weak.upgrade(), state.upgrade())
                else {
                    return;
                };
                let mut s = state.borrow_mut();
                if s.library_worker.is_some() {
                    return;
                }
                let result = s
                    .provider
                    .as_ref()
                    .ok_or("Secure account storage is unavailable".to_string())
                    .and_then(|p| p.import_session(file.path()));
                match result {
                    Ok(()) => {
                        s.library_playlists.clear();
                        s.recommendations.clear();
                        refresh_library_models(&s, &u);
                        u.set_account_connected(true);
                        u.set_account_name("Session imported".into());
                        if configure_account_playback(&mut s, &u) {
                            request_library(&mut s, &u, &tx, tide_provider::Request::Library);
                        } else {
                            u.set_account_status(
                                "Session saved, but signed-in playback could not start".into(),
                            );
                        }
                    }
                    Err(error) => u.set_account_status(error.into()),
                }
            });
            if let (Err(error), Some(u)) = (result, error_ui) {
                u.set_account_status(error.into());
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_refresh_library(move || {
            if let Some(u) = weak.upgrade() {
                request_library(
                    &mut state.borrow_mut(),
                    &u,
                    &tx,
                    tide_provider::Request::Library,
                );
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_refresh_recommendations(move || {
            if let Some(u) = weak.upgrade() {
                request_library(
                    &mut state.borrow_mut(),
                    &u,
                    &tx,
                    tide_provider::Request::Home,
                );
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_play_liked(move || {
            if let Some(u) = weak.upgrade() {
                request_library(
                    &mut state.borrow_mut(),
                    &u,
                    &tx,
                    tide_provider::Request::Liked,
                );
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_select_library_playlist(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(p) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.library_playlists.get(i))
                    .cloned()
                {
                    request_library(
                        &mut s,
                        &u,
                        &tx,
                        tide_provider::Request::Playlist { id: p.id },
                    );
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_select_recommendation(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(card) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.recommendations.get(i))
                    .cloned()
                {
                    if card.kind == "song"
                        && card.id.len() == 11
                        && card
                            .id
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                    {
                        start_tracks(&mut s, &u, vec![recommendation_track(&card)], 0, false);
                    } else if card.kind == "playlist" {
                        request_library(
                            &mut s,
                            &u,
                            &tx,
                            tide_provider::Request::Playlist { id: card.id },
                        );
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let picker_busy = picker_busy.clone();
        ui.on_disconnect_account(move || {
            picker_busy.cancel_account();
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if s.library_worker.is_some() {
                    return;
                }
                let result = s
                    .provider
                    .as_ref()
                    .map(|p| p.disconnect())
                    .unwrap_or(Ok(()));
                match result {
                    Ok(()) => {
                        configure_account_playback(&mut s, &u);
                        s.library_playlists.clear();
                        s.recommendations.clear();
                        refresh_library_models(&s, &u);
                        u.set_account_connected(false);
                        u.set_account_name("".into());
                        u.set_account_status("Disconnected · session removed from Tide".into());
                    }
                    Err(error) => u.set_account_status(error.into()),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_play_detail(move || {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let tracks = s.detail_tracks.clone();
                start_tracks(&mut s, &u, tracks, 0, false);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_shuffle_detail(move || {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let tracks = s.detail_tracks.clone();
                start_tracks(&mut s, &u, tracks, 0, true);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_detail_track(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let tracks = s.detail_tracks.clone();
                if let Ok(index) = usize::try_from(index) {
                    start_tracks(&mut s, &u, tracks, index, false);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_search_track(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(track) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.search_tracks.get(i))
                    .cloned()
                {
                    start_tracks(&mut s, &u, vec![track], 0, false);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_queue_search_track(move |index, next| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(t) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.search_tracks.get(i))
                    .cloned()
                {
                    queue_choice(&mut s, &u, t, next);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_queue_detail_track(move |index, next| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(t) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.detail_tracks.get(i))
                    .cloned()
                {
                    queue_choice(&mut s, &u, t, next);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_requeue_track(move |index, next| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(t) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.settings.queue.tracks.get(i))
                    .cloned()
                {
                    if usize::try_from(index)
                        .ok()
                        .is_some_and(|i| s.settings.queue.current.is_none_or(|current| i > current))
                    {
                        s.settings.queue.remove(index as usize);
                    }
                    queue_choice(&mut s, &u, t, next);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_queue_recommendation(move |index, next| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Some(c) = usize::try_from(index)
                    .ok()
                    .and_then(|i| s.recommendations.get(i))
                    .filter(|c| c.kind == "song")
                    .cloned()
                {
                    queue_choice(&mut s, &u, recommendation_track(&c), next);
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_detail(move || {
            if let Some(u) = weak.upgrade() {
                u.set_detail_open(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_search(move || {
            if let Some(u) = weak.upgrade() {
                u.set_search_open(false);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_retry_error(move || {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if s.account_error {
                    u.set_settings_open(true);
                } else {
                    if s.library_worker.is_some() && s.library_retry.is_some() {
                        return;
                    }
                    u.set_error_message("".into());
                    if !u.get_error_retryable() {
                        s.command_error = None;
                        return;
                    }
                    if s.engine
                        .as_ref()
                        .is_some_and(|e| e.snapshot().error.is_some())
                    {
                        play(&mut s, &u);
                    } else if let Some((request, shuffle)) = s.library_retry.take() {
                        s.shuffle_request = shuffle;
                        request_library(&mut s, &u, &tx, request);
                    } else if matches!(s.library_action, LibraryAction::Radio { .. })
                        && s.account_verified
                    {
                        s.radio_requests.reset();
                        s.radio_resume_when_ready = !s.active_playback && s.settings.autoplay;
                        request_radio(&mut s, &u, &tx, false);
                    } else {
                        play(&mut s, &u);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_start_track_radio(move |context, index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let index = usize::try_from(index).ok();
                let track = index.and_then(|i| match context {
                    0 => s.settings.queue.tracks.get(i).cloned(),
                    1 => s.detail_tracks.get(i).cloned(),
                    2 => s.search_tracks.get(i).cloned(),
                    3 => s
                        .recommendations
                        .get(i)
                        .filter(|c| c.kind == "song")
                        .map(recommendation_track),
                    _ => None,
                });
                if let Some(track) = track {
                    if s.library_worker.is_some() {
                        u.set_status_text(
                            "Wait for the current music request, then try radio again".into(),
                        );
                        return;
                    }
                    if start_tracks(&mut s, &u, vec![track], 0, false) {
                        s.settings.autoplay = true;
                        u.set_autoplay_enabled(true);
                        save(&s.settings);
                        request_radio(&mut s, &u, &tx, true);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_reorder_queue_track(move |index, target| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let (Ok(index), Ok(target)) = (usize::try_from(index), usize::try_from(target)) {
                    if s.settings.queue.move_upcoming_to(index, target) {
                        refresh(&u, &s);
                        save(&s.settings);
                    }
                }
            }
        });
    }
    ui.on_open_account_help(|| {
        let zen = std::env::var_os("TIDE_ZEN_EXE").map(PathBuf::from).or_else(|| std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Zen Browser/zen.exe"))).unwrap_or_else(|| PathBuf::from("zen.exe"));
        let mut command=std::process::Command::new(zen);
        command.arg("-new-tab").arg("https://github.com/yotam1001/tide-music/blob/codex/account-library-sync/docs/ACCOUNT_SETUP.md");
        #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let _=command.spawn();
    });
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_add_source(move |input| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let input = input.trim();
            if input.is_empty() {
                return;
            }
            if input.len() > 2048 {
                ui.set_status_text("Link or query too long".into());
                return;
            }
            let mut state = state.borrow_mut();
            if input.starts_with("http://")
                || input.starts_with("https://")
                || Path::new(input).is_file()
            {
                if add(&mut state, &ui, input.into()) {
                    ui.set_source_input("".into());
                }
            } else if input.contains("://") {
                ui.set_status_text("Only http/https links and local files are supported".into());
            } else {
                if state.search_worker.is_some() {
                    ui.set_status_text("A search is already running. Please wait.".into());
                    return;
                }
                state.search_generation += 1;
                let generation = state.search_generation;
                let query = input.to_string();
                let tx = tx.clone();
                state.search_cancel.store(false, Ordering::Release);
                let cancel = state.search_cancel.clone();
                ui.set_search_open(true);
                ui.set_detail_open(false);
                ui.set_search_busy(true);
                state.search_tracks.clear();
                ui.set_search_rows(ModelRc::new(VecModel::default()));
                ui.set_error_message("".into());
                ui.set_status_text("Searching YouTube…".into());
                state.search_worker = Some(std::thread::spawn(move || {
                    let _ = tx.try_send((generation, search(query, cancel)));
                }));
            }
        });
    }
    // All callbacks work on one queue and one playback engine, regardless of view.
    macro_rules! bind {
        ($method:ident, |$s:ident, $u:ident| $body:block) => {{
            let state = state.clone();
            let weak = ui.as_weak();
            ui.$method(move || {
                if let Some($u) = weak.upgrade() {
                    let mut $s = state.borrow_mut();
                    $body
                }
            });
        }};
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let picker_busy = picker_busy.clone();
        ui.on_open_file(move || {
            let state = Rc::downgrade(&state);
            let weak = weak.clone();
            let error_ui = weak.upgrade();
            let result = spawn_picker(&picker_busy, async move {
                if let Some(file) = rfd::AsyncFileDialog::new()
                    .set_title("Open audio file in Tide")
                    .add_filter(
                        "Audio",
                        &["mp3", "m4a", "ogg", "opus", "flac", "wav", "aac"],
                    )
                    .pick_file()
                    .await
                {
                    if let (Some(u), Some(state)) = (weak.upgrade(), state.upgrade()) {
                        add(
                            &mut state.borrow_mut(),
                            &u,
                            file.path().to_string_lossy().into_owned(),
                        );
                    }
                }
            });
            if let (Err(error), Some(u)) = (result, error_ui) {
                u.set_status_text(error.into());
            }
        });
    }
    bind!(on_toggle_pause, |s, u| {
        if !s.active_playback && s.settings.queue.current.is_some() {
            play(&mut s, &u);
        } else {
            send(&mut s, &u, PlaybackCommand::TogglePause);
        }
    });
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_next_track(move || {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let repeat = s.settings.repeat;
                let previous = s.settings.queue.current;
                if s.settings.queue.next_track(repeat).is_some() {
                    if !play(&mut s, &u) {
                        s.settings.queue.current = previous;
                        refresh(&u, &s);
                    }
                } else if s.settings.autoplay && s.account_verified {
                    send(&mut s, &u, PlaybackCommand::Stop);
                    s.active_playback = false;
                    s.radio_resume_when_ready = true;
                    s.radio_requests.reset();
                    request_radio(&mut s, &u, &tx, false);
                    u.set_paused(true);
                    u.set_status_text("Finding the next recommended song...".into());
                }
            }
        });
    }
    bind!(on_previous_track, |s, u| {
        let previous = s.settings.queue.current;
        if s.settings.queue.previous_track().is_some() && !play(&mut s, &u) {
            s.settings.queue.current = previous;
            refresh(&u, &s);
        }
    });
    bind!(on_shuffle_queue, |s, u| {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |t| t.as_nanos() as u64);
        s.settings.queue.shuffle_upcoming(seed);
        refresh(&u, &s);
        save(&s.settings);
        u.set_status_text("Upcoming tracks shuffled".into());
    });
    bind!(on_toggle_repeat, |s, u| {
        s.settings.repeat = !s.settings.repeat;
        u.set_repeat_enabled(s.settings.repeat);
        save(&s.settings);
    });
    bind!(on_switch_mode, |s, u| {
        s.settings.light_mode = !u.get_light_mode();
        u.set_light_mode(s.settings.light_mode);
        save(&s.settings);
    });
    bind!(on_toggle_compact, |s, u| {
        s.settings.compact_mode = !u.get_compact_mode();
        u.set_compact_mode(s.settings.compact_mode);
        if !u.get_collapsed() {
            u.window().set_size(if s.settings.compact_mode {
                slint::LogicalSize::new(780.0, 720.0)
            } else {
                slint::LogicalSize::new(1140.0, 800.0)
            });
        }
        save(&s.settings);
    });
    bind!(on_toggle_autoplay, |s, u| {
        s.settings.autoplay = !s.settings.autoplay;
        u.set_autoplay_enabled(s.settings.autoplay);
        if s.settings.autoplay {
            s.radio_requests.reset();
        } else {
            s.radio_resume_when_ready = false;
            s.settings.queue.remove_autoplay_upcoming();
            refresh(&u, &s);
        }
        save(&s.settings);
    });
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_remove_queue_track(move |index| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let Ok(index) = usize::try_from(index) else {
                    return;
                };
                let was_current = s.settings.queue.current == Some(index);
                if s.settings.queue.remove(index).is_none() {
                    return;
                }
                if s.settings.queue.current.is_some_and(|current| {
                    s.settings.queue.tracks.len().saturating_sub(current + 1) <= 2
                }) {
                    let generation = s.generation;
                    s.radio_requests.queue_depleted(generation);
                }
                if was_current {
                    s.radio_resume_when_ready = false;
                }
                if was_current && s.active_playback {
                    if s.settings.queue.tracks.get(index).is_some() {
                        play(&mut s, &u);
                    } else {
                        send(&mut s, &u, PlaybackCommand::Stop);
                        s.active_playback = false;
                        u.set_paused(true);
                        u.set_position(0.0);
                        u.set_duration(0.0);
                    }
                }
                refresh(&u, &s);
                save(&s.settings);
            }
        });
    }
    bind!(on_clear_queue, |s, u| {
        send(&mut s, &u, PlaybackCommand::Stop);
        s.settings.queue = Queue::default();
        s.active_playback = false;
        s.radio_resume_when_ready = false;
        refresh(&u, &s);
        u.set_position(0.0);
        u.set_duration(0.0);
        u.set_paused(true);
        u.set_status_text("Queue cleared".into());
        save(&s.settings);
    });
    {
        let weak = ui.as_weak();
        let restored_size = Rc::new(RefCell::new(slint::LogicalSize::new(1140.0, 800.0)));
        ui.on_collapse_player(move || {
            if let Some(u) = weak.upgrade() {
                let c = !u.get_collapsed();
                if c {
                    *restored_size.borrow_mut() =
                        u.window().size().to_logical(u.window().scale_factor());
                }
                u.set_collapsed(c);
                u.window().set_size(if c {
                    slint::LogicalSize::new(700.0, 132.0)
                } else {
                    let size = *restored_size.borrow();
                    slint::LogicalSize::new(size.width.max(650.0), size.height.max(420.0))
                });
                u.window().request_redraw();
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_track(move |i| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if i >= 0 {
                    select_and_play(&mut s, &u, i as usize);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_seek(move |p| {
            if let Some(u) = weak.upgrade() {
                send(&mut state.borrow_mut(), &u, PlaybackCommand::Seek(p as f64));
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_media_command(move |action, value| {
            let Some(u) = weak.upgrade() else {
                return;
            };
            // Hardware transport controls follow exactly the same path as the
            // player buttons, including autoplay when Next reaches queue end.
            match action {
                2 => {
                    u.invoke_toggle_pause();
                    return;
                }
                3 => {
                    u.invoke_next_track();
                    return;
                }
                4 => {
                    u.invoke_previous_track();
                    return;
                }
                _ => {}
            }
            let mut s = state.borrow_mut();
            match action {
                0 => {
                    if !s.active_playback && s.settings.queue.current.is_some() {
                        play(&mut s, &u);
                    } else {
                        send(&mut s, &u, PlaybackCommand::SetPaused(false));
                    }
                }
                1 => {
                    send(&mut s, &u, PlaybackCommand::SetPaused(true));
                }
                5 => {
                    if send(&mut s, &u, PlaybackCommand::Stop) {
                        s.active_playback = false;
                        s.radio_resume_when_ready = false;
                        u.set_paused(true);
                        u.set_position(0.0);
                        u.set_duration(0.0);
                        u.set_status_text("Stopped".into());
                    }
                }
                6 => {
                    send(&mut s, &u, PlaybackCommand::Seek(value as f64));
                }
                _ => {}
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_change_volume(move |v| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                let v = if v.is_finite() {
                    v.clamp(0.0, 100.0)
                } else {
                    return;
                };
                u.set_volume(v);
                s.settings.volume = Some(v as f64);
                send(&mut s, &u, PlaybackCommand::SetVolume(v as f64));
                s.volume_save_pending = true;
            }
        });
    }
    ui.show()?;
    let media = Rc::new(RefCell::new(None::<tide_windows::MediaSession>));
    let media_enabled =
        cfg!(windows) && !test_mode && listening_seconds.is_none() && !smoke_autoplay;
    // Prepare as soon as current playback is loaded, rather than waiting until
    // EOF approaches. Keep the candidate through Pause so an early skip can
    // use it. The resolver deduplicates requests and retains only one stream.
    let preparation_timer = slint::Timer::default();
    if state.borrow().engine.is_some() {
        let state = state.clone();
        preparation_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                let mut s = state.borrow_mut();
                sync_playback_cookies(&mut s);
                let Some(engine) = &s.engine else {
                    return;
                };
                let p = engine.snapshot();
                let eligible = preparation_eligible(s.active_playback, s.generation, &p);
                let _ = engine.prepare_next(if eligible {
                    upcoming_source(&s.settings.queue, s.settings.repeat)
                } else {
                    None
                });
            },
        );
    }
    // Publish confirmed playback state promptly so Windows chooses the correct
    // Play/Pause action even when the user presses a media key twice quickly.
    // Metadata and timeline writes are cached inside MediaSession.
    let window_fit_timer = slint::Timer::default();
    #[cfg(windows)]
    if !test_mode && listening_seconds.is_none() && !smoke_autoplay_depletion {
        let weak = ui.as_weak();
        let mut fitted = None;
        window_fit_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                let Some(u) = weak.upgrade() else {
                    return;
                };
                let window = u.window().window_handle();
                let Ok(handle) = window.window_handle() else {
                    return;
                };
                let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                    return;
                };
                let hwnd = handle.hwnd.get() as *mut std::ffi::c_void;
                let Ok(work) = tide_windows::window_work_area(hwnd) else {
                    fitted = None;
                    return;
                };
                let mode = (
                    u.window().scale_factor(),
                    u.get_compact_mode(),
                    u.get_collapsed(),
                    work,
                );
                if fitted == Some(mode) {
                    return;
                }
                if tide_windows::fit_window_to_work_area(hwnd, work).is_ok() {
                    fitted = Some(mode);
                }
            },
        );
    }
    let media_timer = slint::Timer::default();
    if media_enabled {
        let state = state.clone();
        let weak = ui.as_weak();
        let media = media.clone();
        let mut registration_attempts = 0;
        media_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                let Some(u) = weak.upgrade() else {
                    return;
                };
                // Winit creates the native window after the event loop starts.
                // ui.show() alone does not make its handle available at startup.
                if media.borrow().is_none() && registration_attempts < 20 {
                    registration_attempts += 1;
                    match start_window_media(&u) {
                        Ok(session) => *media.borrow_mut() = Some(session),
                        Err(error) if registration_attempts == 20 => {
                            u.set_status_text(format!("Media controls unavailable: {error}").into())
                        }
                        Err(_) => {}
                    }
                }
                let s = state.borrow();
                let snapshot = s
                    .engine
                    .as_ref()
                    .map(|engine| engine.snapshot())
                    .filter(|p| s.active_playback && p.generation == s.generation);
                let (playing, position, duration) = if let Some(p) = snapshot {
                    u.set_paused(p.paused);
                    (!p.paused && !p.ended, p.position, p.duration)
                } else {
                    (false, u.get_position() as f64, u.get_duration() as f64)
                };
                if let Some(session) = media.borrow_mut().as_mut() {
                    let _ = session.update(
                        &u.get_track_title(),
                        &u.get_track_artist(),
                        &u.get_track_album(),
                        playing,
                        position,
                        duration,
                    );
                }
            },
        );
    }
    let timer = slint::Timer::default();
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let library_tx = library_tx.clone();
        let mut last_content_view = None;
        let mut last_volume_save = Instant::now();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                let Some(u) = weak.upgrade() else {
                    return;
                };
                let mut s = state.borrow_mut();
                let content_view = (
                    u.get_library_open(),
                    u.get_detail_open(),
                    u.get_search_open(),
                );
                if s.volume_save_pending && last_volume_save.elapsed() >= Duration::from_secs(1) {
                    s.volume_save_pending = false;
                    save(&s.settings);
                    last_volume_save = Instant::now();
                }
                if last_content_view != Some(content_view) {
                    refresh_library_models(&s, &u);
                    refresh_content_models(&s, &u);
                    last_content_view = Some(content_view);
                }
                while let Ok(result) = library_rx.try_recv() {
                    if let Some(worker) = s.library_worker.take() {
                        let _ = worker.join();
                    }
                    u.set_account_busy(false);
                    u.set_detail_busy(false);
                    let playback_session_updated = sync_playback_cookies(&mut s);
                    if matches!(
                        &result,
                        Ok(tide_provider::Response::Library { .. }
                            | tide_provider::Response::Tracks { .. }
                            | tide_provider::Response::Home { .. })
                    ) {
                        if !matches!(s.library_action, LibraryAction::Radio { .. })
                            && s.library_retry.take().is_some()
                            && !s.engine.as_ref().is_some_and(|engine| engine.snapshot().error.is_some())
                        {
                            u.set_error_message("".into());
                            u.set_error_retryable(false);
                        }
                        if let (Some(provider), Some(lease)) = (&s.provider, &s.cookie_lease) {
                            if provider.refresh_cookie_lease(lease).is_err() {
                                u.set_account_status("Account data loaded; playback session update will be retried on the next refresh.".into());
                            }
                        }
                    }
                    match result {
                        Ok(tide_provider::Response::Connected) => {
                            u.set_account_index(
                                s.provider
                                    .as_ref()
                                    .map(|provider| provider.account_index() as i32)
                                    .unwrap_or(0),
                            );
                            s.library_playlists.clear();
                            s.recommendations.clear();
                            refresh_library_models(&s, &u);
                            u.set_account_connected(true);
                            u.set_account_name("Verifying account…".into());
                            if configure_account_playback(&mut s, &u) {
                                request_library(
                                    &mut s,
                                    &u,
                                    &library_tx,
                                    tide_provider::Request::Library,
                                );
                            } else {
                                u.set_account_status(
                                    "Session saved, but playback setup failed".into(),
                                );
                            }
                        }
                        Ok(tide_provider::Response::Library {
                            account,
                            playlists,
                            truncated,
                        }) => {
                            s.account_verified = true;
                            u.set_account_verified(true);
                            s.account_error = false;
                            u.set_error_is_account(false);
                            u.set_error_message("".into());
                            s.library_playlists = playlists.into_iter().take(200).collect();
                            u.set_account_name(account.into());
                            refresh_library_models(&s, &u);
                            u.set_account_status(
                                if truncated {
                                    "Signed-in account verified · showing the first 200 playlists"
                                } else {
                                    "Signed-in account verified · playlists refreshed"
                                }
                                .into(),
                            );
                            s.command_error = None;
                            request_library(&mut s, &u, &library_tx, tide_provider::Request::Home);
                        }
                        Ok(tide_provider::Response::Home { cards }) => {
                            s.recommendations = cards.into_iter().take(120).collect();
                            refresh_library_models(&s, &u);
                            u.set_account_status(
                                "Your YouTube Music recommendations are refreshed".into(),
                            );
                        }
                        Ok(tide_provider::Response::Tracks { tracks, truncated }) => {
                            let tracks: Vec<Track> = tracks
                                .into_iter()
                                .take(2000)
                                .filter(|t| {
                                    t.source
                                        .strip_prefix("https://www.youtube.com/watch?v=")
                                        .is_some_and(|id| {
                                            id.len() == 11
                                                && id.bytes().all(|c| {
                                                    c.is_ascii_alphanumeric()
                                                        || c == b'-'
                                                        || c == b'_'
                                                })
                                        })
                                })
                                .collect();
                            if let LibraryAction::Radio {
                                seed,
                                generation,
                                replace_upcoming,
                            } = s.library_action.clone()
                            {
                                if generation != s.generation
                                    || current_video_id(&s).as_deref() != Some(seed.as_str())
                                    || (!s.settings.autoplay && !replace_upcoming)
                                {
                                    continue;
                                }
                                if tracks.is_empty() {
                                    library_failure(&mut s, &u, false);
                                    continue;
                                }
                                if replace_upcoming {
                                    let keep =
                                        s.settings.queue.current.map_or(0, |index| index + 1);
                                    s.settings.queue.tracks.truncate(keep);
                                }
                                s.settings.queue.trim_played_history(20);
                                let old_len = s.settings.queue.tracks.len();
                                let added = s.settings.queue.append_deduplicated(
                                    tracks.into_iter().map(|mut t| {
                                        t.recommended = true;
                                        t
                                    }),
                                );
                                if added == 0 {
                                    library_failure(&mut s, &u, false);
                                    continue;
                                }
                                s.radio_requests.succeeded();
                                if !s.active_playback && s.radio_resume_when_ready {
                                    s.settings.queue.select(old_len);
                                    play(&mut s, &u);
                                }
                                s.radio_resume_when_ready = false;
                                refresh(&u, &s);
                                save(&s.settings);
                                u.set_account_status(
                                    format!("Added {added} recommended songs to the queue").into(),
                                );
                                continue;
                            }
                            if matches!(s.library_action, LibraryAction::Preview { .. }) {
                                s.detail_tracks = tracks;
                                u.set_detail_busy(false);
                                refresh_content_models(&s, &u);
                                u.set_account_status(
                                    if truncated {
                                        "Showing the first 2,000 playlist tracks"
                                    } else {
                                        "Playlist ready"
                                    }
                                    .into(),
                                );
                                continue;
                            }
                            if tracks.is_empty() {
                                u.set_account_status(
                                    "No playable tracks were returned for this playlist".into(),
                                );
                            } else {
                                let previous = s.settings.queue.clone();
                                s.settings.queue = Queue {
                                    tracks,
                                    current: Some(0),
                                };
                                if matches!(s.library_action, LibraryAction::Shuffle) {
                                    let seed = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_nanos()
                                        as u64;
                                    s.settings.queue.shuffle_all(seed);
                                }
                                if play(&mut s, &u) {
                                    u.set_account_status(
                                        if truncated {
                                            "Playing the first 2,000 playlist tracks"
                                        } else {
                                            "Playlist loaded into the player"
                                        }
                                        .into(),
                                    );
                                } else {
                                    s.settings.queue = previous;
                                    refresh(&u, &s);
                                    u.set_account_status(
                                        "Playlist loaded, but playback could not start".into(),
                                    );
                                }
                            }
                        }
                        Ok(tide_provider::Response::Error { code }) => {
                            library_failure(&mut s, &u, code == "session" && !playback_session_updated);
                        }
                        Err(error) if matches!(s.library_action, LibraryAction::Connect) => {
                            if s.account_verified {
                                u.set_account_status(format!("Account connection failed: {error}. Your connected account is unchanged.").into());
                                continue;
                            }
                            s.account_error = true;
                            u.set_error_is_account(true);
                            u.set_error_message(
                                "Account connection failed. Open Account for details.".into(),
                            );
                            u.set_account_status(error.into());
                        }
                        Ok(tide_provider::Response::Session { .. }) | Err(_) => {
                            library_failure(&mut s, &u, false);
                        }
                    }
                }
                let artwork_changed = s.artwork.borrow_mut().poll();
                if artwork_changed {
                    refresh(&u, &s);
                    refresh_library_models(&s, &u);
                    refresh_content_models(&s, &u);
                }
                if s.active_playback || s.radio_resume_when_ready {
                    request_radio(&mut s, &u, &library_tx, false);
                }
                while let Ok((g, result)) = rx.try_recv() {
                    if let Some(worker) = s.search_worker.take() {
                        let _ = worker.join();
                    }
                    if g != s.search_generation {
                        continue;
                    }
                    match result {
                        Ok(tracks) => {
                            let count = tracks.len();
                            s.search_tracks = tracks;
                            u.set_search_busy(false);
                            refresh_content_models(&s, &u);
                            u.set_status_text(format!("{count} results").into());
                        }
                        Err(e) => {
                            u.set_search_busy(false);
                            s.account_error = false;
                            u.set_error_is_account(false);
                            report_error(&mut s, &u, e);
                        }
                    }
                }
                if let Some(engine) = &s.engine {
                    let p = engine.snapshot();
                    if p.generation != s.generation || !s.active_playback {
                        return;
                    }
                    u.set_position(p.position as f32);
                    u.set_duration(p.duration as f32);
                    u.set_paused(p.paused);
                    let failed = p.error.is_some();
                    if let Some(e) = p.error {
                        u.set_status_text(e.into());
                    } else if let Some(error) = &s.command_error {
                        u.set_status_text(error.clone().into());
                    } else if p.loading {
                        u.set_status_text("Loading…".into());
                    } else if s.active_playback {
                        u.set_status_text(if p.paused { "Paused" } else { "Playing" }.into());
                    }
                    if let Some(title) = p.title.filter(|t| !t.is_empty()) {
                        if let Some(i) = s.settings.queue.current {
                            if s.settings.queue.tracks[i].title == "Linked track" {
                                s.settings.queue.tracks[i].title = title;
                                refresh(&u, &s);
                            }
                        }
                    }
                    if p.ended {
                        let repeat = s.settings.repeat;
                        let previous = s.settings.queue.current;
                        if s.settings.queue.next_track(repeat).is_some() {
                            if !play(&mut s, &u) {
                                s.settings.queue.current = previous;
                                s.active_playback = false;
                                refresh(&u, &s);
                            }
                        } else {
                            s.active_playback = false;
                            u.set_paused(true);
                            s.radio_resume_when_ready = s.settings.autoplay
                                && s.account_verified
                                && current_video_id(&s).is_some();
                            request_radio(&mut s, &u, &library_tx, false);
                            if !failed {
                                u.set_status_text(
                                    if s.radio_resume_when_ready && s.account_verified {
                                        "Finding the next recommended song…"
                                    } else {
                                        "Queue finished"
                                    }
                                    .into(),
                                );
                            }
                        }
                    } else if failed {
                        s.active_playback = false;
                        u.set_paused(true);
                        s.account_error = false;
                        u.set_error_is_account(false);
                        u.set_error_retryable(true);
                        u.set_error_message(
                            "Playback interrupted. Check your connection and retry.".into(),
                        );
                    }
                }
            },
        );
    }
    if state.borrow().settings.compact_mode {
        ui.window().set_size(slint::LogicalSize::new(780.0, 720.0));
    }
    let smoke_timer = slint::Timer::default();
    if let Some(mode) = std::env::args()
        .skip_while(|a| a != "--benchmark-mode")
        .nth(1)
    {
        ui.set_light_mode(mode != "full");
        ui.set_compact_mode(mode != "full" && mode != "collapsed");
        ui.set_collapsed(mode == "collapsed");
        let (width, height) = match mode.as_str() {
            "full" => (1200.0, 800.0),
            "collapsed" => (700.0, 132.0),
            _ => (780.0, 720.0),
        };
        ui.window().set_size(slint::LogicalSize::new(width, height));
        if let Some(count) = std::env::args()
            .skip_while(|a| a != "--benchmark-queue-count")
            .nth(1)
            .and_then(|count| count.parse::<usize>().ok())
        {
            let mut s = state.borrow_mut();
            s.settings.queue = Queue {
                tracks: (0..count.min(tide_core::MAX_QUEUE_TRACKS))
                    .map(|index| Track {
                        title: format!("Fixture song {}", index + 1),
                        artist: "Fixture artist".into(),
                        album: "Fixture album".into(),
                        source: String::new(),
                        artwork_url: None,
                        recommended: false,
                    })
                    .collect(),
                current: (count > 0).then_some(0),
            };
            refresh(&ui, &s);
            ui.set_queue_open(true);
        }
        if let Some(source) = std::env::args()
            .skip_while(|a| a != "--benchmark-source")
            .nth(1)
        {
            let mut s = state.borrow_mut();
            s.settings.volume = Some(0.0);
            ui.set_volume(0.0);
            if send(&mut s, &ui, PlaybackCommand::SetVolume(0.0)) {
                add(&mut s, &ui, source);
            }
        }
    }
    if let Some(dir) = std::env::args().skip_while(|a| a != "--smoke-ui").nth(1) {
        if std::env::args().any(|a| a == "--smoke-library") {
            let mut s = state.borrow_mut();
            s.library_playlists = vec![
                tide_provider::Playlist {
                    artwork_url: None,
                    id: "PLfixture".into(),
                    title: "Late night focus".into(),
                    subtitle: "24 tracks".into(),
                },
                tide_provider::Playlist {
                    artwork_url: None,
                    id: "PLfixture2".into(),
                    title: "Weekend favourites".into(),
                    subtitle: "56 tracks".into(),
                },
            ];
            s.recommendations = vec![
                tide_provider::Card {
                    artwork_url: None,
                    id: "fixture0001".into(),
                    title: "A Quiet Sun".into(),
                    subtitle: "True North".into(),
                    kind: "song".into(),
                    shelf: "Listen again".into(),
                },
                tide_provider::Card {
                    artwork_url: None,
                    id: "PLfixture".into(),
                    title: "Your evening mix".into(),
                    subtitle: "A mix for you".into(),
                    kind: "playlist".into(),
                    shelf: "For you".into(),
                },
                tide_provider::Card {
                    artwork_url: None,
                    id: "MPREfixture".into(),
                    title: "Midnight Geometry".into(),
                    subtitle: "Echo Frame".into(),
                    kind: "album".into(),
                    shelf: "Recommended albums".into(),
                },
            ];
            refresh_library_models(&s, &ui);
            ui.set_library_open(true);
            ui.set_account_connected(true);
            ui.set_account_name("Sample listener".into());
            ui.set_account_status("Visual test data · no account connected".into());
        }
        if std::env::args().any(|a| a == "--smoke-populated") {
            let mut s = state.borrow_mut();
            s.settings.queue = Queue {
                tracks: vec![
                    Track {
                        artwork_url: None,
                        recommended: false,
                        title: "Tide Lines".into(),
                        artist: "Nova Wells".into(),
                        album: "Red Terrain".into(),
                        source: String::new(),
                    },
                    Track {
                        artwork_url: None,
                        recommended: false,
                        title: "Hollow Coast".into(),
                        artist: "Echo Frame".into(),
                        album: "Midnight Geometry".into(),
                        source: String::new(),
                    },
                    Track {
                        artwork_url: None,
                        recommended: false,
                        title: "A Quiet Sun / An intentionally long title for layout verification"
                            .into(),
                        artist: "True North".into(),
                        album: "Lower Light".into(),
                        source: String::new(),
                    },
                    Track {
                        artwork_url: None,
                        recommended: false,
                        title: "Still Here".into(),
                        artist: "Maris".into(),
                        album: "Still Here".into(),
                        source: String::new(),
                    },
                    Track {
                        artwork_url: None,
                        recommended: false,
                        title: "Drift".into(),
                        artist: "Nova Wells".into(),
                        album: "Red Terrain".into(),
                        source: String::new(),
                    },
                ],
                current: Some(0),
            };
            refresh(&ui, &s);
            ui.set_status_text("Visual test data · playback inactive".into());
        }
        {
            let mut s = state.borrow_mut();
            s.detail_tracks = s.settings.queue.tracks.clone();
            s.search_tracks = s.settings.queue.tracks.iter().take(3).cloned().collect();
            for t in s.settings.queue.tracks.iter_mut().skip(3) {
                t.recommended = true;
            }
            ui.set_detail_title("Late night focus".into());
            refresh(&ui, &s);
            refresh_content_models(&s, &ui);
        }
        if let Some(audio) = std::env::args()
            .skip_while(|a| a != "--fixture-audio")
            .nth(1)
        {
            {
                let mut s = state.borrow_mut();
                for t in s.settings.queue.tracks.iter_mut() {
                    t.source = audio.clone();
                }
                s.detail_tracks = s.settings.queue.tracks.clone();
                s.search_tracks = s.detail_tracks.iter().take(3).cloned().collect();
            }
            ui.set_search_open(true);
            ui.set_library_open(false);
            ui.invoke_select_search_track(1);
            assert!(
                ui.get_search_open() && !ui.get_queue_open(),
                "Song play must preserve search"
            );
            assert_eq!(
                state.borrow().settings.queue.tracks.len(),
                1,
                "Playing a search song starts a fresh seed"
            );
            ui.invoke_queue_search_track(2, false);
            {
                let mut s = state.borrow_mut();
                let mut automatic = s.search_tracks[0].clone();
                automatic.recommended = true;
                s.settings.queue.tracks.push(automatic);
            }
            ui.invoke_queue_search_track(0, true);
            assert_eq!(state.borrow().settings.queue.tracks[1].title, "Tide Lines");
            assert!(
                state
                    .borrow()
                    .settings
                    .queue
                    .tracks
                    .last()
                    .unwrap()
                    .recommended
            );
            ui.invoke_toggle_autoplay();
            assert!(
                !state
                    .borrow()
                    .settings
                    .queue
                    .tracks
                    .iter()
                    .any(|t| t.recommended),
                "Autoplay off removes upcoming suggestions"
            );
            ui.invoke_toggle_autoplay();
            ui.set_search_open(false);
            ui.set_detail_open(true);
            ui.invoke_select_detail_track(2);
            assert!(
                ui.get_detail_open() && !ui.get_queue_open(),
                "Playlist play must preserve detail view"
            );
            assert_eq!(state.borrow().settings.queue.current, Some(2));
            ui.invoke_shuffle_detail();
            assert_eq!(state.borrow().settings.queue.current, Some(0));
            ui.invoke_media_command(3, 0.0);
            assert_eq!(
                state.borrow().settings.queue.current,
                Some(1),
                "Media Next advances once"
            );
            assert!(ui.get_detail_open(), "Media Next preserves browsing");
            ui.invoke_media_command(4, 0.0);
            assert_eq!(
                state.borrow().settings.queue.current,
                Some(0),
                "Media Previous moves back once"
            );
            let title = state.borrow().settings.queue.tracks[2].title.clone();
            ui.invoke_reorder_queue_track(2, 1);
            assert_eq!(state.borrow().settings.queue.tracks[1].title, title);
            ui.invoke_remove_queue_track(1);
            assert_eq!(state.borrow().settings.queue.tracks.len(), 4);
            {
                let mut s = state.borrow_mut();
                send(&mut s, &ui, PlaybackCommand::Stop);
                s.active_playback = false;
                s.settings.queue = Queue {
                    tracks: s.detail_tracks.clone(),
                    current: Some(0),
                };
                for t in s.settings.queue.tracks.iter_mut().skip(3) {
                    t.recommended = true;
                }
                refresh(&ui, &s);
            }
            ui.set_detail_open(false);
            ui.set_paused(true);
            ui.set_status_text("Visual test data · playback inactive".into());
            println!("PASS: native song/playlist play preserves browsing; search seed, queue priority, autoplay off, shuffle, reorder and removal");
            println!("PASS: media Next/Previous share player actions and preserve browsing");
        }
        let output = PathBuf::from(dir);
        std::fs::create_dir_all(&output)?;
        let step = Rc::new(RefCell::new(0));
        let smoke_state = state.clone();
        let weak = ui.as_weak();
        ui.set_light_mode(false);
        ui.set_home_open(true);
        ui.set_library_open(false);
        ui.set_queue_open(false);
        ui.invoke_change_volume(33.0);
        assert_eq!(
            ui.get_volume(),
            33.0,
            "Volume display must follow the slider"
        );
        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
        smoke_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(2),
            move || {
                let Some(u) = weak.upgrade() else {
                    return;
                };
                let mut i = step.borrow_mut();
                let name = match *i {
                    0 => "full.png",
                    1 => "light.png",
                    2 => "compact-queue.png",
                    3 => "compact-library.png",
                    4 => "playlist-detail.png",
                    5 => "search.png",
                    6 => "queue.png",
                    7 => "settings.png",
                    8 => "library.png",
                    9 => "collapsed.png",
                    _ => "restored.png",
                };
                if let Ok(pixels) = u.window().take_snapshot() {
                    if let Some(image) = image::RgbaImage::from_raw(
                        pixels.width(),
                        pixels.height(),
                        pixels.as_bytes().to_vec(),
                    ) {
                        if let Err(e) = image.save(output.join(name)) {
                            eprintln!("Snapshot failed: {e}");
                        }
                    }
                }
                match *i {
                    0 => {
                        let size = u.window().size();
                        u.invoke_switch_mode();
                        assert_eq!(size, u.window().size(), "Theme must preserve window size");
                    }
                    1 => {
                        u.invoke_toggle_compact();
                        u.set_queue_open(true);
                    }
                    2 => {
                        u.set_queue_open(false);
                        u.set_library_open(true);
                    }
                    3 => {
                        u.invoke_toggle_compact();
                        u.invoke_switch_mode();
                        u.set_detail_open(true);
                    }
                    4 => {
                        u.set_detail_open(false);
                        u.set_search_open(true);
                    }
                    5 => {
                        u.set_search_open(false);
                        u.set_library_open(false);
                        u.set_queue_open(true);
                    }
                    6 => {
                        if smoke_state.borrow().settings.queue.tracks.len() < 3 {
                            u.set_queue_open(false);
                            u.set_settings_open(true);
                            *i += 1;
                            return;
                        }
                        use slint::platform::{PointerEventButton, WindowEvent};
                        let (title, generation) = {
                            let s = smoke_state.borrow();
                            (s.settings.queue.tracks[1].title.clone(), s.generation)
                        };
                        let window_width = u.window().size().width as f32 / u.window().scale_factor();
                        let start = slint::LogicalPosition::new(window_width - 306.0, 277.0);
                        let target = slint::LogicalPosition::new(window_width - 300.0, 363.0);
                        u.window().dispatch_event(WindowEvent::PointerMoved { position: start });
                        u.window().dispatch_event(WindowEvent::PointerPressed {
                            position: start, button: PointerEventButton::Left,
                        });
                        let drag_output = output.clone();
                        let weak = u.as_weak();
                        let drag_state = smoke_state.clone();
                        slint::Timer::single_shot(Duration::from_millis(30), move || {
                        let Some(u) = weak.upgrade() else { return; };
                        u.window().dispatch_event(WindowEvent::PointerMoved { position: target });
                        let weak = u.as_weak();
                        slint::Timer::single_shot(Duration::from_millis(30), move || {
                        let Some(u) = weak.upgrade() else { return; };
                        if let Ok(pixels) = u.window().take_snapshot() {
                            if let Some(image) = image::RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.as_bytes().to_vec()) {
                                image.save(drag_output.join("queue-drag.png")).expect("Save drag marker snapshot");
                            }
                        }
                        u.window().dispatch_event(WindowEvent::PointerReleased {
                            position: target, button: PointerEventButton::Left,
                        });
                        let weak = u.as_weak();
                        slint::Timer::single_shot(Duration::from_millis(50), move || {
                            let s = drag_state.borrow();
                            assert_eq!(s.settings.queue.tracks[2].title, title, "Dragging must move the selected song to the drop position");
                            assert_eq!(s.settings.queue.current, Some(0), "Dragging must keep the playing song fixed");
                            assert_eq!(s.generation, generation, "Dragging must not start playback");
                            println!("PASS: native pointer drag reorders upcoming songs without starting playback");
                            drop(s);
                            if let Some(u) = weak.upgrade() {
                                let saved_queue = drag_state.borrow().settings.queue.clone();
                                {
                                    let mut s = drag_state.borrow_mut();
                                    s.settings.queue = Queue {
                                        tracks: (0..100).map(|index| Track {
                                            title: format!("Drag fixture {index}"),
                                            artist: "Fixture artist".into(), album: String::new(),
                                            source: String::new(), artwork_url: None, recommended: false,
                                        }).collect(),
                                        current: Some(0),
                                    };
                                    refresh(&u, &s);
                                }
                                let _ = u.window().take_snapshot();
                                u.window().dispatch_event(WindowEvent::PointerMoved { position: start });
                                u.window().dispatch_event(WindowEvent::PointerPressed { position: start, button: PointerEventButton::Left });
                                let _ = u.window().take_snapshot();
                                let edge = slint::LogicalPosition::new(start.x, 590.0);
                                u.window().dispatch_event(WindowEvent::PointerMoved { position: edge });
                                let weak = u.as_weak();
                                slint::Timer::single_shot(Duration::from_millis(1000), move || {
                                    if let Some(u) = weak.upgrade() {
                                        u.window().dispatch_event(WindowEvent::PointerReleased { position: edge, button: PointerEventButton::Left });
                                        let weak = u.as_weak();
                                        slint::Timer::single_shot(Duration::from_millis(50), move || {
                                            let mut s = drag_state.borrow_mut();
                                            let target = s.settings.queue.tracks.iter().position(|track| track.title == "Drag fixture 1").unwrap();
                                            assert!(target > 5, "Edge drag must continue after its source scrolls out of view: {target}");
                                            assert_eq!(s.settings.queue.current, Some(0));
                                            assert_eq!(s.generation, generation);
                                            assert_eq!(s.settings.queue.tracks.len(), 100);
                                            println!("PASS: native long drag survives row recycling and edge autoscroll");
                                            s.settings.queue = saved_queue;
                                            if let Some(u) = weak.upgrade() {
                                                refresh(&u, &s);
                                                u.set_queue_open(false);
                                                u.set_settings_open(true);
                                            }
                                        });
                                    }
                                });
                            }
                        });
                        });
                        });
                    }
                    7 => {
                        u.set_settings_open(false);
                        u.set_library_open(true);
                    }
                    8 => {
                        u.invoke_collapse_player();
                    }
                    9 => {
                        u.invoke_collapse_player();
                        u.set_library_open(false);
                        u.set_home_open(true);
                    }
                    _ => {
                        let _ = u.hide();
                        let _ = slint::quit_event_loop();
                    }
                }
                *i += 1;
            },
        );
    }
    let early_skip_passed = Rc::new(RefCell::new(!smoke_early_skip));
    if smoke_early_skip {
        let sources: Vec<_> = std::env::args()
            .skip_while(|a| a != "--smoke-early-skip" && a != "--smoke-early-skip-session")
            .skip(1)
            .take(2)
            .collect();
        if sources.len() != 2 || sources.iter().any(|source| !is_youtube_source(source)) {
            return Err("Early-skip smoke requires two public YouTube URLs".into());
        }
        ui.invoke_change_volume(0.0);
        {
            let mut s = state.borrow_mut();
            s.settings.autoplay = false;
            s.settings.queue = Queue {
                tracks: sources
                    .iter()
                    .enumerate()
                    .map(|(index, source)| Track {
                        title: format!("Early skip probe {}", index + 1),
                        artist: String::new(),
                        album: String::new(),
                        source: source.clone(),
                        artwork_url: None,
                        recommended: false,
                    })
                    .collect(),
                current: Some(0),
            };
        }
        let state = state.clone();
        let weak = ui.as_weak();
        let passed = early_skip_passed.clone();
        let started = std::time::Instant::now();
        let mut skip_started: Option<std::time::Instant> = None;
        let mut playback_started = false;
        smoke_timer.start(slint::TimerMode::Repeated, Duration::from_millis(25), move || {
            let Some(u) = weak.upgrade() else { return; };
            if !playback_started {
                let mut s = state.borrow_mut();
                if !smoke_skip_session || s.account_verified {
                    if !play(&mut s, &u) {
                        eprintln!("Could not start early-skip probe");
                        let _ = slint::quit_event_loop();
                        return;
                    }
                    playback_started = true;
                } else if started.elapsed() > Duration::from_secs(90) {
                    eprintln!("Early-skip session verification timed out");
                    let _ = slint::quit_event_loop();
                }
                return;
            }
            // Drop the state borrow before invoking the same Next callback that
            // the player button and Windows media key use.
            let ready = {
                let s = state.borrow();
                let engine = s.engine.as_ref().unwrap();
                let p = engine.snapshot();
                assert_eq!(p.volume, 0.0, "Early-skip probe must stay muted");
                if p.error.is_some() {
                    eprintln!("Early-skip playback failed");
                    let _ = slint::quit_event_loop();
                    return;
                }
                if let Some(skipped_at) = skip_started.filter(|_| p.generation == 2 && !p.loading && p.position > 0.05) {
                    *passed.borrow_mut() = true;
                    println!("PASS early skip: {:.3}s from Next to decoded audio; prepared without seeking near EOF", skipped_at.elapsed().as_secs_f64());
                    let _ = u.hide(); let _ = slint::quit_event_loop(); return;
                }
                skip_started.is_none() && p.generation == 1 && !p.loading && p.duration > 120.0
                    && p.duration - p.position > 60.0 && engine.next_is_prepared(&sources[1])
            };
            if ready {
                println!("Next stream ready after {:.3}s from initial Load", started.elapsed().as_secs_f64());
                skip_started = Some(std::time::Instant::now());
                u.invoke_media_command(3, 0.0);
            }
            if started.elapsed() > Duration::from_secs(90) {
                eprintln!("Early-skip smoke timed out"); let _ = u.hide(); let _ = slint::quit_event_loop();
            }
        });
    }
    let autoplay_passed = Rc::new(RefCell::new(!smoke_autoplay));
    if smoke_autoplay {
        let state = state.clone();
        let weak = ui.as_weak();
        let passed = autoplay_passed.clone();
        let started = std::time::Instant::now();
        let mut stage = 0;
        smoke_timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(u) = weak.upgrade() else { return; };
            let mut s = state.borrow_mut();
            if stage == 0 && s.account_verified && s.library_worker.is_none() {
                if add(&mut s, &u, "https://www.youtube.com/watch?v=K4DyBUG242c".into()) {
                    stage = 1;
                }
            } else if stage == 1 && s.settings.queue.tracks.len() > 1 && u.get_duration() > 2.0
                && s.library_worker.is_none() {
                if smoke_autoplay_depletion {
                    let current = s.settings.queue.current.unwrap();
                    let upcoming = s.settings.queue.tracks.len() - current - 1;
                    let generation = s.generation;
                    drop(s);
                    for _ in 0..upcoming {
                        u.invoke_remove_queue_track((current + 1) as i32);
                    }
                    s = state.borrow_mut();
                    if s.settings.queue.tracks.len() != current + 1 || s.generation != generation {
                        eprintln!("Autoplay depletion probe could not remove upcoming tracks safely");
                        let _ = slint::quit_event_loop();
                        return;
                    }
                    println!("Autoplay depletion check: upcoming rows removed without restarting current playback");
                }
                let near_end = f64::from(u.get_duration()) - 0.2;
                send(&mut s, &u, PlaybackCommand::Seek(near_end));
                stage = 2;
            } else if stage == 2 && s.generation >= 2 && u.get_position() > 0.0 {
                *passed.borrow_mut() = true;
                if smoke_autoplay_depletion {
                    println!("PASS: depleted queue refilled and native EOF continued decoded playback; muted playback");
                } else {
                    println!("PASS: recommendations appended and native EOF advanced to the next song; muted playback");
                }
                let _ = u.hide();
                let _ = slint::quit_event_loop();
            }
            if started.elapsed() > Duration::from_secs(120) {
                eprintln!("Autoplay smoke timed out at stage {stage}");
                let _ = u.hide();
                let _ = slint::quit_event_loop();
            }
        });
    }
    let listening = listening_seconds
        .map(|seconds| listening::start(state.clone(), &ui, seconds, local_listening_source));
    // Sustained probes must not expose a second interactive player or compete
    // with the user's window. Keep timers running without a visible window.
    let result = if listening_seconds.is_some() || smoke_autoplay_depletion {
        ui.hide()?;
        slint::run_event_loop_until_quit()
    } else {
        ui.run()
    };
    timer.stop();
    preparation_timer.stop();
    media_timer.stop();
    window_fit_timer.stop();
    smoke_timer.stop();
    if let Some((timer, _)) = &listening {
        timer.stop();
    }
    let mut s = state.borrow_mut();
    if s.volume_save_pending {
        save(&s.settings);
    }
    s.search_cancel.store(true, Ordering::Release);
    s.library_cancel.store(true, Ordering::Release);
    if let Some(worker) = s.library_worker.take() {
        let _ = worker.join();
    }
    if let Some(worker) = s.search_worker.take() {
        let _ = worker.join();
    }
    if let Some(e) = &s.engine {
        let _ = e.send(PlaybackCommand::Shutdown);
    }
    result?;
    if listening
        .as_ref()
        .is_some_and(|(_, passed)| !*passed.borrow())
    {
        return Err("Sustained listening verification did not complete".into());
    }
    if !*early_skip_passed.borrow() {
        return Err("Early-skip verification did not complete".into());
    }
    if !*autoplay_passed.borrow() {
        return Err("Live autoplay verification did not complete".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obsolete_radio_responses_cannot_change_current_account_state() {
        let action = LibraryAction::Radio {
            seed: "fixture0001".into(),
            generation: 3,
            replace_upcoming: false,
        };
        assert!(action.accepts_response(3, Some("fixture0001"), true));
        assert!(!action.accepts_response(4, Some("fixture0001"), true));
        assert!(!action.accepts_response(3, Some("fixture0002"), true));
        assert!(!action.accepts_response(3, Some("fixture0001"), false));
    }

    #[test]
    fn older_saved_tracks_default_to_manual() {
        let t: Track =
            serde_json::from_str(r#"{"title":"old","artist":"","album":"","source":"local.wav"}"#)
                .unwrap();
        assert!(!t.recommended);
    }

    #[test]
    fn account_gate_is_only_for_youtube_hosts() {
        for source in [
            "https://www.youtube.com/watch?v=fixture0001",
            "https://music.youtube.com/watch?v=fixture0001",
            "https://youtu.be/fixture0001",
        ] {
            assert!(is_youtube_source(source));
        }
        for source in [
            "local.wav",
            "https://youtube.com.example/song",
            "https://example.com/youtube.com",
        ] {
            assert!(!is_youtube_source(source));
        }
    }

    fn track(source: &str) -> Track {
        Track {
            artwork_url: None,
            recommended: false,
            title: source.into(),
            artist: String::new(),
            album: String::new(),
            source: source.into(),
        }
    }

    #[test]
    fn preparation_survives_eof_until_queue_handoff_but_not_stop_or_failure() {
        let mut p = tide_core::PlaybackSnapshot {
            generation: 7,
            ended: true,
            ..Default::default()
        };
        assert!(preparation_eligible(true, 7, &p));
        assert!(!preparation_eligible(false, 7, &p));
        assert!(!preparation_eligible(true, 8, &p));
        p.error = Some("failure".into());
        assert!(!preparation_eligible(true, 7, &p));
        p.error = None;
        p.loading = true;
        assert!(!preparation_eligible(true, 7, &p));
    }

    #[test]
    fn content_retries_preserve_playlist_identity_without_repeating_account_actions() {
        use tide_provider::Request;
        for request in [
            Request::Library,
            Request::Home,
            Request::Liked,
            Request::Playlist {
                id: "fixture-playlist".into(),
            },
        ] {
            assert_eq!(
                serde_json::to_value(content_retry_request(&request).unwrap()).unwrap(),
                serde_json::to_value(request).unwrap()
            );
        }
        for request in [
            Request::Connect {
                database: "fixture.sqlite".into(),
                index: 1,
            },
            Request::SwitchAccount { index: 1 },
            Request::Radio {
                video_id: "fixture0001".into(),
            },
        ] {
            assert!(content_retry_request(&request).is_none());
        }
    }

    #[test]
    fn prepare_follows_queue_order_and_wraps_only_with_repeat() {
        let mut queue = Queue {
            tracks: vec![track("current"), track("next"), track("last")],
            current: Some(0),
        };
        assert_eq!(upcoming_source(&queue, false), Some("next"));
        queue.tracks.swap(1, 2);
        assert_eq!(upcoming_source(&queue, false), Some("last"));
        queue.current = Some(2);
        assert_eq!(upcoming_source(&queue, false), None);
        assert_eq!(upcoming_source(&queue, true), Some("current"));
        queue.current = Some(99);
        assert_eq!(upcoming_source(&queue, true), None);
        queue.current = None;
        assert_eq!(upcoming_source(&queue, true), None);
    }

    #[test]
    fn deleting_upcoming_tracks_reopens_successful_radio_without_unbounding_failures() {
        let mut queue = Queue {
            tracks: vec![track("current"), track("next")],
            current: Some(0),
        };
        let mut requests = radio::RadioRequests::default();
        let now = Instant::now();
        requests.started(7);
        requests.succeeded();
        assert!(!requests.ready(7, now));
        queue.remove(1).unwrap();
        assert_eq!(upcoming_source(&queue, false), None);
        requests.queue_depleted(7);
        assert!(requests.ready(7, now));
        requests.started(7);
        assert!(!requests.ready(7, now));
        for attempt in 1..=3 {
            if attempt > 1 {
                requests.started(7);
            }
            requests.failed(now);
        }
        requests.queue_depleted(7);
        assert!(!requests.ready(7, now + Duration::from_secs(3600)));
    }

    #[test]
    fn failed_load_preserves_generation_and_active_playback() {
        let mut generation = 7;
        let mut active = true;
        let result = submit_load(&mut generation, &mut active, |next| {
            assert_eq!(next, 8);
            Err("rejected".into())
        });
        assert_eq!(result.unwrap_err(), "rejected");
        assert_eq!(generation, 7);
        assert!(active);
    }

    #[test]
    fn restarting_stopped_or_ended_playback_increases_generation() {
        let mut generation = 4;
        let mut active = false;
        submit_load(&mut generation, &mut active, |next| {
            assert_eq!(next, 5);
            Ok(())
        })
        .unwrap();
        assert!(active);
        active = false;
        submit_load(&mut generation, &mut active, |next| {
            assert_eq!(next, 6);
            Ok(())
        })
        .unwrap();
        assert_eq!(generation, 6);
        assert!(active);
    }

    #[test]
    fn generation_overflow_does_not_submit_a_load() {
        let mut generation = u64::MAX;
        let mut active = false;
        assert!(submit_load(&mut generation, &mut active, |_| panic!("must not submit")).is_err());
        assert_eq!(generation, u64::MAX);
        assert!(!active);
    }

    #[test]
    fn saved_urls_are_sanitized_and_selection_is_remapped() {
        let mut queue = Queue {
            tracks: vec![
                track("https://user:secret@example.com/song"),
                track("C:\\unavailable\\track.wav"),
                track("https://example.com/song"),
                track("ftp://example.com/song"),
            ],
            current: Some(2),
        };
        assert!(sanitize_queue(&mut queue));
        assert_eq!(queue.tracks.len(), 2);
        assert_eq!(queue.current, Some(1));
        assert_eq!(queue.tracks[0].source, "C:\\unavailable\\track.wav");
        assert!(!sanitize_queue(&mut queue));
        queue.tracks[1].source = "https://user:secret@example.com/song".into();
        assert!(sanitize_queue(&mut queue));
        assert_eq!(queue.current, None);
    }
}
