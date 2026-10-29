use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tide_core::Track;
use zeroize::Zeroize;
mod feedback;
pub use feedback::{FeedbackOperation, FeedbackRequest, FeedbackResult, PlaylistDetails, Rating};

const MAX_SESSION: usize = 65536;
// Local encrypted metadata, removed before any headers reach a helper or HTTP.
const CONNECTION_REVISION: &str = "tide-connection-revision";

/// Opaque login generation and selected identity; contains no credentials.
/// A normal cookie refresh keeps this generation, while explicit imports
/// create a new one. Legacy sessions have no generation until reimported.
#[derive(Clone, PartialEq, Eq)]
pub struct SavedConnection {
    pub revision: Option<String>,
    pub account_index: u32,
    pub page_id: Option<String>,
}
impl SavedConnection {
    fn from_headers(headers: &BTreeMap<String, String>, revision: Option<String>) -> Self {
        Self {
            revision,
            account_index: headers
                .get("x-goog-authuser")
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|index| *index < 10)
                .unwrap_or(0),
            page_id: headers.get("x-goog-pageid").cloned(),
        }
    }
}

/// Fixed, credential-free outcomes for browser-assisted session imports.
#[derive(Debug, PartialEq, Eq)]
pub enum SessionImportError {
    InvalidHeaders,
    SessionRejected,
    VerificationUnavailable,
    SessionChanged,
    StorageError,
    Cancelled,
}
static LEASE_COUNTER: AtomicU64 = AtomicU64::new(1);

// Serialize read/compare/write across Tide instances. The Windows mutex is
// recursive, so guarded merges can reuse the ordinary protected import path.
#[cfg(windows)]
struct SessionWriteLock(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl SessionWriteLock {
    fn acquire() -> Result<Self, &'static str> {
        Self::acquire_with_timeout(1000)
    }
    fn try_acquire() -> Result<Self, &'static str> {
        Self::acquire_with_timeout(0)
    }
    fn acquire_with_timeout(timeout_ms: u32) -> Result<Self, &'static str> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0},
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };
        let name: Vec<u16> = "Local\\TideMusicSessionStorage"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: terminated name and a uniquely owned handle; no raw data is logged.
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err("Account storage lock unavailable");
            }
            match WaitForSingleObject(handle, timeout_ms) {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
                _ => {
                    CloseHandle(handle);
                    Err("Account storage is busy; retry the request")
                }
            }
        }
    }
}
#[cfg(windows)]
impl Drop for SessionWriteLock {
    fn drop(&mut self) {
        // SAFETY: this thread acquired this unique handle exactly once.
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0);
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
struct SessionWriteLock;
#[cfg(not(windows))]
impl SessionWriteLock {
    fn acquire() -> Result<Self, &'static str> {
        Ok(Self)
    }
    fn try_acquire() -> Result<Self, &'static str> {
        Ok(Self)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub owned: bool,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Card {
    #[serde(default)]
    pub duration_seconds: Option<u64>,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub shelf: String,
    #[serde(default)]
    pub artists: Vec<tide_core::Artist>,
    #[serde(default)]
    pub artwork_url: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum Request {
    Connect {
        database: PathBuf,
        index: u32,
    },
    SwitchAccount {
        index: u32,
    },
    Library,
    Playlist {
        id: String,
    },
    Radio {
        video_id: String,
    },
    Liked,
    Supermix,
    Home,
    Artist {
        id: String,
    },
    Artists {
        video_id: String,
    },
    Lyrics {
        video_id: String,
    },
    Feedback {
        request: FeedbackRequest,
        #[serde(skip)]
        expected: SavedConnection,
    },
}
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct LyricLine {
    pub text: String,
    pub start_ms: Option<u64>,
    #[serde(default)]
    pub end_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Response {
    Connected,
    Lyrics {
        lines: Vec<LyricLine>,
        synced: bool,
        source: String,
    },
    Artist {
        name: String,
        tracks: Vec<Track>,
        truncated: bool,
    },
    Artists {
        artists: Vec<tide_core::Artist>,
    },
    Session {
        headers: BTreeMap<String, String>,
    },
    Library {
        account: String,
        playlists: Vec<Playlist>,
        truncated: bool,
    },
    Tracks {
        tracks: Vec<Track>,
        truncated: bool,
        #[serde(default)]
        playlist: Option<PlaylistDetails>,
    },
    Feedback {
        result: FeedbackResult,
    },
    Home {
        cards: Vec<Card>,
    },
    Error {
        code: String,
    },
}

// Keep refreshed credentials inside the provider; callers receive metadata only.
struct HelperReply {
    response: Response,
    refreshed: RefreshedHeaders,
}
#[derive(Default)]
struct RefreshedHeaders(Option<BTreeMap<String, String>>);
impl Drop for RefreshedHeaders {
    fn drop(&mut self) {
        if let Some(headers) = &mut self.0 {
            for value in headers.values_mut() {
                value.zeroize();
            }
        }
    }
}

// Deserialize only the optional credentials a second time. Serde skips the
// already-decoded track/card metadata without allocating another JSON tree.
#[derive(Default, Deserialize)]
#[serde(transparent)]
struct RefreshedJson(serde_json::Value);
impl Drop for RefreshedJson {
    fn drop(&mut self) {
        zeroize_json(&mut self.0);
    }
}
#[derive(Deserialize)]
struct RefreshEnvelope {
    #[serde(default)]
    refreshed_headers: RefreshedJson,
}

fn decode_helper_reply(bytes: &[u8]) -> Result<HelperReply, &'static str> {
    // Parse data independently so an invalid optional update cannot hide valid
    // library results or turn a transient error into a rejected session.
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|_| "Account helper returned an unsupported response")?;
    let mut refreshed = RefreshedHeaders::default();
    if matches!(
        response,
        Response::Library { .. }
            | Response::Tracks { .. }
            | Response::Home { .. }
            | Response::Artist { .. }
            | Response::Artists { .. }
            | Response::Lyrics { .. }
            | Response::Feedback { .. }
    ) || matches!(&response, Response::Error { code } if code.starts_with("feedback_") || code == "lyrics_unavailable")
    {
        if let Ok(document) = serde_json::from_slice::<RefreshEnvelope>(bytes) {
            let mut encoded = serde_json::to_vec(&document.refreshed_headers.0).unwrap_or_default();
            if encoded.len() <= MAX_SESSION {
                refreshed.0 = parse_headers(&encoded).ok();
            }
            encoded.zeroize();
        }
    }
    Ok(HelperReply {
        response,
        refreshed,
    })
}

fn zeroize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(value) => value.zeroize(),
        serde_json::Value::Array(values) => values.iter_mut().for_each(zeroize_json),
        serde_json::Value::Object(values) => values.values_mut().for_each(zeroize_json),
        _ => {}
    }
}

#[derive(Clone)]
pub struct Provider {
    directory: PathBuf,
}
pub struct CookieLease {
    path: PathBuf,
}
impl CookieLease {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for CookieLease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
struct LoginDirectory {
    path: PathBuf,
    parent: PathBuf,
}
#[cfg(test)]
impl Drop for LoginDirectory {
    fn drop(&mut self) {
        remove_private_login_directory(&self.path, &self.parent);
    }
}
fn remove_private_login_directory(path: &Path, parent: &Path) {
    // Resolve both before any recursive deletion. Reject links outside the
    // designated account directory and remove only Tide's own login root.
    let (Ok(target), Ok(root)) = (path.canonicalize(), parent.canonicalize()) else {
        return;
    };
    if target.parent() != Some(root.as_path())
        || !target
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("login-"))
    {
        return;
    }
    for _ in 0..5 {
        if fs::remove_dir_all(&target).is_ok() || !target.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

impl Provider {
    pub fn new() -> Result<Self, String> {
        let directory = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or("Windows application-data directory is unavailable")?
            .join("TideMusic/account");
        private_directory(&directory)?;
        remove_stale_leases(&directory);
        Ok(Self { directory })
    }
    pub fn is_connected(&self) -> bool {
        self.directory.join("session.dpapi").is_file()
    }
    pub fn account_index(&self) -> u32 {
        let Ok(mut headers) = self.headers() else {
            return 0;
        };
        let index = headers
            .get("x-goog-authuser")
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value < 10)
            .unwrap_or(0);
        for value in headers.values_mut() {
            value.zeroize();
        }
        index
    }
    fn save_verified_session(
        &self,
        mut headers: BTreeMap<String, String>,
        cancel: &AtomicBool,
    ) -> Result<Response, String> {
        let encoded = serde_json::to_vec(&headers);
        for value in headers.values_mut() {
            value.zeroize();
        }
        let mut encoded = encoded.map_err(|_| "Could not prepare account session")?;
        let result = if cancel.load(Ordering::Acquire) {
            Err("Account connection cancelled".into())
        } else if encoded.len() > MAX_SESSION {
            Err("Account session exceeds the input limit".into())
        } else {
            self.import_bytes(&encoded)
        };
        encoded.zeroize();
        result?;
        Ok(Response::Connected)
    }
    #[cfg(windows)]
    fn save_verified_session_for_connection(
        &self,
        mut headers: BTreeMap<String, String>,
        cancel: &AtomicBool,
        expected: &Option<SavedConnection>,
    ) -> Result<Response, String> {
        let lock = SessionWriteLock::acquire().map_err(str::to_string);
        let current = lock
            .as_ref()
            .map_err(|error| error.clone())
            .and_then(|_| self.saved_connection());
        if current.as_ref().ok() != Some(expected) {
            for value in headers.values_mut() {
                value.zeroize();
            }
            return Err("Saved account changed during connection; retry the request".into());
        }
        self.save_verified_session(headers, cancel)
    }
    pub fn import_session(&self, path: &Path) -> Result<(), String> {
        let mut bytes = read_limited(path, MAX_SESSION)?;
        let result = self.import_bytes(&bytes);
        bytes.zeroize();
        result
    }

    /// Verify before replacing a saved session. A connection changed while the
    /// network request was in flight must not be overwritten by its result.
    #[cfg(windows)]
    pub fn import_browser_session(
        &self,
        bytes: &[u8],
        cancel: Arc<AtomicBool>,
    ) -> Result<(), SessionImportError> {
        if bytes.len() > MAX_SESSION {
            return Err(SessionImportError::InvalidHeaders);
        }
        let baseline = self.import_baseline()?;
        let mut headers = parse_headers(bytes).map_err(|_| SessionImportError::InvalidHeaders)?;
        #[derive(Serialize)]
        struct ImportPayload<'a> {
            action: &'static str,
            headers: &'a BTreeMap<String, String>,
        }
        let encoded = serde_json::to_vec(&ImportPayload {
            action: "verify_import",
            headers: &headers,
        });
        for value in headers.values_mut() {
            value.zeroize();
        }
        let mut input = encoded.map_err(|_| SessionImportError::InvalidHeaders)?;
        if input.len() > MAX_SESSION {
            input.zeroize();
            return Err(SessionImportError::InvalidHeaders);
        }
        let result = execute_helper(&input, cancel.clone(), timeout);
        input.zeroize();
        match result
            .map_err(|_| {
                if cancel.load(Ordering::Acquire) {
                    SessionImportError::Cancelled
                } else {
                    SessionImportError::VerificationUnavailable
                }
            })?
            .response
        {
            Response::Session { headers } => {
                self.commit_browser_session(headers, baseline, &cancel)
            }
            Response::Error { code } if code == "session" => {
                Err(SessionImportError::SessionRejected)
            }
            _ => Err(SessionImportError::VerificationUnavailable),
        }
    }

    #[cfg(not(windows))]
    pub fn import_browser_session(
        &self,
        _bytes: &[u8],
        _cancel: Arc<AtomicBool>,
    ) -> Result<(), SessionImportError> {
        Err(SessionImportError::StorageError)
    }

    #[cfg(windows)]
    fn import_baseline(&self) -> Result<Option<Vec<u8>>, SessionImportError> {
        let _lock = SessionWriteLock::acquire().map_err(|_| SessionImportError::StorageError)?;
        let path = self.directory.join("session.dpapi");
        match fs::metadata(&path) {
            Ok(_) => read_limited(&path, MAX_SESSION + 8192)
                .map(Some)
                .map_err(|_| SessionImportError::StorageError),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(SessionImportError::StorageError),
        }
    }

    #[cfg(windows)]
    fn commit_browser_session(
        &self,
        mut headers: BTreeMap<String, String>,
        baseline: Option<Vec<u8>>,
        cancel: &AtomicBool,
    ) -> Result<(), SessionImportError> {
        let encoded = serde_json::to_vec(&headers);
        for value in headers.values_mut() {
            value.zeroize();
        }
        let mut encoded = encoded.map_err(|_| SessionImportError::InvalidHeaders)?;
        let result = (|| {
            let _lock =
                SessionWriteLock::acquire().map_err(|_| SessionImportError::StorageError)?;
            if cancel.load(Ordering::Acquire) {
                return Err(SessionImportError::Cancelled);
            }
            if self.import_baseline()? != baseline {
                return Err(SessionImportError::SessionChanged);
            }
            self.import_bytes(&encoded)
                .map_err(|_| SessionImportError::StorageError)
        })();
        encoded.zeroize();
        result
    }
    fn import_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        self.write_session(bytes, true)
    }
    fn write_session(&self, bytes: &[u8], new_connection: bool) -> Result<(), String> {
        let _session_lock = SessionWriteLock::acquire().map_err(str::to_string)?;
        (|| {
            let mut headers = parse_headers(bytes)?;
            let revision = if new_connection {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                Some(format!(
                    "v1-{timestamp}-{}-{}",
                    std::process::id(),
                    LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
                ))
            } else {
                self.saved_connection()
                    .map_err(|_| "Saved account changed during refresh")?
                    .and_then(|connection| connection.revision)
            };
            if let Some(revision) = revision {
                headers.insert(CONNECTION_REVISION.into(), revision);
            }
            let mut clear = serde_json::to_vec(&headers).map_err(|_| "Could not encode session")?;
            for value in headers.values_mut() {
                value.zeroize();
            }
            let encrypted = protect(&clear);
            clear.zeroize();
            let encrypted = encrypted?;
            let temporary = self.directory.join("session.pending");
            fs::write(&temporary, encrypted).map_err(|_| "Could not save protected session")?;
            // Rename in the same protected directory replaces the destination
            // atomically; keep the previous session if replacement fails.
            let destination = self.directory.join("session.dpapi");
            fs::rename(temporary, destination).map_err(|_| "Could not finish session import")
        })()
        .map_err(str::to_string)
    }
    pub fn disconnect(&self) -> Result<(), String> {
        remove_stale_leases(&self.directory);
        // Use the same cross-process guard as imports and refreshed-cookie
        // commits. Otherwise an import can pass its baseline check, race a
        // disconnect, and recreate the session the user just removed.
        let _session_lock = SessionWriteLock::acquire().map_err(str::to_string)?;
        let path = self.directory.join("session.dpapi");
        if path.exists() {
            fs::remove_file(path).map_err(|_| "Could not remove the saved session")?;
        }
        Ok(())
    }
    fn headers(&self) -> Result<BTreeMap<String, String>, String> {
        let (headers, _) = self.read_session()?;
        Ok(headers)
    }
    fn read_session(&self) -> Result<(BTreeMap<String, String>, Option<String>), String> {
        let encrypted = read_limited(&self.directory.join("session.dpapi"), MAX_SESSION + 8192)?;
        let mut clear = unprotect(&encrypted).map_err(str::to_string)?;
        let headers: Result<BTreeMap<String, String>, String> = serde_json::from_slice(&clear)
            .map_err(|_| "Saved session is invalid. Reconnect your account".to_string());
        clear.zeroize();
        let mut headers = headers?;
        let revision = headers.remove(CONNECTION_REVISION);
        if revision.as_ref().is_some_and(|value| {
            let parts: Vec<_> = value.split('-').collect();
            value.len() > 100
                || parts.len() != 4
                || parts[0] != "v1"
                || parts[1..]
                    .iter()
                    .any(|part| part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()))
        }) {
            for value in headers.values_mut() {
                value.zeroize();
            }
            return Err("Saved connection metadata is invalid".into());
        }
        Ok((headers, revision))
    }
    /// Read only a credential-free descriptor for application adoption checks.
    pub fn saved_connection(&self) -> Result<Option<SavedConnection>, String> {
        if !self.is_connected() {
            return Ok(None);
        }
        let (mut headers, revision) = self.read_session()?;
        let connection = SavedConnection::from_headers(&headers, revision);
        for value in headers.values_mut() {
            value.zeroize();
        }
        Ok(Some(connection))
    }
    /// Capture identity and playback cookies under the same session guard.
    pub fn playback_connection(
        &self,
    ) -> Result<(Option<SavedConnection>, Option<CookieLease>), String> {
        let _guard = SessionWriteLock::try_acquire()?;
        Ok((self.saved_connection()?, self.cookie_lease()?))
    }
    pub fn cookie_lease(&self) -> Result<Option<CookieLease>, String> {
        if !self.is_connected() {
            return Ok(None);
        }
        let mut headers = self.headers()?;
        let mut jar = cookie_jar(headers.get("cookie").ok_or("Saved session has no cookie")?)?;
        for value in headers.values_mut() {
            value.zeroize();
        }
        let name = format!(
            "playback-{}-{}.txt",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = self.directory.join(name);
        let mut created = false;
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| "Could not prepare signed-in playback")?;
            created = true;
            file.write_all(jar.as_bytes())
                .map_err(|_| "Could not prepare signed-in playback")?;
            Ok(Some(CookieLease { path: path.clone() }))
        })();
        jar.zeroize();
        if result.is_err() && created {
            let _ = fs::remove_file(path);
        }
        result.map_err(str::to_string)
    }

    pub fn refresh_cookie_lease(&self, lease: &CookieLease) -> Result<(), String> {
        let root = self
            .directory
            .canonicalize()
            .map_err(|_| "Account storage unavailable")?;
        let path = lease
            .path
            .canonicalize()
            .map_err(|_| "Playback cookie lease unavailable")?;
        if lease.path.parent() != Some(self.directory.as_path())
            || path.parent() != Some(root.as_path())
            || fs::symlink_metadata(&lease.path)
                .map_err(|_| "Playback cookie lease unavailable")?
                .file_type()
                .is_symlink()
            || !path.is_file()
            || !path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with(&format!("playback-{}-", std::process::id()))
                    && name.ends_with(".txt")
            })
        {
            return Err("Playback cookie lease is outside account storage".into());
        }
        let mut headers = self.headers()?;
        let jar = headers
            .get("cookie")
            .ok_or("Saved session has no cookie")
            .and_then(|cookie| cookie_jar(cookie));
        for value in headers.values_mut() {
            value.zeroize();
        }
        let mut jar = jar.map_err(str::to_string)?;
        let temporary = self.directory.join(format!(
            "playback-{}-{}.pending",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| "Could not update signed-in playback")?;
            file.write_all(jar.as_bytes())
                .map_err(|_| "Could not update signed-in playback")?;
            file.sync_all()
                .map_err(|_| "Could not update signed-in playback")?;
            drop(file);
            fs::rename(&temporary, &lease.path).map_err(|_| "Could not update signed-in playback")
        })();
        jar.zeroize();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(str::to_string)
    }

    /// Adopt only the exact connection observed by the application. An import
    /// racing this refresh must not pair another identity's cookies with it.
    pub fn refresh_cookie_lease_for_connection(
        &self,
        lease: &CookieLease,
        expected: &SavedConnection,
    ) -> Result<(), String> {
        let _session_lock = SessionWriteLock::try_acquire().map_err(str::to_string)?;
        if self.saved_connection()?.as_ref() != Some(expected) {
            return Err("Saved connection changed; retry adoption".into());
        }
        self.refresh_cookie_lease(lease)
    }

    /// Merge only scoped extractor changes based on the still-current session.
    /// Missing rows are not deletions: extractors may omit expired cookies.
    pub fn merge_playback_cookies(
        &self,
        account_index: u32,
        baseline: &[u8],
        updated: &[u8],
    ) -> Result<bool, String> {
        let _session_lock = SessionWriteLock::acquire().map_err(str::to_string)?;
        let (Ok(before), Ok(after)) = (
            scoped_cookie_snapshot(baseline),
            scoped_cookie_snapshot(updated),
        ) else {
            return Ok(false);
        };
        let mut protected = RefreshedHeaders(Some(self.headers()?));
        let headers = protected.0.as_mut().ok_or("Saved session unavailable")?;
        if headers
            .get("x-goog-authuser")
            .and_then(|v| v.parse::<u32>().ok())
            != Some(account_index)
        {
            return Ok(false);
        }
        let current_jar = headers
            .get("cookie")
            .ok_or("Saved session unavailable")
            .and_then(|v| cookie_jar(v))
            .map_err(str::to_string)?;
        let mut current_jar = zeroize::Zeroizing::new(current_jar);
        let mut current = scoped_cookie_snapshot(current_jar.as_bytes()).map_err(str::to_string)?;
        current_jar.zeroize();
        let signing = "__Secure-3PAPISID";
        if !before.0.contains_key(signing) || before.0.get(signing) != current.0.get(signing) {
            return Ok(false);
        }
        let auth_changed = after
            .0
            .iter()
            .any(|(name, value)| authentication_cookie(name) && before.0.get(name) != Some(value));
        // Authentication cookies rotate as a cohort. Reject a stale cohort
        // rather than combining independently refreshed account credentials.
        if auth_changed
            && before.0.iter().chain(current.0.iter()).any(|(name, _)| {
                authentication_cookie(name) && before.0.get(name) != current.0.get(name)
            })
        {
            return Ok(false);
        }
        let mut changed = false;
        for (name, value) in &after.0 {
            if before.0.get(name) != Some(value) && current.0.get(name) == before.0.get(name) {
                if let Some(mut old) = current.0.insert(name.clone(), value.clone()) {
                    old.zeroize();
                }
                changed = true;
            }
        }
        if !changed {
            return Ok(false);
        }
        let mut cookie = String::new();
        for (name, value) in &current.0 {
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(name);
            cookie.push('=');
            cookie.push_str(value);
        }
        if cookie.len() > 32768 {
            cookie.zeroize();
            return Err("Playback session exceeds the size limit".into());
        }
        if let Some(mut old) = headers.insert("cookie".into(), cookie) {
            old.zeroize();
        }
        let mut encoded =
            serde_json::to_vec(headers).map_err(|_| "Could not prepare playback session")?;
        let result = self.write_session(&encoded, false);
        encoded.zeroize();
        result?;
        Ok(true)
    }

    fn persist_refreshed_headers(
        &self,
        refreshed: &RefreshedHeaders,
        expected_authuser: Option<&str>,
        expected_pageid: Option<&str>,
        expected_cookie: &str,
        cancel: &AtomicBool,
    ) -> bool {
        let Some(candidate) = &refreshed.0 else {
            return false;
        };
        let Ok(_session_lock) = SessionWriteLock::acquire() else {
            return false;
        };
        if cancel.load(Ordering::Acquire)
            || candidate.get("x-goog-authuser").map(String::as_str) != expected_authuser
            || candidate.get("x-goog-pageid").map(String::as_str) != expected_pageid
        {
            return false;
        }
        let Ok(current_headers) = self.headers() else {
            return false;
        };
        let mut protected = RefreshedHeaders(Some(current_headers));
        let Some(headers) = protected.0.as_mut() else {
            return false;
        };
        if headers.get("x-goog-authuser").map(String::as_str) != expected_authuser
            || headers.get("x-goog-pageid").map(String::as_str) != expected_pageid
        {
            return false;
        }
        let (Ok(before), Ok(after), Ok(mut current)) = (
            scoped_header_cookies(expected_cookie),
            scoped_header_cookies(
                candidate
                    .get("cookie")
                    .map(String::as_str)
                    .unwrap_or_default(),
            ),
            scoped_header_cookies(
                headers
                    .get("cookie")
                    .map(String::as_str)
                    .unwrap_or_default(),
            ),
        ) else {
            return false;
        };
        let signing = "__Secure-3PAPISID";
        if !before.0.contains_key(signing) || before.0.get(signing) != current.0.get(signing) {
            return false;
        }
        let auth_changed = after
            .0
            .iter()
            .any(|(name, value)| authentication_cookie(name) && before.0.get(name) != Some(value));
        // Concurrent non-authentication updates do not invalidate a response's
        // authentication baseline. A newer authentication cohort does.
        if auth_changed
            && before.0.iter().chain(current.0.iter()).any(|(name, _)| {
                authentication_cookie(name) && before.0.get(name) != current.0.get(name)
            })
        {
            return false;
        }
        let mut changed = false;
        for (name, value) in &after.0 {
            // Omitted cookies are preserved: this envelope carries no explicit
            // deletion information. Each non-authentication change has its own CAS.
            if before.0.get(name) != Some(value) && current.0.get(name) == before.0.get(name) {
                if let Some(mut old) = current.0.insert(name.clone(), value.clone()) {
                    old.zeroize();
                }
                changed = true;
            }
        }
        if !changed {
            return false;
        }
        let mut cookie = zeroize::Zeroizing::new(String::new());
        for (name, value) in &current.0 {
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(name);
            cookie.push('=');
            cookie.push_str(value);
        }
        if cookie.len() > 32768 {
            return false;
        }
        if let Some(mut old) = headers.insert("cookie".into(), std::mem::take(&mut *cookie)) {
            old.zeroize();
        }
        // Only the cookie baseline was captured. Preserve current request
        // metadata instead of rolling it back to the helper's earlier snapshot.
        let Ok(mut encoded) = serde_json::to_vec(headers) else {
            return false;
        };
        let accepted = encoded.len() <= MAX_SESSION && self.write_session(&encoded, false).is_ok();
        encoded.zeroize();
        accepted
    }

    #[cfg(windows)]
    pub fn execute(&self, request: Request, cancel: Arc<AtomicBool>) -> Result<Response, String> {
        if let Request::Connect {
            ref database,
            index,
        } = request
        {
            if index > 9 {
                return Err("Choose an account between 1 and 10".into());
            }
            let expected_connection = self.saved_connection()?;
            let input = serde_json::to_vec(
                &serde_json::json!({"action":"connect_existing", "database": database, "index": index}),
            )
            .map_err(|_| "Could not prepare account connection")?;
            return match execute_helper(&input, cancel.clone(), Duration::from_secs(600))?.response
            {
                Response::Session { headers } => self.save_verified_session_for_connection(
                    headers,
                    &cancel,
                    &expected_connection,
                ),
                Response::Error { code } => Ok(Response::Error { code }),
                _ => Err("Account connection did not return a session".into()),
            };
        }
        let switching = matches!(request, Request::SwitchAccount { .. });
        let timeout = if matches!(request, Request::Lyrics { .. }) {
            Duration::from_secs(15)
        } else {
            Duration::from_secs(60)
        };
        if matches!(request, Request::SwitchAccount { index } if index > 9) {
            return Err("Choose an account between 1 and 10".into());
        }
        let (mut headers, revision) = self.read_session()?;
        let expected_connection = SavedConnection::from_headers(&headers, revision);
        if let Request::Feedback { expected, request } = &request {
            if expected != &expected_connection
                || !request.valid()
                || cancel.load(Ordering::Acquire)
            {
                for value in headers.values_mut() {
                    value.zeroize();
                }
                return Err(
                    "Account changed or collection action is invalid; choose the action again"
                        .into(),
                );
            }
        }
        let expected_authuser = headers.get("x-goog-authuser").cloned();
        let expected_pageid = headers.get("x-goog-pageid").cloned();
        let expected_cookie =
            zeroize::Zeroizing::new(headers.get("cookie").cloned().unwrap_or_default());
        #[derive(Serialize)]
        struct Payload<'a> {
            #[serde(flatten)]
            request: Request,
            headers: &'a BTreeMap<String, String>,
        }
        let mut input = serde_json::to_vec(&Payload {
            request,
            headers: &headers,
        })
        .map_err(|_| "Could not prepare library request")?;
        for header in headers.values_mut() {
            header.zeroize();
        }
        if input.len() > MAX_SESSION {
            input.zeroize();
            return Err("Session headers exceed the helper input limit".into());
        }
        let result = execute_helper(&input, cancel.clone(), Duration::from_secs(60));
        input.zeroize();
        let mut reply = result?;
        let _session_lock = SessionWriteLock::acquire().map_err(str::to_string)?;
        if cancel.load(Ordering::Acquire)
            || self.saved_connection()?.as_ref() != Some(&expected_connection)
        {
            if let Response::Session { headers } = &mut reply.response {
                for value in headers.values_mut() {
                    value.zeroize();
                }
            }
            return Err("Saved account changed or request cancelled; refresh to retry".into());
        }
        if !switching {
            self.persist_refreshed_headers(
                &reply.refreshed,
                expected_authuser.as_deref(),
                expected_pageid.as_deref(),
                &expected_cookie,
                &cancel,
            );
        }
        match reply.response {
            Response::Session { headers } if switching => {
                self.save_verified_session(headers, &cancel)
            }
            Response::Session { mut headers } => {
                for value in headers.values_mut() {
                    value.zeroize();
                }
                Err("Unexpected account session response".into())
            }
            response => Ok(response),
        }
    }
    #[cfg(not(windows))]
    pub fn execute(&self, _: Request, _: Arc<AtomicBool>) -> Result<Response, String> {
        Err("Account integration currently requires Windows".into())
    }
}

// Never delete another running Tide instance's lease. A crash is reaped on the
// next startup/disconnect; inaccessible or reused PIDs are conservatively kept.
#[cfg(windows)]
fn remove_stale_leases(directory: &Path) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let login = name.starts_with("login-");
        let Some(body) = (if login {
            name.strip_prefix("login-")
        } else {
            name.strip_prefix("playback-")
                .and_then(|s| s.strip_suffix(".txt"))
        }) else {
            continue;
        };
        let Some((pid, counter)) = body.split_once('-') else {
            continue;
        };
        let (Ok(pid), Ok(_counter)) = (pid.parse::<u32>(), counter.parse::<u64>()) else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        // SAFETY: valid process ID; close exactly the handle obtained here.
        let gone = unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                GetLastError() == ERROR_INVALID_PARAMETER
            } else {
                let exited = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
                CloseHandle(handle);
                exited
            }
        };
        if gone {
            if login {
                remove_private_login_directory(&entry.path(), directory);
            } else {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
#[cfg(not(windows))]
fn remove_stale_leases(_: &Path) {}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|_| "Could not read the local session file")?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the local session file")?;
    if bytes.len() > limit {
        bytes.zeroize();
        return Err("Session file is too large".into());
    }
    Ok(bytes)
}

fn parse_headers(bytes: &[u8]) -> Result<BTreeMap<String, String>, &'static str> {
    if bytes.len() > MAX_SESSION {
        return Err("Account session exceeds the input limit");
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "Session file must be UTF-8 request headers or browser JSON")?;
    let mut imported: BTreeMap<String, String> = if text.trim_start().starts_with('{') {
        serde_json::from_str(text)
            .map_err(|_| "Use browser authentication JSON, not OAuth tokens")?
    } else {
        text.lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(key, value)| (key.trim().to_lowercase(), value.trim().to_string()))
            .collect()
    };
    let mut selected = BTreeMap::new();
    for (key, mut value) in std::mem::take(&mut imported) {
        let key = key.to_ascii_lowercase();
        if [
            "cookie",
            "x-goog-authuser",
            "x-goog-pageid",
            "x-goog-visitor-id",
            "accept-language",
            "user-agent",
        ]
        .contains(&key.as_str())
        {
            let limit = match key.as_str() {
                "cookie" => 32768,
                "x-goog-authuser" => 8,
                "x-goog-pageid" => 128,
                "x-goog-visitor-id" => 4096,
                _ => 1024,
            };
            if value.is_empty()
                || value.len() > limit
                || !value.is_ascii()
                || value.chars().any(char::is_control)
            {
                value.zeroize();
                return Err("Session headers contain invalid characters");
            }
            selected.insert(key, value);
        } else {
            value.zeroize();
        }
    }
    let cookie = selected
        .get("cookie")
        .ok_or("The file needs a Cookie header from a signed-in YouTube Music request")?;
    if !cookie.split(';').any(|part| {
        part.trim()
            .strip_prefix("__Secure-3PAPISID=")
            .is_some_and(|value| !value.is_empty())
    }) {
        return Err("No YouTube Music sign-in cookie found. Follow the connection guide");
    }
    let mut jar = cookie_jar(cookie)?;
    jar.zeroize();
    for name in ["x-goog-authuser", "x-goog-pageid"] {
        if let Some(value) = selected.get(name) {
            if !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("Session account selection is invalid");
            }
        }
    }
    selected.insert("x-origin".into(), "https://music.youtube.com".into());
    // ytmusicapi identifies browser auth from this marker and calculates a
    // fresh SAPISIDHASH from the cookie for every actual request.
    selected.insert("authorization".into(), "SAPISIDHASH".into());
    Ok(selected)
}

fn cookie_jar(cookie: &str) -> Result<String, &'static str> {
    let mut jar = zeroize::Zeroizing::new(String::from("# Netscape HTTP Cookie File\n"));
    for pair in cookie.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            || value.chars().any(char::is_control)
        {
            return Err("Session cookie is malformed");
        }
        jar.push_str(".youtube.com\tTRUE\t/\tTRUE\t0\t");
        jar.push_str(name);
        jar.push('\t');
        jar.push_str(value);
        jar.push('\n');
        if jar.len() > MAX_SESSION {
            jar.zeroize();
            return Err("Playback cookie lease exceeds the size limit");
        }
    }
    Ok(std::mem::take(&mut *jar))
}

fn scoped_header_cookies(cookie: &str) -> Result<ScopedCookies, &'static str> {
    if cookie.len() > 32768 {
        return Err("Session cookie exceeds the size limit");
    }
    let jar = zeroize::Zeroizing::new(cookie_jar(cookie)?);
    scoped_cookie_snapshot(jar.as_bytes())
}

struct ScopedCookies(BTreeMap<String, String>);
impl Drop for ScopedCookies {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}
fn authentication_cookie(name: &str) -> bool {
    name.starts_with("__Secure-")
        || matches!(
            name,
            "SID" | "HSID" | "SSID" | "APISID" | "SAPISID" | "SIDCC" | "LOGIN_INFO"
        )
}
fn scoped_cookie_snapshot(bytes: &[u8]) -> Result<ScopedCookies, &'static str> {
    if bytes.len() > MAX_SESSION {
        return Err("Playback cookie update exceeds the size limit");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Playback cookie update is invalid")?;
    let mut cookies = ScopedCookies(BTreeMap::new());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    for line in text.lines() {
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 7 {
            return Err("Playback cookie update is malformed");
        }
        if ![
            ".youtube.com",
            "youtube.com",
            ".music.youtube.com",
            "music.youtube.com",
        ]
        .contains(&fields[0])
            || fields[2] != "/"
        {
            continue;
        }
        if !["TRUE", "FALSE"].contains(&fields[1]) {
            return Err("Playback cookie update is malformed");
        }
        // Netscape's include-subdomains flag distinguishes domain cookies
        // from host-only cookies. A host-only youtube.com cookie is not sent
        // to music.youtube.com and must not shadow its domain-scoped session.
        if fields[1] == "FALSE" && fields[0].trim_start_matches('.') != "music.youtube.com" {
            continue;
        }
        let expiry = fields[4]
            .parse::<u64>()
            .map_err(|_| "Playback cookie expiry is malformed")?;
        if expiry != 0 && expiry <= now {
            continue;
        }
        let (name, value) = (fields[5], fields[6]);
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || v == b'-' || v == b'_')
            || value.is_empty()
            || value.len() > 4096
            || !value.is_ascii()
            || value.chars().any(char::is_control)
            || value.contains(';')
            || !["TRUE", "FALSE"].contains(&fields[1])
            || !["TRUE", "FALSE"].contains(&fields[3])
            || (name.starts_with("__Secure-") && fields[3] != "TRUE")
        {
            return Err("Playback cookie update is malformed");
        }
        if cookies.0.get(name).is_some_and(|old| old != value) {
            return Err("Playback cookie scopes conflict");
        }
        cookies.0.insert(name.into(), value.into());
        if cookies.0.len() > 200 {
            return Err("Playback cookie update has too many entries");
        }
    }
    Ok(cookies)
}

#[cfg(windows)]
fn execute_helper(
    input: &[u8],
    cancel: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<HelperReply, String> {
    let python = runtime_path(
        "TIDE_PROVIDER_PYTHON",
        ".tools/provider-venv/Scripts/python.exe",
        "python",
    );
    let helper = runtime_path(
        "TIDE_PROVIDER_HELPER",
        "helpers/youtube_music.py",
        "tools/youtube_music.py",
    );
    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = tide_playback::OwnedProcess::spawn(&mut command)
        .map_err(|_| "Account helper unavailable. Run scripts/setup-provider.ps1")?;
    let mut stdin = child
        .take_stdin()
        .ok_or("Account helper input unavailable")?;
    let mut data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let result = stdin.write_all(&data);
        data.zeroize();
        result
    });
    let stdout = child
        .take_stdout()
        .ok_or("Account helper output unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) || started.elapsed() > timeout {
            let _ = child.terminate_tree();
            let _ = writer.join();
            let _ = reader.join();
            return Err("Account request cancelled or timed out".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => {
                let _ = child.terminate_tree();
                let _ = writer.join();
                let _ = reader.join();
                return Err("Account helper stopped unexpectedly".into());
            }
        }
    };
    let _ = child.terminate_tree();
    writer
        .join()
        .map_err(|_| "Account helper input failed")?
        .map_err(|_| "Account helper input failed")?;
    let mut bytes = reader
        .join()
        .map_err(|_| "Account helper output failed")?
        .map_err(|_| "Account helper output failed")?;
    if !status.success() || bytes.len() > 2 * 1024 * 1024 {
        bytes.zeroize();
        return Err("Account helper failed or response exceeded the size limit".into());
    }
    let response = decode_helper_reply(&bytes);
    bytes.zeroize();
    let reply = response.map_err(str::to_string)?;
    let response: Result<Response, String> = match reply.response {
        Response::Error { code } if code == "lyrics_unavailable" => Ok(Response::Error { code }),
        Response::Error { code } if code == "runtime" => {
            Err("Install the optional account helper with scripts/setup-provider.ps1".into())
        }
        Response::Error { code } if code == "profile" => {
            Err("Select cookies.sqlite inside your Zen profile, or use Import session for a custom profile".into())
        }
        Response::Error { code } if code == "profile_read" => {
            Err("Zen's cookie database could not be read. Close Zen after signing in, then reconnect using the same profile.".into())
        }
        Response::Error { code } if code == "cookie_ambiguity" => {
            Err("Zen has conflicting YouTube sign-in cookies. Use Account > Advanced > Import session with request headers from the intended YouTube Music account.".into())
        }
        Response::Error { code } if code == "container" => {
            Err("Sign-in was found in a Zen container. Tide currently needs YouTube Music signed in on a tab outside that container.".into())
        }
        Response::Error { code }
            if code == "session" || code == "request" || code == "supermix_unavailable" || code == "artist_unavailable" =>
        {
            Ok(Response::Error { code })
        }
        Response::Error { code } if code == "login_browser" => Err(
            "Could not open the sign-in browser. Install Zen or set TIDE_ZEN_EXE, or use Import session"
                .into(),
        ),
        Response::Error { code } if code == "login_blocked" => Err(
            "Google declined this browser's login. Use Import session from your regular browser"
                .into(),
        ),
        Response::Error { code } if code == "login_closed" || code == "login_timeout" => {
            Err("Sign-in window closed or timed out. Choose Connect account to try again".into())
        }
        Response::Error { code } if code == "cleanup" => {
            Err("Sign-in browser cleanup failed. Close the temporary Zen window and retry Connect account".into())
        }
        Response::Error { .. } => {
            Err("YouTube Music request failed. Re-import a fresh session and try again".into())
        }
        response => Ok(response),
    };
    let response = response?;
    Ok(HelperReply {
        response,
        refreshed: reply.refreshed,
    })
}

fn runtime_path(variable: &str, relative: &str, fallback: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(variable) {
        return path.into();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if variable == "TIDE_PROVIDER_HELPER" && parent.join(fallback).is_file() {
                return parent.join(fallback);
            }
            for directory in parent.ancestors().take(5) {
                let path = directory.join(relative);
                if path.is_file() {
                    return path;
                }
            }
        }
    }
    PathBuf::from(fallback)
}

#[cfg(windows)]
fn private_directory(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    fs::create_dir_all(path).map_err(|_| "Could not create account storage")?;
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;OW)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: null-terminated UTF-16 inputs and a freshly allocated descriptor,
    // freed exactly once below. The protected ACL grants only owner and SYSTEM.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err("Could not protect account storage permissions".into());
        }
        let ok = SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        LocalFree(descriptor);
        if ok == 0 {
            return Err("Could not protect account storage permissions".into());
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn private_directory(_: &Path) -> Result<(), String> {
    Err("Account integration currently requires Windows".into())
}

#[cfg(windows)]
fn crypt(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, &'static str> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: input points to a live slice; output is owned Windows allocation,
    // copied and freed once. No machine-wide DPAPI flag or interactive prompt.
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("Windows could not protect or unlock the session. Reconnect your account");
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}
#[cfg(windows)]
fn protect(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    crypt(bytes, true)
}
#[cfg(windows)]
fn unprotect(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    crypt(bytes, false)
}
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>, &'static str> {
    Err("Account integration currently requires Windows")
}
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>, &'static str> {
    Err("Account integration currently requires Windows")
}

#[cfg(test)]
mod tests {
    // These fixtures intentionally use the production machine-wide Windows mutex.
    // Serialize fixture owners while keeping each test's real cross-thread locking.
    #[cfg(windows)]
    static SESSION_FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    use super::*;

    #[cfg(windows)]
    #[test]
    fn native_rotation_merges_without_rollback_deletion_or_account_switch() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-native-merge-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        let original_cookie = "__Secure-3PAPISID=old; SID=old; PREF=quiet";
        provider.import_bytes(&serde_json::to_vec(&serde_json::json!({"cookie":original_cookie, "x-goog-authuser":"1", "x-goog-pageid":"123"})).unwrap()).unwrap();
        let before = cookie_jar(original_cookie).unwrap();
        let after = cookie_jar("__Secure-3PAPISID=fresh; SID=new; PREF=quiet").unwrap();
        assert!(!provider
            .merge_playback_cookies(0, before.as_bytes(), after.as_bytes())
            .unwrap());
        assert!(!provider
            .merge_playback_cookies(1, before.as_bytes(), b"malformed")
            .unwrap());
        let omitted = cookie_jar(concat!("__Secure-3PAPISID=", "old")).unwrap();
        assert!(!provider
            .merge_playback_cookies(1, before.as_bytes(), omitted.as_bytes())
            .unwrap());
        assert!(provider
            .merge_playback_cookies(1, before.as_bytes(), after.as_bytes())
            .unwrap());
        let saved = provider.headers().unwrap();
        assert_eq!(saved["x-goog-pageid"], "123");
        assert!(saved["cookie"].contains("PREF=quiet"));
        assert!(saved["cookie"].contains("SID=new"));
        let stale_cohort = cookie_jar("__Secure-3PAPISID=fresh; SID=old; PREF=quiet").unwrap();
        let conflicting = cookie_jar("__Secure-3PAPISID=fresh; SID=later; PREF=quiet").unwrap();
        assert!(!provider
            .merge_playback_cookies(1, stale_cohort.as_bytes(), conflicting.as_bytes())
            .unwrap());
        let encrypted = fs::read(directory.join("session.dpapi")).unwrap();
        assert!(!provider
            .merge_playback_cookies(1, before.as_bytes(), after.as_bytes())
            .unwrap());
        let stale_api = refresh_fixture(
            "home",
            serde_json::json!({"cookie":"__Secure-3PAPISID=later", "x-goog-authuser":"1", "x-goog-pageid":"123"}),
        );
        assert!(!provider.persist_refreshed_headers(
            &stale_api.refreshed,
            Some("1"),
            Some("123"),
            original_cookie,
            &AtomicBool::new(false)
        ));
        assert_eq!(
            fs::read(directory.join("session.dpapi")).unwrap(),
            encrypted
        );
        provider.disconnect().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn api_auth_rotation_preserves_concurrent_non_auth_cookies_and_metadata() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        for concurrent_update in [false, true] {
            let directory = std::env::temp_dir().join(format!(
                "tide-api-merge-{}-{}",
                std::process::id(),
                LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            private_directory(&directory).unwrap();
            let provider = Provider {
                directory: directory.clone(),
            };
            let baseline = "__Secure-3PAPISID=old; SID=old; PREF=quiet; KEEP=retained";
            provider
                .import_bytes(
                    &serde_json::to_vec(&serde_json::json!({
                        "cookie":baseline, "x-goog-authuser":"1", "x-goog-pageid":"123",
                        "x-goog-visitor-id":"visitor-original", "user-agent":"agent-original"
                    }))
                    .unwrap(),
                )
                .unwrap();
            if concurrent_update {
                let before = cookie_jar(baseline).unwrap();
                let after = cookie_jar(
                    "__Secure-3PAPISID=old; SID=old; PREF=native; KEEP=retained; NATIVE=added",
                )
                .unwrap();
                assert!(provider
                    .merge_playback_cookies(1, before.as_bytes(), after.as_bytes())
                    .unwrap());
            }
            // Simulate newer metadata and a different serialization order while
            // the API request is in flight; neither changes account identity.
            let mut current = provider.headers().unwrap();
            if !concurrent_update {
                current.insert(
                    "cookie".into(),
                    "KEEP=retained; __Secure-3PAPISID=old; PREF=quiet; SID=old".into(),
                );
            }
            current.insert("x-goog-visitor-id".into(), "visitor-current".into());
            current.insert("user-agent".into(), "agent-current".into());
            provider
                .import_bytes(&serde_json::to_vec(&current).unwrap())
                .unwrap();
            let candidate = refresh_fixture(
                "home",
                serde_json::json!({
                    "cookie":"__Secure-3PAPISID=api-fresh; SID=api-fresh; PREF=api; API=added",
                    "x-goog-authuser":"1", "x-goog-pageid":"123",
                    "x-goog-visitor-id":"visitor-stale", "user-agent":"agent-stale"
                }),
            );
            assert!(provider.persist_refreshed_headers(
                &candidate.refreshed,
                Some("1"),
                Some("123"),
                baseline,
                &AtomicBool::new(false)
            ));
            let saved = provider.headers().unwrap();
            let cookies = scoped_header_cookies(&saved["cookie"]).unwrap();
            assert_eq!(cookies.0["__Secure-3PAPISID"], "api-fresh");
            assert_eq!(cookies.0["SID"], "api-fresh");
            assert_eq!(
                cookies.0["PREF"],
                if concurrent_update { "native" } else { "api" }
            );
            assert_eq!(cookies.0["KEEP"], "retained");
            assert_eq!(cookies.0["API"], "added");
            assert_eq!(
                cookies.0.get("NATIVE").map(String::as_str),
                concurrent_update.then_some("added")
            );
            assert_eq!(saved["x-goog-visitor-id"], "visitor-current");
            assert_eq!(saved["user-agent"], "agent-current");
            let encrypted = fs::read(directory.join("session.dpapi")).unwrap();
            assert!(!encrypted
                .windows(b"api-fresh".len())
                .any(|bytes| bytes == b"api-fresh"));
            // Repeating the same completed response must not change storage.
            assert!(!provider.persist_refreshed_headers(
                &candidate.refreshed,
                Some("1"),
                Some("123"),
                baseline,
                &AtomicBool::new(false)
            ));
            assert_eq!(
                fs::read(directory.join("session.dpapi")).unwrap(),
                encrypted
            );
            provider.disconnect().unwrap();
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn api_refresh_rejects_newer_auth_cohort_even_if_signing_cookie_is_unchanged() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-api-auth-conflict-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        let baseline = "__Secure-3PAPISID=old; SID=old; PREF=quiet";
        provider
            .import_bytes(
                &serde_json::to_vec(&serde_json::json!({
                    "cookie":baseline, "x-goog-authuser":"1", "x-goog-pageid":"123"
                }))
                .unwrap(),
            )
            .unwrap();
        let before = cookie_jar(baseline).unwrap();
        let after = cookie_jar("__Secure-3PAPISID=old; SID=native-fresh; PREF=quiet").unwrap();
        assert!(provider
            .merge_playback_cookies(1, before.as_bytes(), after.as_bytes())
            .unwrap());
        let encrypted = fs::read(directory.join("session.dpapi")).unwrap();
        let candidate = refresh_fixture(
            "tracks",
            serde_json::json!({
                "cookie":"__Secure-3PAPISID=api-stale; SID=api-stale; PREF=api-stale",
                "x-goog-authuser":"1", "x-goog-pageid":"123"
            }),
        );
        assert!(!provider.persist_refreshed_headers(
            &candidate.refreshed,
            Some("1"),
            Some("123"),
            baseline,
            &AtomicBool::new(false)
        ));
        assert_eq!(
            fs::read(directory.join("session.dpapi")).unwrap(),
            encrypted
        );
        let saved = provider.headers().unwrap();
        assert_eq!(
            scoped_header_cookies(&saved["cookie"]).unwrap().0["SID"],
            "native-fresh"
        );
        for (authuser, pageid) in [("2", "123"), ("1", "456")] {
            // The original authentication cookies do not authorize overwriting
            // an account or page selection made while the request was in flight.
            provider
                .import_bytes(
                    &serde_json::to_vec(&serde_json::json!({
                        "cookie":baseline, "x-goog-authuser":authuser, "x-goog-pageid":pageid
                    }))
                    .unwrap(),
                )
                .unwrap();
            let selected = fs::read(directory.join("session.dpapi")).unwrap();
            assert!(!provider.persist_refreshed_headers(
                &candidate.refreshed,
                Some("1"),
                Some("123"),
                baseline,
                &AtomicBool::new(false)
            ));
            assert_eq!(fs::read(directory.join("session.dpapi")).unwrap(), selected);
        }
        provider.disconnect().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn native_cookie_scope_and_visitor_bounds_are_enforced() {
        let bytes = b"# Netscape HTTP Cookie File\n#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tfixture\n.evil.test\tTRUE\t/\tTRUE\t0\tSID\tforeign\n.youtube.com\tTRUE\t/subpath\tTRUE\t0\tPREF\tforeign\n";
        let cookies = scoped_cookie_snapshot(bytes).unwrap();
        assert_eq!(cookies.0.len(), 1);
        assert_eq!(cookies.0["SID"], "fixture");
        let scoped = scoped_cookie_snapshot(
            b".youtube.com\tTRUE\t/\tTRUE\t0\tSID\tdomain-session\nyoutube.com\tFALSE\t/\tTRUE\t0\tSID\troot-host-only\nmusic.youtube.com\tFALSE\t/\tTRUE\t0\tPREF\tmusic-host-only\n",
        )
        .unwrap();
        assert_eq!(scoped.0.len(), 2);
        assert_eq!(scoped.0["SID"], "domain-session");
        assert_eq!(scoped.0["PREF"], "music-host-only");
        assert!(scoped_cookie_snapshot(
            b"youtube.com\tTRUE\t/\tTRUE\t0\tSID\tdomain-one\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tdomain-two\n"
        )
        .is_err());
        assert!(scoped_cookie_snapshot(&vec![b'x'; MAX_SESSION + 1]).is_err());
        for length in [4096, 4097] {
            let headers = serde_json::to_vec(&serde_json::json!({"cookie":"__Secure-3PAPISID=fixture", "x-goog-authuser":"1", "x-goog-visitor-id":"v".repeat(length)})).unwrap();
            assert_eq!(parse_headers(&headers).is_ok(), length == 4096);
        }
    }

    fn refresh_fixture(response_type: &str, refreshed: serde_json::Value) -> HelperReply {
        let mut response = match response_type {
            "error" => serde_json::json!({"type":"error", "code":"request"}),
            "home" => serde_json::json!({"type":"home", "cards":[]}),
            "library" => {
                serde_json::json!({"type":"library", "account":"Fixture", "playlists":[], "truncated":false})
            }
            _ => serde_json::json!({"type":"tracks", "tracks":[], "truncated":false}),
        };
        response["refreshed_headers"] = refreshed;
        decode_helper_reply(&serde_json::to_vec(&response).unwrap()).unwrap()
    }

    #[test]
    fn refreshed_headers_are_optional_bounded_and_ignored_on_errors() {
        let headers = serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1", "x-goog-visitor-id":"fixturevisitor"});
        for response_type in ["library", "tracks", "home"] {
            let reply = refresh_fixture(response_type, headers.clone());
            assert_eq!(
                reply.refreshed.0.as_ref().unwrap()["x-goog-visitor-id"],
                "fixturevisitor"
            );
        }
        assert!(refresh_fixture("error", headers).refreshed.0.is_none());
        for invalid in [
            serde_json::json!("invalid"),
            serde_json::json!({"cookie":12}),
            serde_json::json!({"cookie":concat!("__Secure-3PAPISID=", "fixture\nInjected:value")}),
            serde_json::json!({"cookie":format!("__Secure-3PAPISID={}", "x".repeat(MAX_SESSION))}),
        ] {
            let reply = refresh_fixture("tracks", invalid);
            assert!(matches!(reply.response, Response::Tracks { .. }));
            assert!(reply.refreshed.0.is_none());
        }
        for response in [
            serde_json::json!({"type":"artist", "name":"Band", "tracks":[], "truncated":false}),
            serde_json::json!({"type":"artists", "artists":[]}),
        ] {
            let mut response = response;
            response["refreshed_headers"] =
                serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1"});
            let reply = decode_helper_reply(&serde_json::to_vec(&response).unwrap()).unwrap();
            assert!(
                reply.refreshed.0.is_some(),
                "artist navigation must retain refreshed session cookies"
            );
        }
        let reply = decode_helper_reply(br#"{"type":"home","cards":[]}"#).unwrap();
        assert!(reply.refreshed.0.is_none());
    }

    #[test]
    fn large_playlist_keeps_metadata_and_optional_credentials_separate() {
        let tracks: Vec<_> = (0..tide_core::MAX_QUEUE_TRACKS)
            .map(|index| {
                serde_json::json!({
                    "title":format!("Track {index}"), "artist":"Fixture artist",
                    "album":"Fixture album", "source":format!("fixture-{index}"),
                    "artwork_url":"https://i.ytimg.com/fixture.jpg"
                })
            })
            .collect();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "type":"tracks", "tracks":tracks, "truncated":true,
            "refreshed_headers":{"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1"}
        }))
        .unwrap();
        let reply = decode_helper_reply(&bytes).unwrap();
        let Response::Tracks {
            tracks, truncated, ..
        } = reply.response
        else {
            panic!("Playlist response was lost");
        };
        assert!(truncated);
        assert_eq!(tracks.len(), tide_core::MAX_QUEUE_TRACKS);
        assert_eq!(tracks.last().unwrap().title, "Track 1999");
        assert_eq!(reply.refreshed.0.as_ref().unwrap()["x-goog-authuser"], "1");

        // An ambiguous optional update must not discard playable results.
        let reply = decode_helper_reply(
            br#"{"type":"tracks","tracks":[],"truncated":false,"refreshed_headers":{"cookie":"__Secure-3PAPISID=fresh" },"refreshed_headers":null}"#,
        )
        .unwrap();
        assert!(matches!(reply.response, Response::Tracks { .. }));
        assert!(reply.refreshed.0.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn successful_refresh_is_encrypted_and_updates_existing_playback_lease() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-refresh-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        provider.import_bytes(br#"{"cookie":"__Secure-3PAPISID=old", "x-goog-authuser":"1", "x-goog-pageid":"123"}"#).unwrap();
        let connection_before_refresh = provider.saved_connection().unwrap();
        let lease = provider.cookie_lease().unwrap().unwrap();
        let lease_path = lease.path().to_owned();
        let reply = refresh_fixture(
            "home",
            serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1", "x-goog-pageid":"123", "x-goog-visitor-id":"fixturevisitor"}),
        );
        assert!(provider.persist_refreshed_headers(
            &reply.refreshed,
            Some("1"),
            Some("123"),
            "__Secure-3PAPISID=old",
            &AtomicBool::new(false)
        ));
        assert_eq!(
            provider.headers().unwrap()["cookie"],
            "__Secure-3PAPISID=fresh"
        );
        assert!(provider.saved_connection().unwrap() == connection_before_refresh);
        let encrypted = fs::read(directory.join("session.dpapi")).unwrap();
        assert!(!encrypted
            .windows(b"fresh".len())
            .any(|window| window == b"fresh"));
        provider.refresh_cookie_lease(&lease).unwrap();
        let jar = fs::read_to_string(&lease_path).unwrap();
        assert!(jar.contains("fresh"));
        assert!(!jar.contains("old"));
        drop(lease);
        assert!(!lease_path.exists());
        provider.disconnect().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejected_refresh_preserves_session_and_selected_identity() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-refresh-reject-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        provider.import_bytes(br#"{"cookie":"__Secure-3PAPISID=old", "x-goog-authuser":"1", "x-goog-pageid":"123"}"#).unwrap();
        let original = fs::read(directory.join("session.dpapi")).unwrap();
        for candidate in [
            serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"2", "x-goog-pageid":"123"}),
            serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1", "x-goog-pageid":"456"}),
            serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1"}),
            serde_json::json!({"cookie":"__Secure-3PAPISID=", "x-goog-authuser":"1", "x-goog-pageid":"123"}),
        ] {
            let reply = refresh_fixture("tracks", candidate);
            assert!(!provider.persist_refreshed_headers(
                &reply.refreshed,
                Some("1"),
                Some("123"),
                "__Secure-3PAPISID=old",
                &AtomicBool::new(false)
            ));
            assert_eq!(fs::read(directory.join("session.dpapi")).unwrap(), original);
        }
        let valid = serde_json::json!({"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1", "x-goog-pageid":"123"});
        let error = refresh_fixture("error", valid.clone());
        assert!(!provider.persist_refreshed_headers(
            &error.refreshed,
            Some("1"),
            Some("123"),
            "__Secure-3PAPISID=old",
            &AtomicBool::new(false)
        ));
        let cancelled = refresh_fixture("tracks", valid);
        assert!(!provider.persist_refreshed_headers(
            &cancelled.refreshed,
            Some("1"),
            Some("123"),
            "__Secure-3PAPISID=old",
            &AtomicBool::new(true)
        ));
        assert_eq!(fs::read(directory.join("session.dpapi")).unwrap(), original);
        provider.disconnect().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn disconnect_waits_for_in_flight_session_commit() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-disconnect-lock-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        provider
            .import_bytes(br#"{"cookie":"__Secure-3PAPISID=old", "x-goog-authuser":"1"}"#)
            .unwrap();
        let guard = SessionWriteLock::acquire().unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            done_tx.send(provider.disconnect()).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let blocked = done_rx.recv_timeout(Duration::from_millis(150));
        let session_retained = directory.join("session.dpapi").exists();
        drop(guard);
        let result = match blocked {
            Ok(result) => result,
            Err(_) => done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        };
        worker.join().unwrap();
        result.unwrap();
        assert!(
            session_retained,
            "disconnect bypassed the session commit guard"
        );
        assert!(!directory.join("session.dpapi").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn login_generation_preserves_legacy_refreshes_and_tracks_explicit_imports() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-connection-revision-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        assert!(provider.saved_connection().unwrap().is_none());
        let legacy =
            br#"{"cookie":"__Secure-3PAPISID=old", "x-goog-authuser":"1", "x-goog-pageid":"123"}"#;
        fs::write(directory.join("session.dpapi"), protect(legacy).unwrap()).unwrap();
        assert!(provider
            .saved_connection()
            .unwrap()
            .unwrap()
            .revision
            .is_none());
        let refreshed =
            br#"{"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1", "x-goog-pageid":"123"}"#;
        provider.write_session(refreshed, false).unwrap();
        assert!(provider
            .saved_connection()
            .unwrap()
            .unwrap()
            .revision
            .is_none());
        provider.import_bytes(legacy).unwrap();
        let imported = provider.saved_connection().unwrap().unwrap();
        assert!(imported.revision.is_some());
        assert_eq!(imported.account_index, 1);
        assert_eq!(imported.page_id.as_deref(), Some("123"));
        assert!(!provider
            .headers()
            .unwrap()
            .contains_key(CONNECTION_REVISION));
        provider.write_session(refreshed, false).unwrap();
        assert!(provider.saved_connection().unwrap().unwrap() == imported);
        // A reconnect of the same identity must still notify the running player.
        provider.import_bytes(refreshed).unwrap();
        assert!(provider.saved_connection().unwrap().unwrap().revision != imported.revision);
        provider.import_bytes(br#"{"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"2", "x-goog-pageid":"456"}"#).unwrap();
        let switched = provider.saved_connection().unwrap().unwrap();
        assert_eq!(switched.account_index, 2);
        assert_eq!(switched.page_id.as_deref(), Some("456"));
        provider.disconnect().unwrap();
        assert!(provider.saved_connection().unwrap().is_none());
        fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn reconnect_rejects_stale_playback_lease_and_verified_candidate() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        // Isolate this multi-step race scenario from other storage fixtures.
        let _guard = SessionWriteLock::acquire().unwrap();
        let directory = std::env::temp_dir().join(format!(
            "tide-adoption-race-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        let old = br#"{"cookie":"__Secure-3PAPISID=old", "x-goog-authuser":"1"}"#;
        let fresh = br#"{"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1"}"#;
        provider.import_bytes(old).unwrap();
        let (original, lease) = provider.playback_connection().unwrap();
        let lease = lease.unwrap();
        let before = fs::read(lease.path()).unwrap();
        provider.import_bytes(fresh).unwrap();
        let current = provider.saved_connection().unwrap();
        assert!(provider
            .refresh_cookie_lease_for_connection(&lease, original.as_ref().unwrap())
            .is_err());
        assert_eq!(fs::read(lease.path()).unwrap(), before);
        let candidate = serde_json::from_slice(old).unwrap();
        assert!(provider
            .save_verified_session_for_connection(candidate, &AtomicBool::new(false), &original)
            .is_err());
        assert!(provider.saved_connection().unwrap() == current);
        provider
            .refresh_cookie_lease_for_connection(&lease, current.as_ref().unwrap())
            .unwrap();
        assert!(fs::read(lease.path()).unwrap() != before);
        provider.disconnect().unwrap();
        assert!(provider
            .save_verified_session_for_connection(
                serde_json::from_slice(fresh).unwrap(),
                &AtomicBool::new(false),
                &current
            )
            .is_err());
        assert!(provider.saved_connection().unwrap().is_none());
        drop(lease);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn feedback_envelope_does_not_send_local_account_revision_to_the_helper() {
        let request = Request::Feedback {
            request: FeedbackRequest::Rate {
                video_id: "K4DyBUG242c".into(),
                rating: Rating::Like,
            },
            expected: SavedConnection {
                revision: Some("local-generation".into()),
                account_index: 1,
                page_id: None,
            },
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "action":"feedback", "request":{"kind":"rate", "video_id":"K4DyBUG242c", "rating":"LIKE"}
            })
        );
        let reply = decode_helper_reply(br#"{"type":"error","code":"feedback_unconfirmed","refreshed_headers":{"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1"}}"#).unwrap();
        assert!(reply.refreshed.0.is_some());
        let lyrics = decode_helper_reply(br#"{"type":"error","code":"lyrics_unavailable","refreshed_headers":{"cookie":"__Secure-3PAPISID=fresh", "x-goog-authuser":"1"}}"#).unwrap();
        assert!(
            lyrics.refreshed.0.is_some(),
            "Optional lyrics failures must retain refreshed cookies"
        );
    }

    #[cfg(windows)]
    #[test]
    fn changed_account_rejects_collection_write_before_starting_any_helper() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-feedback-binding-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        provider
            .import_bytes(br#"{"cookie":"__Secure-3PAPISID=old", "x-goog-authuser":"1"}"#)
            .unwrap();
        let mut expected = provider.saved_connection().unwrap().unwrap();
        expected.account_index = 2;
        let result = provider.execute(
            Request::Feedback {
                request: FeedbackRequest::History {
                    video_id: "K4DyBUG242c".into(),
                },
                expected,
            },
            Arc::new(AtomicBool::new(false)),
        );
        assert!(
            matches!(result, Err(message) if message == "Account changed or collection action is invalid; choose the action again")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn radio_request_matches_helper_contract() {
        let request = Request::Radio {
            video_id: "K4DyBUG242c".into(),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "action": "radio", "video_id": "K4DyBUG242c"
            })
        );
    }
    #[test]
    fn supermix_request_matches_helper_contract() {
        assert_eq!(
            serde_json::to_value(Request::Supermix).unwrap(),
            serde_json::json!({"action": "supermix"})
        );
    }
    #[test]
    fn older_metadata_without_artwork_still_deserializes() {
        let response: Response = serde_json::from_value(serde_json::json!({
            "type": "tracks", "tracks": [{"title":"Song", "artist":"", "album":"", "source":"fixture"}],
            "truncated":false
        })).unwrap();
        let Response::Tracks { tracks, .. } = response else {
            panic!("expected tracks")
        };
        assert_eq!(tracks[0].artwork_url, None);
        let playlist: Playlist = serde_json::from_value(serde_json::json!({
            "id":"PLfixture", "title":"Mix", "subtitle":""
        }))
        .unwrap();
        let card: Card = serde_json::from_value(serde_json::json!({
            "id":"PLfixture", "title":"Mix", "subtitle":"", "kind":"playlist", "shelf":"For you"
        }))
        .unwrap();
        assert_eq!(playlist.artwork_url, None);
        assert_eq!(card.artwork_url, None);
    }
    #[test]
    fn only_needed_headers_are_retained() {
        let headers = parse_headers(
            concat!(
                "Cookie: __Secure-3PAPISID=",
                "fixture",
                "\r\nAuthorization: ignored\r\nHost: unrelated.example\r\nX-Goog-AuthUser: 0"
            )
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(headers.len(), 4);
        assert_eq!(headers["authorization"], "SAPISIDHASH");
        assert_eq!(headers["x-origin"], "https://music.youtube.com");
    }
    #[test]
    fn rejects_missing_or_injected_auth() {
        assert!(parse_headers(b"{\"cookie\":\"other=fixture\"}").is_err());
        assert!(parse_headers(
            concat!("{\"cookie\":\"SAPISID=", "fixture", "\\nInjected: value\"}").as_bytes()
        )
        .is_err());
    }
    #[test]
    fn jar_is_scoped_to_youtube() {
        let jar = cookie_jar("SAPISID=fixture; PREF=test").unwrap();
        assert!(jar.contains(".youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfixture"));
        assert!(!jar.contains(".google.com"));
        assert!(cookie_jar(&"a=b;".repeat(MAX_SESSION / 4)).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_and_tamper_rejection() {
        let encrypted = protect(b"public fixture").unwrap();
        assert_ne!(encrypted, b"public fixture");
        assert_eq!(unprotect(&encrypted).unwrap(), b"public fixture");
        assert!(unprotect(b"invalid ciphertext").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn imported_session_is_encrypted_and_lease_is_removed() {
        let _fixture = SESSION_FIXTURES.lock().unwrap_or_else(|e| e.into_inner());
        let directory = std::env::temp_dir().join(format!(
            "tide-account-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
        // Verification takes place outside the storage lock. Its eventual result
        // cannot replace an account imported/disconnected in the meantime.
        let first =
            parse_headers(concat!("Cookie: __Secure-3PAPISID=", "fixture-first").as_bytes())
                .unwrap();
        let second =
            parse_headers(concat!("Cookie: __Secure-3PAPISID=", "fixture-second").as_bytes())
                .unwrap();
        let baseline = provider.import_baseline().unwrap();
        let cancel = AtomicBool::new(false);
        provider
            .commit_browser_session(first.clone(), baseline, &cancel)
            .unwrap();
        let baseline = provider.import_baseline().unwrap();
        provider
            .import_bytes(concat!("Cookie: __Secure-3PAPISID=", "fixture-newer").as_bytes())
            .unwrap();
        let newer = provider.import_baseline().unwrap();
        assert_eq!(
            provider.commit_browser_session(second.clone(), baseline, &cancel),
            Err(SessionImportError::SessionChanged)
        );
        assert_eq!(provider.import_baseline().unwrap(), newer);
        let baseline = provider.import_baseline().unwrap();
        provider.disconnect().unwrap();
        assert_eq!(
            provider.commit_browser_session(second.clone(), baseline, &cancel),
            Err(SessionImportError::SessionChanged)
        );
        assert!(!provider.is_connected());
        cancel.store(true, Ordering::Release);
        assert_eq!(
            provider.commit_browser_session(second, None, &cancel),
            Err(SessionImportError::Cancelled)
        );
        assert!(!provider.is_connected());
        let stale = directory.join("playback-4294967295-1.txt");
        fs::write(&stale, b"public fixture").unwrap();
        let active = directory.join(format!("playback-{}-999.txt", std::process::id()));
        fs::write(&active, b"public fixture").unwrap();
        remove_stale_leases(&directory);
        assert!(!stale.exists());
        assert!(active.exists());
        let stale_profile = directory.join("login-4294967295-1");
        fs::create_dir(&stale_profile).unwrap();
        fs::write(stale_profile.join("public-fixture"), b"fixture").unwrap();
        remove_stale_leases(&directory);
        assert!(!stale_profile.exists());
        let owned_profile = directory.join(format!("login-{}-1000", std::process::id()));
        fs::create_dir(&owned_profile).unwrap();
        let guard = LoginDirectory {
            path: owned_profile.clone(),
            parent: directory.clone(),
        };
        drop(guard);
        assert!(!owned_profile.exists());
        let input = directory.join("fixture-input.txt");
        fs::write(&input, concat!("Cookie: __Secure-3PAPISID=", "fixture")).unwrap();
        provider.import_session(&input).unwrap();
        assert!(provider.is_connected());
        let encrypted = fs::read(directory.join("session.dpapi")).unwrap();
        assert!(!encrypted.windows(7).any(|s| s == b"fixture"));
        let mut candidate = provider.headers().unwrap();
        candidate.insert("x-goog-authuser".into(), "1".into());
        assert!(provider
            .save_verified_session(candidate.clone(), &AtomicBool::new(true))
            .is_err());
        assert_eq!(
            fs::read(directory.join("session.dpapi")).unwrap(),
            encrypted
        );
        assert_eq!(provider.account_index(), 0);
        provider
            .save_verified_session(candidate, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(provider.account_index(), 1);
        let lease = provider.cookie_lease().unwrap().unwrap();
        let path = lease.path().to_owned();
        assert!(path.is_file());
        drop(lease);
        assert!(!path.exists());
        provider.disconnect().unwrap();
        assert!(!provider.is_connected());
        assert!(input.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
