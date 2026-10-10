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

#[derive(Clone, Deserialize, Serialize)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub subtitle: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub shelf: String,
}
#[derive(Clone, Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum Request {
    Connect { database: PathBuf },
    SwitchAccount { index: u32 },
    Library,
    Playlist { id: String },
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
            // Both files inherit the protected directory ACL. Replace only our own session.
            let destination = self.directory.join("session.dpapi");
            if destination.exists() {
                fs::remove_file(&destination).map_err(|_| "Could not replace session")?;
            }
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

    #[cfg(windows)]
    pub fn execute(&self, request: Request, cancel: Arc<AtomicBool>) -> Result<Response, String> {
        if let Request::Connect { ref database } = request {
            let input = serde_json::to_vec(
                &serde_json::json!({"action":"connect_existing", "database": database}),
            )
            .map_err(|_| "Could not prepare account connection")?;
            return match execute_helper(&input, cancel.clone(), Duration::from_secs(600))? {
                Response::Session { headers } => self.save_verified_session(headers, &cancel),
                _ => Err("Account connection did not return a session".into()),
            };
        }
        let switching = matches!(request, Request::SwitchAccount { .. });
        if matches!(request, Request::SwitchAccount { index } if index > 9) {
            return Err("Choose an account between 1 and 10".into());
        }
        let mut headers = self.headers()?;
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
        match result? {
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
            "accept-language",
            "user-agent",
        ]
        .contains(&key.as_str())
        {
            if value.chars().any(char::is_control) {
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
    if !cookie
        .split(';')
        .any(|part| part.trim().starts_with("__Secure-3PAPISID="))
    {
        return Err("No YouTube Music sign-in cookie found. Follow the connection guide");
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
    }
    Ok(jar)
}

#[cfg(windows)]
fn execute_helper(
    input: &[u8],
    cancel: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<Response, String> {
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
    let response = serde_json::from_slice::<Response>(&bytes);
    bytes.zeroize();
    match response.map_err(|_| "Account helper returned an unsupported response")? {
        Response::Error { code } if code == "runtime" => {
            Err("Install the optional account helper with scripts/setup-provider.ps1".into())
        }
        Response::Error { code } if code == "profile" => {
            Err("Select cookies.sqlite inside your Zen profile, or use Import session for a custom profile".into())
        }
        Response::Error { code } if code == "session" => {
            Err("Sign in to YouTube Music in your Zen profile, then retry Connect account. For another Google account, use Import session".into())
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
    }
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
