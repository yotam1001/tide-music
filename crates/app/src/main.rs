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
slint::include_modules!();

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    queue: Queue,
    light_mode: bool,
    volume: Option<f64>,
    repeat: bool,
    #[serde(skip)]
    transient: bool,
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
    account_verified: bool,
}

type LibraryResult = Result<tide_provider::Response, String>;
fn request_library(
    s: &mut State,
    u: &AppWindow,
    tx: &mpsc::SyncSender<LibraryResult>,
    request: tide_provider::Request,
) {
    if s.library_worker.is_some() {
        return;
    }
    let Some(provider) = s.provider.clone().filter(|p| p.is_connected()) else {
        u.set_account_status("Import a local signed-in YouTube Music session first".into());
        return;
    };
    s.library_cancel.store(false, Ordering::Release);
    let cancel = s.library_cancel.clone();
    let tx = tx.clone();
    u.set_account_busy(true);
    u.set_account_status("Fetching YouTube Music…".into());
    s.library_worker = Some(std::thread::spawn(move || {
        let _ = tx.try_send(provider.execute(request, cancel));
    }));
}

fn configure_account_playback(s: &mut State, u: &AppWindow) -> bool {
    s.engine.take(); // Drop/join helpers before releasing their cookie lease.
    s.cookie_lease.take();
    s.active_playback = false;
    s.account_verified = false;
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
        let engine =
            tide_playback::start_with_cookie_file(lease.as_ref().map(|l| l.path().to_path_buf()))?;
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
    let playlists: Vec<PlaylistRow> = s
        .library_playlists
        .iter()
        .map(|p| PlaylistRow {
            id: p.id.clone().into(),
            title: p.title.clone().into(),
            subtitle: p.subtitle.clone().into(),
        })
        .collect();
    u.set_library_playlists(ModelRc::new(VecModel::from(playlists)));
    let cards: Vec<RecommendationRow> = s
        .recommendations
        .iter()
        .map(|p| RecommendationRow {
            id: p.id.clone().into(),
            title: p.title.clone().into(),
            subtitle: p.subtitle.clone().into(),
            kind: p.kind.clone().into(),
            shelf: p.shelf.clone().into(),
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
fn refresh(ui: &AppWindow, state: &State) {
    let rows: Vec<TrackRow> = state
        .settings
        .queue
        .tracks
        .iter()
        .map(|t| TrackRow {
            title: t.title.clone().into(),
            artist: t.artist.clone().into(),
            album: t.album.clone().into(),
            duration: "—".into(),
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
    } else {
        ui.set_track_title("Nothing playing".into());
        ui.set_track_artist("Add a file or paste a music link".into());
        ui.set_track_album("".into());
    }
}
fn report_error(state: &mut State, ui: &AppWindow, error: String) {
    state.command_error = Some(error.clone());
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
        report_error(
            state,
            ui,
            "Refresh the library to verify your account before signed-in playback".into(),
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
    if state.settings.queue.tracks.len() >= 2000 {
        ui.set_status_text("Queue limit reached (2,000 tracks).".into());
        return false;
    }
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
    state.settings.queue.tracks.push(Track {
        title,
        artist: String::new(),
        album: String::new(),
        source,
    });
    let index = state.settings.queue.tracks.len() - 1;
    select_and_play(state, ui, index);
    save(&state.settings);
    true
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
    if std::env::args().any(|a| a == "--smoke-ui" || a == "--benchmark-mode") {
        settings = Settings {
            transient: true,
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
            tide_playback::start_with_cookie_file(
                cookie_lease.as_ref().map(|l| l.path().to_path_buf()),
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
        account_verified: false,
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
    if ui.get_account_connected() {
        request_library(
            &mut state.borrow_mut(),
            &ui,
            &library_tx,
            tide_provider::Request::Library,
        );
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
                        if add(
                            &mut s,
                            &u,
                            format!("https://www.youtube.com/watch?v={}", card.id),
                        ) {
                            u.set_library_open(false);
                        }
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
    ui.on_open_account_help(|| {
        let mut command=std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg("https://github.com/yotam1001/tide-music/blob/codex/account-library-sync/docs/ACCOUNT_SETUP.md");
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
    bind!(on_next_track, |s, u| {
        let repeat = s.settings.repeat;
        let previous = s.settings.queue.current;
        if s.settings.queue.next_track(repeat).is_some() && !play(&mut s, &u) {
            s.settings.queue.current = previous;
            refresh(&u, &s);
        }
    });
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
        u.set_collapsed(false);
        u.window().set_size(if s.settings.light_mode {
            slint::LogicalSize::new(490.0, 680.0)
        } else {
            slint::LogicalSize::new(1200.0, 800.0)
        });
        save(&s.settings);
    });
    bind!(on_clear_queue, |s, u| {
        if !send(&mut s, &u, PlaybackCommand::Stop) {
            return;
        }
        s.settings.queue = Queue::default();
        s.active_playback = false;
        refresh(&u, &s);
        u.set_position(0.0);
        u.set_duration(0.0);
        u.set_paused(true);
        u.set_status_text("Queue cleared".into());
        save(&s.settings);
    });
    {
        let weak = ui.as_weak();
        ui.on_collapse_player(move || {
            if let Some(u) = weak.upgrade() {
                let c = !u.get_collapsed();
                u.set_collapsed(c);
                u.window().set_size(if c {
                    slint::LogicalSize::new(650.0, 170.0)
                } else {
                    slint::LogicalSize::new(490.0, 680.0)
                });
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
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                let Some(u) = weak.upgrade() else {
                    return;
                };
                let mut s = state.borrow_mut();
                while let Ok(result) = library_rx.try_recv() {
                    if let Some(worker) = s.library_worker.take() {
                        let _ = worker.join();
                    }
                    u.set_account_busy(false);
                    match result {
                        Ok(tide_provider::Response::Library {
                            account,
                            playlists,
                            truncated,
                        }) => {
                            s.account_verified = true;
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
                                if play(&mut s, &u) {
                                    u.set_library_open(false);
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
                        Ok(tide_provider::Response::Error { .. }) => u.set_account_status(
                            "Account request failed; reconnect and retry".into(),
                        ),
                        Err(error) => u.set_account_status(error.into()),
                    }
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
                            let remaining = 2000usize.saturating_sub(s.settings.queue.tracks.len());
                            let count = tracks.len().min(remaining);
                            s.settings
                                .queue
                                .tracks
                                .extend(tracks.into_iter().take(remaining));
                            refresh(&u, &s);
                            save(&s.settings);
                            u.set_status_text(
                                format!("Added {count} results · choose a track to play").into(),
                            );
                        }
                        Err(e) => u.set_status_text(e.into()),
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
                            if !failed {
                                u.set_status_text("Queue finished".into());
                            }
                        }
                    } else if failed {
                        s.active_playback = false;
                    }
                }
            },
        );
    }
    if state.borrow().settings.light_mode {
        ui.window().set_size(slint::LogicalSize::new(490.0, 680.0));
    }
    let smoke_timer = slint::Timer::default();
    if let Some(mode) = std::env::args()
        .skip_while(|a| a != "--benchmark-mode")
        .nth(1)
    {
        ui.set_light_mode(mode != "full");
        ui.set_collapsed(mode == "collapsed");
        let (width, height) = match mode.as_str() {
            "full" => (1200.0, 800.0),
            "collapsed" => (650.0, 170.0),
            _ => (490.0, 680.0),
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
                    id: "PLfixture".into(),
                    title: "Late night focus".into(),
                    subtitle: "24 tracks".into(),
                },
                tide_provider::Playlist {
                    id: "PLfixture2".into(),
                    title: "Weekend favourites".into(),
                    subtitle: "56 tracks".into(),
                },
            ];
            s.recommendations = vec![
                tide_provider::Card {
                    id: "fixture0001".into(),
                    title: "A Quiet Sun".into(),
                    subtitle: "True North".into(),
                    kind: "song".into(),
                    shelf: "Listen again".into(),
                },
                tide_provider::Card {
                    id: "PLfixture".into(),
                    title: "Your evening mix".into(),
                    subtitle: "A mix for you".into(),
                    kind: "playlist".into(),
                    shelf: "For you".into(),
                },
                tide_provider::Card {
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
                        title: "Tide Lines".into(),
                        artist: "Nova Wells".into(),
                        album: "Red Terrain".into(),
                        source: String::new(),
                    },
                    Track {
                        title: "Hollow Coast".into(),
                        artist: "Echo Frame".into(),
                        album: "Midnight Geometry".into(),
                        source: String::new(),
                    },
                    Track {
                        title: "A Quiet Sun / An intentionally long title for layout verification"
                            .into(),
                        artist: "True North".into(),
                        album: "Lower Light".into(),
                        source: String::new(),
                    },
                    Track {
                        title: "Still Here".into(),
                        artist: "Maris".into(),
                        album: "Still Here".into(),
                        source: String::new(),
                    },
                    Track {
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
        let output = PathBuf::from(dir);
        std::fs::create_dir_all(&output)?;
        let step = Rc::new(RefCell::new(0));
        let weak = ui.as_weak();
        ui.set_light_mode(false);
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
                    _ => "collapsed.png",
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
                        u.set_light_mode(true);
                        u.window().set_size(slint::LogicalSize::new(490.0, 680.0));
                    }
                    1 => {
                        u.set_collapsed(true);
                        u.window().set_size(slint::LogicalSize::new(650.0, 170.0));
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
