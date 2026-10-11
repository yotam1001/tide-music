#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
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
    library_worker: Option<std::thread::JoinHandle<()>>,
    library_cancel: Arc<AtomicBool>,
    library_playlists: Vec<tide_provider::Playlist>,
    recommendations: Vec<tide_provider::Card>,
    detail_tracks: Vec<Track>,
    search_tracks: Vec<Track>,
    account_error: bool,
    account_verified: bool,
    library_action: LibraryAction,
    shuffle_request: bool,
    radio_attempted_generation: Option<u64>,
    radio_resume_when_ready: bool,
    artwork: RefCell<artwork::ArtworkCache>,
}

type LibraryResult = Result<tide_provider::Response, String>;
fn select_zen_session() -> Option<tide_provider::Request> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Connect your Zen session: select cookies.sqlite in your profile")
        .add_filter("Zen cookie database", &["sqlite"])
        .set_file_name("cookies.sqlite");
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dialog = dialog.set_directory(PathBuf::from(appdata).join("zen/Profiles"));
    }
    dialog
        .pick_file()
        .map(|database| tide_provider::Request::Connect { database })
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
            || s.radio_attempted_generation == Some(s.generation))
    {
        return;
    }
    let Some(seed) = current_video_id(s) else {
        return;
    };
    s.radio_attempted_generation = Some(s.generation);
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

fn configure_account_playback(s: &mut State, u: &AppWindow) -> bool {
    s.engine.take(); // Drop/join helpers before releasing their cookie lease.
    s.cookie_lease.take();
    s.active_playback = false;
    s.radio_resume_when_ready = false;
    s.radio_attempted_generation = None;
    s.artwork.borrow_mut().clear();
    s.account_verified = false;
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
    state.command_error = Some(error.clone());
    ui.set_error_message(error.clone().into());
    ui.set_status_text(error.into());
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
    let ui = AppWindow::new()?;
    let mut settings: Settings = std::fs::read(settings_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let smoke_autoplay = std::env::args().any(|a| a == "--smoke-autoplay");
    if smoke_autoplay || std::env::args().any(|a| a == "--smoke-ui" || a == "--benchmark-mode") {
        settings = Settings {
            transient: true,
            volume: smoke_autoplay.then_some(0.0),
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
    let test_mode = std::env::args().any(|a| a == "--smoke-ui" || a == "--benchmark-mode");
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
        library_worker: None,
        library_cancel: Arc::new(AtomicBool::new(false)),
        library_playlists: Vec::new(),
        recommendations: Vec::new(),
        detail_tracks: Vec::new(),
        search_tracks: Vec::new(),
        account_error: false,
        account_verified: false,
        library_action: LibraryAction::default(),
        shuffle_request: false,
        radio_attempted_generation: None,
        radio_resume_when_ready: false,
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
        ui.on_switch_account(move |index| {
            if let Some(u) = weak.upgrade() {
                if (0..10).contains(&index) {
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
        if let Some(request) = select_zen_session() {
            request_library(&mut state.borrow_mut(), &ui, &library_tx, request);
        }
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_connect_account(move || {
            if let Some(u) = weak.upgrade() {
                let Some(request) = select_zen_session() else {
                    return;
                };
                request_library(&mut state.borrow_mut(), &u, &tx, request);
            }
        });
    }
    {
        let state = state.clone();
        ui.on_cancel_account(move || {
            state.borrow().library_cancel.store(true, Ordering::Release);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let tx = library_tx.clone();
        ui.on_import_account(move || {
            let file = rfd::FileDialog::new()
                .add_filter(
                    "Browser request headers / authentication JSON",
                    &["txt", "json"],
                )
                .pick_file();
            let (Some(file), Some(u)) = (file, weak.upgrade()) else {
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
                .and_then(|p| p.import_session(&file));
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
        ui.on_disconnect_account(move || {
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
        ui.on_retry_error(move || {
            if let Some(u) = weak.upgrade() {
                if state.borrow().account_error {
                    u.set_settings_open(true);
                } else {
                    u.set_error_message("".into());
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
        ui.on_move_queue_track(move |index, direction| {
            if let Some(u) = weak.upgrade() {
                let mut s = state.borrow_mut();
                if let Ok(index) = usize::try_from(index) {
                    if s.settings.queue.move_upcoming(index, direction) {
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
        ui.on_open_file(move || {
            // The native dialog can pump window messages. Borrow state only
            // after it returns so timer/media callbacks can continue safely.
            if let Some(p) = rfd::FileDialog::new()
                .add_filter(
                    "Audio",
                    &["mp3", "m4a", "ogg", "opus", "flac", "wav", "aac"],
                )
                .pick_file()
            {
                if let Some(u) = weak.upgrade() {
                    add(
                        &mut state.borrow_mut(),
                        &u,
                        p.to_string_lossy().into_owned(),
                    );
                }
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
                    s.radio_attempted_generation = None;
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
            s.radio_attempted_generation = None;
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
                    slint::LogicalSize::new(
                        size.width
                            .max(if u.get_compact_mode() { 700.0 } else { 950.0 }),
                        size.height.max(660.0),
                    )
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
                2 => {
                    if !s.active_playback && s.settings.queue.current.is_some() {
                        play(&mut s, &u);
                    } else {
                        send(&mut s, &u, PlaybackCommand::TogglePause);
                    }
                }
                3 => {
                    let repeat = s.settings.repeat;
                    let previous = s.settings.queue.current;
                    if s.settings.queue.next_track(repeat).is_some() && !play(&mut s, &u) {
                        s.settings.queue.current = previous;
                        refresh(&u, &s);
                    }
                }
                4 => {
                    let previous = s.settings.queue.current;
                    if s.settings.queue.previous_track().is_some() && !play(&mut s, &u) {
                        s.settings.queue.current = previous;
                        refresh(&u, &s);
                    }
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
                save(&s.settings);
            }
        });
    }
    ui.show()?;
    let media = Rc::new(RefCell::new(None::<tide_windows::MediaSession>));
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = ui.window().window_handle().window_handle() {
            if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                let weak = ui.as_weak();
                match tide_windows::start(
                    handle.hwnd.get() as *mut std::ffi::c_void,
                    move |action| {
                        let (code, value) = match action {
                            tide_windows::MediaAction::Play => (0, 0.0),
                            tide_windows::MediaAction::Pause => (1, 0.0),
                            tide_windows::MediaAction::Toggle => (2, 0.0),
                            tide_windows::MediaAction::Next => (3, 0.0),
                            tide_windows::MediaAction::Previous => (4, 0.0),
                            tide_windows::MediaAction::Stop => (5, 0.0),
                            tide_windows::MediaAction::Seek(seconds) => (6, seconds as f32),
                        };
                        let _ = weak
                            .upgrade_in_event_loop(move |u| u.invoke_media_command(code, value));
                    },
                ) {
                    Ok(session) => *media.borrow_mut() = Some(session),
                    Err(error) => {
                        ui.set_status_text(format!("Media controls unavailable: {error}").into())
                    }
                }
            }
        }
    }
    let timer = slint::Timer::default();
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let media = media.clone();
        let library_tx = library_tx.clone();
        let mut last_content_view = None;
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                let Some(u) = weak.upgrade() else {
                    return;
                };
                let mut s = state.borrow_mut();
                let content_view = (u.get_library_open(), u.get_detail_open(), u.get_search_open());
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
                                    s.radio_resume_when_ready = false;
                                    u.set_account_status(
                                        "No further song recommendations available".into(),
                                    );
                                    continue;
                                }
                                if replace_upcoming {
                                    let keep =
                                        s.settings.queue.current.map_or(0, |index| index + 1);
                                    s.settings.queue.tracks.truncate(keep);
                                }
                                let old_len = s.settings.queue.tracks.len();
                                let added = s.settings.queue.append_deduplicated(tracks.into_iter().map(|mut t| {
                                    t.recommended = true; t
                                }));
                                if added > 0 && !s.active_playback && s.radio_resume_when_ready {
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
                                u.set_account_status(if truncated {
                                    "Showing the first 2,000 playlist tracks"
                                } else { "Playlist ready" }.into());
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
                        Ok(
                            tide_provider::Response::Error { .. }
                            | tide_provider::Response::Session { .. },
                        ) => {
                            s.radio_resume_when_ready = false;
                            s.account_error = true;
                            u.set_error_is_account(true);
                            u.set_error_message("Reconnect your YouTube Music account".into());
                            u.set_account_status(
                                "Account request failed; reconnect and retry".into(),
                            );
                            u.set_status_text(
                                "Open Account settings to reconnect YouTube Music".into(),
                            );
                        }
                        Err(error) => {
                            s.radio_resume_when_ready = false;
                            s.account_error = true;
                            u.set_error_is_account(true);
                            u.set_error_message("Account unavailable. Reconnect or retry.".into());
                            u.set_account_status(error.into());
                            u.set_status_text(
                                "Account request failed; open Account settings to reconnect or retry".into(),
                            );
                        }
                    }
                }
                let artwork_changed = s.artwork.borrow_mut().poll();
                if artwork_changed {
                    refresh(&u, &s);
                    refresh_library_models(&s, &u);
                    refresh_content_models(&s, &u);
                }
                if s.active_playback {
                    request_radio(&mut s, &u, &library_tx, false);
                }
                if let Some(session) = media.borrow_mut().as_mut() {
                    let _ = session.update(
                        &u.get_track_title(),
                        &u.get_track_artist(),
                        &u.get_track_album(),
                        !u.get_paused() && s.active_playback,
                        u.get_position() as f64,
                        u.get_duration() as f64,
                    );
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
                            s.radio_resume_when_ready = s.settings.autoplay;
                            request_radio(&mut s, &u, &library_tx, false);
                            if !failed {
                                u.set_status_text("Queue finished".into());
                            }
                        }
                    } else if failed {
                        s.active_playback = false;
                        u.set_paused(true);
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
            let title = state.borrow().settings.queue.tracks[2].title.clone();
            ui.invoke_move_queue_track(2, -1);
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
        }
        let output = PathBuf::from(dir);
        std::fs::create_dir_all(&output)?;
        let step = Rc::new(RefCell::new(0));
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
                        u.set_queue_open(false);
                        u.set_settings_open(true);
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
            } else if stage == 1 && s.settings.queue.tracks.len() > 1 && u.get_duration() > 2.0 {
                let near_end = f64::from(u.get_duration()) - 0.2;
                send(&mut s, &u, PlaybackCommand::Seek(near_end));
                stage = 2;
            } else if stage == 2 && s.generation >= 2 && u.get_position() > 0.0 {
                *passed.borrow_mut() = true;
                println!("PASS: recommendations appended and native EOF advanced to the next song; muted playback");
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
    let result = ui.run();
    timer.stop();
    smoke_timer.stop();
    let mut s = state.borrow_mut();
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
    if !*autoplay_passed.borrow() {
        return Err("Live autoplay verification did not complete".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
