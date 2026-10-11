//! One bounded, memory-only next-song extraction. Never log signed stream URLs.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::watch;

const FRESH_FOR: Duration = Duration::from_secs(90);
#[cfg(windows)]
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(windows)]
const MAX_OUTPUT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Default)]
pub(super) struct Request {
    source: Option<String>,
    token: u64,
}

#[derive(Clone)]
pub(super) struct PreparedStream {
    url: String,
    options: Value,
    prepared_at: Instant,
}

struct State {
    source: Option<String>,
    ready: Option<PreparedStream>,
}

pub(super) struct Preparation {
    state: Mutex<State>,
    token: AtomicU64,
    closed: AtomicBool,
    requests: watch::Sender<Request>,
}

impl Preparation {
    pub(super) fn new() -> (Arc<Self>, watch::Receiver<Request>) {
        let (requests, receiver) = watch::channel(Request::default());
        (
            Arc::new(Self {
                state: Mutex::new(State {
                    source: None,
                    ready: None,
                }),
                token: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                requests,
            }),
            receiver,
        )
    }

    pub(super) fn set(&self, source: Option<String>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let expired = state
            .ready
            .as_ref()
            .is_some_and(|s| s.prepared_at.elapsed() >= FRESH_FOR);
        if state.source == source && !expired {
            return;
        }
        state.source = source.clone();
        state.ready = None;
        let token = self.token.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        self.requests.send_replace(Request { source, token });
    }

    pub(super) fn clear(&self) {
        self.set(None);
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.closed.store(true, Ordering::Release);
        self.token.fetch_add(1, Ordering::AcqRel);
        state.source = None;
        state.ready = None;
        self.requests.send_replace(Request::default());
    }

    pub(super) fn take_for(&self, source: &str) -> Option<PreparedStream> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let result = if state.source.as_deref() == Some(source) {
            state
                .ready
                .take()
                .filter(|s| s.prepared_at.elapsed() < FRESH_FOR)
        } else {
            None
        };
        state.source = None;
        state.ready = None;
        let token = self.token.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        self.requests.send_replace(Request {
            source: None,
            token,
        });
        result
    }

    pub(super) fn is_ready(&self, source: &str) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.source.as_deref() == Some(source)
            && state
                .ready
                .as_ref()
                .is_some_and(|s| s.prepared_at.elapsed() < FRESH_FOR)
    }

    fn current(&self, token: u64) -> bool {
        !self.closed.load(Ordering::Acquire) && self.token.load(Ordering::Acquire) == token
    }
}

#[cfg(windows)]
pub(super) async fn run(
    runtimes: super::RuntimePaths,
    preparation: Arc<Preparation>,
    mut requests: watch::Receiver<Request>,
) {
    loop {
        let request = requests.borrow_and_update().clone();
        if preparation.closed.load(Ordering::Acquire) {
            break;
        }
        if request.source.is_some() && preparation.current(request.token) {
            let resolver_preparation = Arc::clone(&preparation);
            let resolver_runtimes = runtimes.clone();
            let token = request.token;
            // Await cleanup before starting another resolver: only one exists.
            let result = tokio::task::spawn_blocking(move || {
                resolve(&resolver_runtimes, &request, &resolver_preparation)
            })
            .await;
            if let Ok(Some(stream)) = result {
                let mut state = preparation.state.lock().unwrap_or_else(|p| p.into_inner());
                if preparation.current(token) {
                    state.ready = Some(stream);
                }
            }
        }
        if requests.changed().await.is_err() {
            break;
        }
    }
}

#[cfg(windows)]
fn resolve(
    runtimes: &super::RuntimePaths,
    request: &Request,
    preparation: &Preparation,
) -> Option<PreparedStream> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::thread;

    let mut command = Command::new(runtimes.yt_dlp.as_ref()?);
    command
        .args([
            "--ignore-config",
            "--no-playlist",
            "--skip-download",
            "--dump-single-json",
            "--no-warnings",
            "--no-progress",
            "--no-cache-dir",
            "--format",
            "bestaudio/best",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("PATH", super::child_path(runtimes).ok()?);
    // yt-dlp rewrites its cookie jar. Give this resolver an exclusive copy
    // beside the caller's private lease, inheriting its protected directory ACL.
    let cookie_copy = runtimes
        .cookies
        .as_ref()
        .map(|path| CookieCopy::create(path))
        .transpose()
        .ok()?;
    if let Some(cookies) = &cookie_copy {
        command.arg("--cookies").arg(&cookies.0);
        if runtimes.account_index > 0 {
            command
                .arg("--add-header")
                .arg(format!("X-Goog-AuthUser:{}", runtimes.account_index));
            command.args(["--extractor-args", "youtube:player_skip=webpage,configs"]);
        }
    }
    command.arg("--").arg(request.source.as_ref()?);
    if !preparation.current(request.token) {
        return None;
    }
    let mut child = super::OwnedProcess::spawn(&mut command).ok()?;
    let stdout = child.take_stdout()?;
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_OUTPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= MAX_OUTPUT_BYTES as usize).then_some(bytes)
    });
    let started = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None)
                if started.elapsed() < RESOLVE_TIMEOUT && preparation.current(request.token) =>
            {
                thread::sleep(Duration::from_millis(25))
            }
            _ => break false,
        }
    };
    // A launcher may exit before descendants release stdout. Kill the whole
    // owned job even on successful exit, then join the bounded reader.
    if child.terminate_tree().is_err() {
        drop(child);
    }
    let bytes = output.join().ok()??;
    if !success || !preparation.current(request.token) {
        return None;
    }
    parse_stream(&bytes)
}

#[cfg(windows)]
struct CookieCopy(std::path::PathBuf);

#[cfg(windows)]
impl CookieCopy {
    fn create(source: &std::path::Path) -> std::io::Result<Self> {
        use std::io::Read;
        let parent = source
            .parent()
            .ok_or_else(|| std::io::Error::other("Missing cookie lease directory"))?;
        let counter = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let path = parent.join(format!("playback-{}-{counter}.txt", std::process::id()));
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let copy = Self(path);
        let result = (|| {
            let mut input = std::fs::File::open(source)?.take(MAX_OUTPUT_BYTES + 1);
            let size = std::io::copy(&mut input, &mut destination)?;
            if size > MAX_OUTPUT_BYTES {
                return Err(std::io::Error::other("Cookie lease exceeds size limit"));
            }
            Ok(())
        })();
        drop(destination);
        result?;
        Ok(copy)
    }
}

#[cfg(windows)]
impl Drop for CookieCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn parse_stream(bytes: &[u8]) -> Option<PreparedStream> {
    let info: Value = serde_json::from_slice(bytes).ok()?;
    let url = info.get("url")?.as_str()?;
    let validated = super::validate_source(url).ok()?;
    let parsed = url::Url::parse(&validated.source).ok()?;
    // Restrict pre-resolution to YouTube's media CDN. Unsupported transports
    // or formats continue through mpv's mature extractor hook instead.
    let host = parsed.host_str()?;
    if parsed.scheme() != "https"
        || !(host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
    {
        return None;
    }
    if info.get("acodec")?.as_str()? == "none"
        || info.get("requested_formats").is_some_and(Value::is_array)
    {
        return None;
    }
    let mut options = json!({ "ytdl": "no" });
    if let Some(headers) = info.get("http_headers").and_then(Value::as_object) {
        for (name, value) in headers {
            let value = value.as_str()?;
            if value.len() > 8192 || value.chars().any(char::is_control) {
                return None;
            }
            if name.eq_ignore_ascii_case("user-agent") {
                options["user-agent"] = json!(value);
            } else if name.eq_ignore_ascii_case("referer") {
                options["referrer"] = json!(value);
            } else if name.eq_ignore_ascii_case("x-goog-authuser") {
                // The account selector is public numeric routing information,
                // not an auth token. Preserve only the exact value we generate.
                if value.len() != 1 || !value.as_bytes()[0].is_ascii_digit() {
                    return None;
                }
                options["http-header-fields"] = json!(format!("X-Goog-AuthUser:{value}"));
            } else if !["accept", "accept-language", "sec-fetch-mode"]
                .iter()
                .any(|allowed| name.eq_ignore_ascii_case(allowed))
            {
                // Never silently drop authentication or unrecognized headers.
                return None;
            }
        }
    }
    if let Some(title) = info
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| title.len() <= 1024 && !title.chars().any(char::is_control))
    {
        options["force-media-title"] = json!(title);
    }
    Some(PreparedStream {
        url: validated.source,
        options,
        prepared_at: Instant::now(),
    })
}

impl PreparedStream {
    pub(super) fn load_command(&self) -> Value {
        json!(["loadfile", self.url, "replace", -1, self.options])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> PreparedStream {
        parse_stream(br#"{"url":"https://r1.googlevideo.com/videoplayback?secret=hidden","acodec":"opus","title":"Song","http_headers":{"User-Agent":"agent","Referer":"https://www.youtube.com/"}}"#).unwrap()
    }

    #[test]
    fn deduplicates_requests_and_consumes_only_matching_fresh_streams() {
        let (preparation, _) = Preparation::new();
        preparation.set(Some("song-a".into()));
        let token = preparation.token.load(Ordering::Acquire);
        preparation.set(Some("song-a".into()));
        assert_eq!(preparation.token.load(Ordering::Acquire), token);
        preparation.state.lock().unwrap().ready = Some(stream());
        assert!(preparation.take_for("song-b").is_none());
        assert!(!preparation.current(token));
        preparation.set(Some("song-a".into()));
        preparation.state.lock().unwrap().ready = Some(stream());
        assert!(preparation.take_for("song-a").is_some());
        assert!(preparation.take_for("song-a").is_none());
    }

    #[test]
    fn expiration_refresh_and_stop_discard_signed_urls() {
        let (preparation, _) = Preparation::new();
        preparation.set(Some("song".into()));
        let token = preparation.token.load(Ordering::Acquire);
        let mut expired = stream();
        expired.prepared_at = Instant::now() - FRESH_FOR;
        preparation.state.lock().unwrap().ready = Some(expired);
        assert!(!preparation.is_ready("song"));
        preparation.set(Some("song".into()));
        assert!(!preparation.current(token));
        preparation.state.lock().unwrap().ready = Some(stream());
        preparation.clear();
        assert!(!preparation.is_ready("song"));
        preparation.close();
        preparation.set(Some("song".into()));
        assert!(preparation.state.lock().unwrap().source.is_none());
    }

    #[test]
    fn rejects_credentials_transports_other_hosts_and_private_headers() {
        for info in [
            json!({"url":"http://r1.googlevideo.com/a", "acodec":"opus"}),
            json!({"url":"https://user:pass@r1.googlevideo.com/a", "acodec":"opus"}),
            json!({"url":"https://googlevideo.com.example.com/a", "acodec":"opus"}),
            json!({"url":"https://r1.googlevideo.com/a", "acodec":"none"}),
            json!({"url":"https://r1.googlevideo.com/a", "acodec":"opus", "http_headers":{"Authorization":"private"}}),
            json!({"url":"https://r1.googlevideo.com/a", "acodec":"opus", "http_headers":{"User-Agent":"bad\r\nHeader: value"}}),
        ] {
            assert!(parse_stream(&serde_json::to_vec(&info).unwrap()).is_none());
        }
        let command = stream().load_command();
        assert_eq!(command[4]["ytdl"], "no");
        assert_eq!(command[4]["user-agent"], "agent");
        assert_eq!(command[4]["referrer"], "https://www.youtube.com/");
        let account = json!({"url":"https://r1.googlevideo.com/a", "acodec":"opus", "http_headers":{"X-Goog-AuthUser":"1"}});
        assert_eq!(
            parse_stream(&serde_json::to_vec(&account).unwrap())
                .unwrap()
                .load_command()[4]["http-header-fields"],
            "X-Goog-AuthUser:1"
        );
        let invalid_account = json!({"url":"https://r1.googlevideo.com/a", "acodec":"opus", "http_headers":{"X-Goog-AuthUser":"10"}});
        assert!(parse_stream(&serde_json::to_vec(&invalid_account).unwrap()).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn exclusive_cookie_copy_preserves_original_and_removes_itself() {
        let directory = std::env::temp_dir().join(format!(
            "tide-prefetch-cookie-test-{}-{}",
            std::process::id(),
            super::super::PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("session.txt");
        std::fs::write(&source, b"fixture-only session").unwrap();
        let copy = CookieCopy::create(&source).unwrap();
        let path = copy.0.clone();
        assert_eq!(
            std::fs::read(&source).unwrap(),
            std::fs::read(&path).unwrap()
        );
        std::fs::write(&path, b"changed fixture").unwrap();
        drop(copy);
        assert!(!path.exists());
        assert_eq!(std::fs::read(&source).unwrap(), b"fixture-only session");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
