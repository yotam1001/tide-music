//! MPRIS media session and StatusNotifierItem tray icon.
//!
//! D-Bus work runs on background threads; the UI thread only posts cached,
//! changed values. Callbacks are isolated with `catch_unwind` like Windows.
use super::x11;
use crate::MediaAction;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

type Callback = Arc<Mutex<Box<dyn Fn(MediaAction) + Send>>>;

const OBJECT: &str = "/org/mpris/MediaPlayer2";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
pub(crate) const ICON_PNG: &[u8] = include_bytes!("../../../../ui/assets/tide-icon.png");

struct Shared {
    window: Option<u32>,
    callback: Callback,
    close_to_tray: AtomicBool,
    tray_ready: AtomicBool,
    quit: AtomicBool,
    playing: AtomicBool,
    enabled: AtomicBool,
}

impl Shared {
    fn dispatch(&self, action: MediaAction) {
        if !self.enabled.load(Ordering::Acquire) && !matches!(action, MediaAction::ShowMini) {
            return;
        }
        if let Ok(callback) = self.callback.lock() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(action)));
        }
    }

    fn show_player(&self) {
        if let (Some(window), Ok(x)) = (self.window, x11::shared()) {
            let _ = x.show(window);
        }
    }

    fn hide_player(&self) {
        if let (Some(window), Ok(x)) = (self.window, x11::shared()) {
            let _ = x.hide(window);
        }
    }

    fn quit(&self) {
        self.quit.store(true, Ordering::Release);
        if let (Some(window), Ok(x)) = (self.window, x11::shared()) {
            let _ = x.request_close(window);
        }
    }
}

#[derive(Clone, Default, PartialEq)]
struct Track {
    id: u64,
    title: String,
    artist: String,
    album: String,
    length_us: i64,
    art_url: String,
}

struct PlayerState {
    track: Track,
    playing: bool,
    has_track: bool,
    position_us: i64,
    at: Instant,
}

impl PlayerState {
    fn position(&self) -> i64 {
        let mut position = self.position_us;
        if self.playing {
            position += self.at.elapsed().as_micros() as i64;
        }
        if self.track.length_us > 0 {
            position = position.min(self.track.length_us);
        }
        position.max(0)
    }
}

enum Update {
    Track(Track),
    Status(bool),
    Seeked(i64),
}

struct Root {
    shared: Arc<Shared>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.shared.show_player();
    }
    fn quit(&self) {
        self.shared.quit();
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        self.shared.window.is_some()
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        self.shared.window.is_some()
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Tide Music"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "tide-music"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    shared: Arc<Shared>,
    state: Arc<Mutex<PlayerState>>,
}

fn track_path(id: u64) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("/org/tide/track/{id}"))
        .unwrap_or_else(|_| OwnedObjectPath::try_from(NO_TRACK).expect("static path"))
}

fn owned(value: Value<'_>) -> Option<OwnedValue> {
    OwnedValue::try_from(value).ok()
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.shared.dispatch(MediaAction::Next);
    }
    fn previous(&self) {
        self.shared.dispatch(MediaAction::Previous);
    }
    fn pause(&self) {
        self.shared.dispatch(MediaAction::Pause);
    }
    fn play_pause(&self) {
        self.shared.dispatch(MediaAction::Toggle);
    }
    fn stop(&self) {
        self.shared.dispatch(MediaAction::Stop);
    }
    fn play(&self) {
        self.shared.dispatch(MediaAction::Play);
    }
    fn seek(&self, offset: i64) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        if !state.has_track {
            return;
        }
        let target = (state.position() + offset).max(0);
        let length = state.track.length_us;
        drop(state);
        if length > 0 && target >= length {
            self.shared.dispatch(MediaAction::Next);
        } else {
            self.shared
                .dispatch(MediaAction::Seek(target as f64 / 1_000_000.0));
        }
    }
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        let current = track_path(state.track.id);
        let length = state.track.length_us;
        drop(state);
        if track_id.as_str() == current.as_str() && position >= 0 && position <= length {
            self.shared
                .dispatch(MediaAction::Seek(position as f64 / 1_000_000.0));
        }
    }
    fn open_uri(&self, _uri: &str) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        match self.state.lock() {
            Ok(state) if state.has_track && state.playing => "Playing",
            Ok(state) if state.has_track => "Paused",
            _ => "Stopped",
        }
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let Ok(state) = self.state.lock() else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        if !state.has_track {
            if let Some(value) = owned(Value::from(
                ObjectPath::try_from(NO_TRACK).expect("static path"),
            )) {
                map.insert("mpris:trackid".into(), value);
            }
            return map;
        }
        let track = &state.track;
        let entries = [
            (
                "mpris:trackid",
                Value::from(track_path(track.id).into_inner()),
            ),
            ("xesam:title", Value::from(track.title.clone())),
            ("xesam:artist", Value::from(vec![track.artist.clone()])),
            ("xesam:album", Value::from(track.album.clone())),
        ];
        for (key, value) in entries {
            if let Some(value) = owned(value) {
                map.insert(key.into(), value);
            }
        }
        if track.length_us > 0 {
            if let Some(value) = owned(Value::from(track.length_us)) {
                map.insert("mpris:length".into(), value);
            }
        }
        if !track.art_url.is_empty() {
            if let Some(value) = owned(Value::from(track.art_url.clone())) {
                map.insert("mpris:artUrl".into(), value);
            }
        }
        map
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.state.lock().map(|state| state.position()).unwrap_or(0)
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.has_track && state.track.length_us > 0)
    }
    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}

type SharedState = Arc<Mutex<PlayerState>>;

/// Register the MPRIS object and name. Fast and local, so it runs inline.
fn connect(shared: Arc<Shared>) -> Result<(zbus::blocking::Connection, SharedState), String> {
    let state = Arc::new(Mutex::new(PlayerState {
        track: Track::default(),
        playing: false,
        has_track: false,
        position_us: 0,
        at: Instant::now(),
    }));
    let conn = zbus::blocking::connection::Builder::session()
        .map_err(|e| e.to_string())?
        .serve_at(
            OBJECT,
            Root {
                shared: shared.clone(),
            },
        )
        .map_err(|e| e.to_string())?
        .serve_at(
            OBJECT,
            Player {
                shared,
                state: state.clone(),
            },
        )
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    // A second running copy (for example a portable build beside an installed
    // one) still gets its own, distinct MPRIS name.
    if conn.request_name("org.mpris.MediaPlayer2.tide").is_err() {
        conn.request_name(format!(
            "org.mpris.MediaPlayer2.tide.instance{}",
            std::process::id()
        ))
        .map_err(|e| e.to_string())?;
    }
    Ok((conn, state))
}

fn serve(
    conn: zbus::blocking::Connection,
    state: SharedState,
    updates: mpsc::Receiver<Update>,
) -> Result<(), String> {
    let player = conn
        .object_server()
        .interface::<_, Player>(OBJECT)
        .map_err(|e| e.to_string())?;
    let emitter = player.signal_emitter().clone();
    while let Ok(update) = updates.recv() {
        let iface = player.get();
        match update {
            Update::Track(track) => {
                if let Ok(mut s) = state.lock() {
                    s.has_track = !track.title.is_empty();
                    s.track = track;
                    s.position_us = 0;
                    s.at = Instant::now();
                }
                let _ = zbus::block_on(iface.metadata_changed(&emitter));
                let _ = zbus::block_on(iface.can_seek_changed(&emitter));
                let _ = zbus::block_on(iface.playback_status_changed(&emitter));
            }
            Update::Status(playing) => {
                if let Ok(mut s) = state.lock() {
                    s.position_us = s.position();
                    s.at = Instant::now();
                    s.playing = playing;
                }
                let _ = zbus::block_on(iface.playback_status_changed(&emitter));
            }
            Update::Seeked(position) => {
                if let Ok(mut s) = state.lock() {
                    s.position_us = position;
                    s.at = Instant::now();
                }
                let _ = zbus::block_on(Player::seeked(&emitter, position));
            }
        }
    }
    Ok(())
}

struct TideTray {
    shared: Arc<Shared>,
    icons: Vec<ksni::Icon>,
    song: String,
}

impl ksni::Tray for TideTray {
    fn id(&self) -> String {
        "tide-music".into()
    }
    fn title(&self) -> String {
        "Tide Music".into()
    }
    fn icon_name(&self) -> String {
        "tide-music".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Tide Music".into(),
            description: self.song.clone(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.shared.show_player();
    }
    fn watcher_online(&self) {
        self.shared.tray_ready.store(true, Ordering::Release);
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // Keep the service: the panel may return. Until then closing quits.
        self.shared.tray_ready.store(false, Ordering::Release);
        true
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let item = |label: &str, run: fn(&mut Self)| {
            StandardItem {
                label: label.into(),
                activate: Box::new(run),
                ..Default::default()
            }
            .into()
        };
        let playing = self.shared.playing.load(Ordering::Acquire);
        vec![
            item("Open Tide", |t| t.shared.show_player()),
            item("Open Mini player", |t| {
                t.shared.show_player();
                t.shared.dispatch(MediaAction::ShowMini);
            }),
            item(if playing { "Pause" } else { "Play" }, |t| {
                t.shared.dispatch(MediaAction::Toggle)
            }),
            item("Previous song", |t| {
                t.shared.dispatch(MediaAction::Previous)
            }),
            item("Next song", |t| t.shared.dispatch(MediaAction::Next)),
            item("Minimize to tray", |t| t.shared.hide_player()),
            item("Quit Tide", |t| t.shared.quit()),
        ]
    }
}

/// ARGB32 (network byte order) tray pixmaps decoded from the app icon.
fn tray_icons() -> Vec<ksni::Icon> {
    let Ok(image) = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png) else {
        return Vec::new();
    };
    [22u32, 32, 48, 64]
        .into_iter()
        .map(|size| {
            let rgba = image
                .resize_exact(size, size, image::imageops::FilterType::Triangle)
                .to_rgba8();
            let data = rgba
                .pixels()
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect();
            ksni::Icon {
                width: size as i32,
                height: size as i32,
                data,
            }
        })
        .collect()
}

/// Owns the MPRIS name, tray icon and artwork file. Keep on the UI thread.
pub struct MediaSession {
    shared: Arc<Shared>,
    updates: Option<mpsc::Sender<Update>>,
    worker: Option<JoinHandle<()>>,
    tray: Option<ksni::blocking::Handle<TideTray>>,
    track_cache: Option<Track>,
    status_cache: Option<bool>,
    expected: Option<(f64, Instant)>,
    art_url: String,
    art_file: Option<std::path::PathBuf>,
}

pub fn start(
    view: *mut c_void,
    callback: impl Fn(MediaAction) + Send + 'static,
) -> Result<MediaSession, String> {
    let window = x11::window(view).ok();
    let shared = Arc::new(Shared {
        window,
        callback: Arc::new(Mutex::new(Box::new(callback))),
        close_to_tray: AtomicBool::new(false),
        tray_ready: AtomicBool::new(false),
        quit: AtomicBool::new(false),
        playing: AtomicBool::new(false),
        enabled: AtomicBool::new(true),
    });
    let (conn, state) = connect(shared.clone())
        .map_err(|error| format!("Desktop media controls are unavailable: {error}"))?;
    let (send, receive) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("tide-mpris".into())
        .spawn(move || {
            let _ = serve(conn, state, receive);
        })
        .map_err(|e| e.to_string())?;
    // The tray needs a window to show or hide; without one only MPRIS runs.
    let tray = window.and_then(|_| {
        use ksni::blocking::TrayMethods;
        TideTray {
            shared: shared.clone(),
            icons: tray_icons(),
            song: String::new(),
        }
        .assume_sni_available(true)
        .spawn()
        .ok()
    });
    Ok(MediaSession {
        shared,
        updates: Some(send),
        worker: Some(worker),
        tray,
        track_cache: None,
        status_cache: None,
        expected: None,
        art_url: String::new(),
        art_file: None,
    })
}

static ART_COUNTER: AtomicU64 = AtomicU64::new(0);

impl MediaSession {
    fn send(&self, update: Update) {
        if let Some(updates) = &self.updates {
            let _ = updates.send(update);
        }
    }

    pub fn set_close_to_tray(&self, enabled: bool) {
        self.shared.close_to_tray.store(enabled, Ordering::Release);
    }

    /// The window close button hides Tide to the tray when "keep playing" is
    /// on and a tray is visible, like the Windows notification-area icon.
    pub fn hide_to_tray(&self) -> bool {
        if self.quit_requested()
            || !self.shared.close_to_tray.load(Ordering::Acquire)
            || !self.shared.tray_ready.load(Ordering::Acquire)
            || self.tray.is_none()
        {
            return false;
        }
        self.shared.hide_player();
        true
    }

    /// True once "Quit Tide" was chosen from the tray or MPRIS.
    pub fn quit_requested(&self) -> bool {
        self.shared.quit.load(Ordering::Acquire)
    }

    /// Publish a bounded, already decoded/re-encoded PNG. No URL, file, or network access.
    pub fn set_thumbnail(&mut self, png: Option<&[u8]>) -> Result<(), String> {
        let url = match png {
            Some(png) => {
                if png.len() < 33
                    || png.len() > 128 * 1024
                    || !png.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
                    || ![16..20, 20..24].into_iter().all(|range| {
                        let side =
                            u32::from_be_bytes(png[range].try_into().expect("header size checked"));
                        (1..=160).contains(&side)
                    })
                {
                    return Err("System artwork must be a small PNG thumbnail".into());
                }
                let dir = artwork_dir()?;
                // A fresh name per song makes desktop shells reload the image.
                let path = dir.join(format!(
                    "artwork-{}.png",
                    ART_COUNTER.fetch_add(1, Ordering::Relaxed)
                ));
                write_private(&path, png)?;
                if let Some(old) = self.art_file.replace(path.clone()) {
                    let _ = std::fs::remove_file(old);
                }
                file_url(&path)
            }
            None => {
                if let Some(old) = self.art_file.take() {
                    let _ = std::fs::remove_file(old);
                }
                String::new()
            }
        };
        if url != self.art_url {
            self.art_url = url;
            if let Some(mut track) = self.track_cache.clone() {
                track.art_url = self.art_url.clone();
                self.track_cache = Some(track.clone());
                self.send(Update::Track(track));
            }
        }
        Ok(())
    }

    pub fn update(
        &mut self,
        title: &str,
        artist: &str,
        album: &str,
        playing: bool,
        position: f64,
        duration: f64,
    ) -> Result<(), String> {
        if !position.is_finite() || !duration.is_finite() || position < 0.0 || duration < 0.0 {
            return Err("Invalid media timeline".into());
        }
        self.shared.playing.store(playing, Ordering::Release);
        let length_us = (duration * 1_000_000.0) as i64;
        let changed = self.track_cache.as_ref().is_none_or(|cached| {
            cached.title != title
                || cached.artist != artist
                || cached.album != album
                || cached.length_us != length_us
        });
        if changed {
            let same_song = self
                .track_cache
                .as_ref()
                .is_some_and(|cached| cached.title == title && cached.artist == artist);
            let id = match &self.track_cache {
                Some(cached) if same_song => cached.id,
                Some(cached) => cached.id + 1,
                None => 1,
            };
            let track = Track {
                id,
                title: title.into(),
                artist: artist.into(),
                album: album.into(),
                length_us,
                art_url: self.art_url.clone(),
            };
            self.track_cache = Some(track.clone());
            self.send(Update::Track(track));
            self.status_cache = None;
            self.expected = None;
            if let Some(tray) = &self.tray {
                let song = if artist.is_empty() {
                    title.to_owned()
                } else {
                    format!("{title} — {artist}")
                };
                tray.update(move |t| t.song = song);
            }
        }
        if self.status_cache != Some(playing) {
            self.status_cache = Some(playing);
            self.send(Update::Status(playing));
            if let Some(tray) = &self.tray {
                // Rebuild the menu so its Play/Pause label matches.
                tray.update(|_| {});
            }
        }
        // Report jumps (seeks, restarts) so desktop progress bars resync.
        let now = Instant::now();
        let predicted = self.expected.map(|(base, at)| {
            base + if playing {
                now.duration_since(at).as_secs_f64()
            } else {
                0.0
            }
        });
        if predicted.is_none_or(|predicted| (predicted - position).abs() > 1.5) {
            self.send(Update::Seeked((position * 1_000_000.0) as i64));
        }
        self.expected = Some((position, now));
        Ok(())
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        self.shared.enabled.store(false, Ordering::Release);
        if let Some(tray) = self.tray.take() {
            tray.shutdown().wait();
        }
        // Closing the channel ends the MPRIS thread and releases the bus name.
        self.updates.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(file) = self.art_file.take() {
            let _ = std::fs::remove_file(file);
        }
    }
}

fn artwork_dir() -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("tide-music-{}", unsafe { libc::getuid() }));
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&dir);
    let meta = std::fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
        return Err("Artwork directory is not private".into());
    }
    Ok(dir)
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

pub(crate) fn file_url(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut url = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                url.push(*byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_escape_reserved_bytes() {
        assert_eq!(
            file_url(std::path::Path::new("/run/user/1000/a b#?.png")),
            "file:///run/user/1000/a%20b%23%3F.png"
        );
    }

    #[test]
    fn tray_icons_decode_to_argb() {
        let icons = tray_icons();
        assert_eq!(icons.len(), 4);
        for icon in icons {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        }
    }

    #[test]
    fn positions_extrapolate_only_while_playing_and_clamp_to_length() {
        let mut state = PlayerState {
            track: Track {
                length_us: 2_000_000,
                ..Default::default()
            },
            playing: false,
            has_track: true,
            position_us: 1_500_000,
            at: Instant::now() - std::time::Duration::from_secs(5),
        };
        assert_eq!(state.position(), 1_500_000);
        state.playing = true;
        assert_eq!(state.position(), 2_000_000);
    }
}
