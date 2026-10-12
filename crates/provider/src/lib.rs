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

const MAX_SESSION: usize = 65536;
static LEASE_COUNTER: AtomicU64 = AtomicU64::new(1);

// Serialize read/compare/write across Tide instances. The Windows mutex is
// recursive, so guarded merges can reuse the ordinary protected import path.
#[cfg(windows)]
struct SessionWriteLock(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl SessionWriteLock {
    fn acquire() -> Result<Self, &'static str> {
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
            match WaitForSingleObject(handle, 1000) {
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
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub shelf: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum Request {
    Connect { database: PathBuf, index: u32 },
    SwitchAccount { index: u32 },
    Library,
    Playlist { id: String },
    Radio { video_id: String },
    Liked,
    Home,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Response {
    Connected,
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

fn decode_helper_reply(bytes: &[u8]) -> Result<HelperReply, &'static str> {
    // Parse data independently so an invalid optional update cannot hide valid
    // library results or turn a transient error into a rejected session.
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|_| "Account helper returned an unsupported response")?;
    let mut refreshed = RefreshedHeaders::default();
    if matches!(
        response,
        Response::Library { .. } | Response::Tracks { .. } | Response::Home { .. }
    ) {
        let mut document: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|_| "Account helper returned an unsupported response")?;
        if let Some(value) = document.get_mut("refreshed_headers") {
            let mut encoded = serde_json::to_vec(value).unwrap_or_default();
            zeroize_json(value);
            if encoded.len() <= MAX_SESSION {
                refreshed.0 = parse_headers(&encoded).ok();
            }
            encoded.zeroize();
        }
        zeroize_json(&mut document);
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
    pub fn import_session(&self, path: &Path) -> Result<(), String> {
        let mut bytes = read_limited(path, MAX_SESSION)?;
        let result = self.import_bytes(&bytes);
        bytes.zeroize();
        result
    }
    fn import_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        let _session_lock = SessionWriteLock::acquire().map_err(str::to_string)?;
        (|| {
            let mut headers = parse_headers(bytes)?;
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
        let path = self.directory.join("session.dpapi");
        if path.exists() {
            fs::remove_file(path).map_err(|_| "Could not remove the saved session")?;
        }
        Ok(())
    }
    fn headers(&self) -> Result<BTreeMap<String, String>, String> {
        let encrypted = read_limited(&self.directory.join("session.dpapi"), MAX_SESSION + 8192)?;
        let mut clear = unprotect(&encrypted).map_err(str::to_string)?;
        let headers = serde_json::from_slice(&clear)
            .map_err(|_| "Saved session is invalid. Reconnect your account".to_string());
        clear.zeroize();
        headers
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
        let result = self.import_bytes(&encoded);
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
        let Ok(mut current) = self.headers() else {
            return false;
        };
        let same_identity = current.get("x-goog-authuser").map(String::as_str) == expected_authuser
            && current.get("x-goog-pageid").map(String::as_str) == expected_pageid
            && current.get("cookie").map(String::as_str) == Some(expected_cookie);
        let changed = &current != candidate;
        for value in current.values_mut() {
            value.zeroize();
        }
        if !same_identity || !changed {
            return false;
        }
        let Ok(mut encoded) = serde_json::to_vec(candidate) else {
            return false;
        };
        let accepted = encoded.len() <= MAX_SESSION && self.import_bytes(&encoded).is_ok();
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
            let input = serde_json::to_vec(
                &serde_json::json!({"action":"connect_existing", "database": database, "index": index}),
            )
            .map_err(|_| "Could not prepare account connection")?;
            return match execute_helper(&input, cancel.clone(), Duration::from_secs(600))?.response
            {
                Response::Session { headers } => self.save_verified_session(headers, &cancel),
                Response::Error { code } => Ok(Response::Error { code }),
                _ => Err("Account connection did not return a session".into()),
            };
        }
        let switching = matches!(request, Request::SwitchAccount { .. });
        if matches!(request, Request::SwitchAccount { index } if index > 9) {
            return Err("Choose an account between 1 and 10".into());
        }
        let mut headers = self.headers()?;
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
        let reply = result?;
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
    let mut jar = String::from("# Netscape HTTP Cookie File\n");
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
        jar.push_str(&format!(
            ".youtube.com\tTRUE\t/\tTRUE\t0\t{name}\t{value}\n"
        ));
        if jar.len() > MAX_SESSION {
            jar.zeroize();
            return Err("Playback cookie lease exceeds the size limit");
        }
    }
    Ok(jar)
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
        Response::Error { code } if code == "runtime" => {
            Err("Install the optional account helper with scripts/setup-provider.ps1".into())
        }
        Response::Error { code } if code == "profile" => {
            Err("Select cookies.sqlite inside your Zen profile, or use Import session for a custom profile".into())
        }
        Response::Error { code } if code == "profile_read" => {
            Err("Zen's cookie database could not be read. Close Zen after signing in, then reconnect using the same profile.".into())
        }
        Response::Error { code } if code == "container" => {
            Err("Sign-in was found in a Zen container. Tide currently needs YouTube Music signed in on a tab outside that container.".into())
        }
        Response::Error { code } if code == "session" || code == "request" => {
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
    use super::*;

    #[cfg(windows)]
    #[test]
    fn native_rotation_merges_without_rollback_deletion_or_account_switch() {
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

    #[test]
    fn native_cookie_scope_and_visitor_bounds_are_enforced() {
        let bytes = b"# Netscape HTTP Cookie File\n#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tfixture\n.evil.test\tTRUE\t/\tTRUE\t0\tSID\tforeign\n.youtube.com\tTRUE\t/subpath\tTRUE\t0\tPREF\tforeign\n";
        let cookies = scoped_cookie_snapshot(bytes).unwrap();
        assert_eq!(cookies.0.len(), 1);
        assert_eq!(cookies.0["SID"], "fixture");
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
        let reply = decode_helper_reply(br#"{"type":"home","cards":[]}"#).unwrap();
        assert!(reply.refreshed.0.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn successful_refresh_is_encrypted_and_updates_existing_playback_lease() {
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
        let directory = std::env::temp_dir().join(format!(
            "tide-account-test-{}-{}",
            std::process::id(),
            LEASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&directory).unwrap();
        let provider = Provider {
            directory: directory.clone(),
        };
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
