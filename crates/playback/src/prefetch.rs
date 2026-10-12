//! One bounded, memory-only next-song extraction. Never log signed stream URLs.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use super::cookie_updates::{CookieUpdate, CookieUpdates, MAX_COOKIE_BYTES};
use serde_json::{json, Value};
use tokio::sync::watch;
#[cfg(windows)]
use zeroize::Zeroizing;

const FRESH_FOR: Duration = Duration::from_secs(10 * 60);
const EXPIRY_MARGIN: Duration = Duration::from_secs(30);
const RETRY_DELAY: Duration = Duration::from_secs(20);
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
    fresh_for: Duration,
    safe_until: Option<SystemTime>,
}

struct State {
    source: Option<String>,
    ready: Option<PreparedStream>,
    retry_after: Option<Instant>,
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
                    retry_after: None,
                }),
                token: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                requests,
            }),
            receiver,
        )
    }

    pub(super) fn set(&self, source: Option<String>) {
        self.set_at(source, Instant::now());
    }

    fn set_at(&self, source: Option<String>, now: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let expired = state.ready.as_ref().is_some_and(|s| !s.is_fresh());
        let retry_due =
            source.is_some() && state.retry_after.is_some_and(|deadline| now >= deadline);
        if state.source == source && !expired && !retry_due {
            return;
        }
        state.source = source.clone();
        state.ready = None;
        state.retry_after = None;
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
        state.retry_after = None;
        self.requests.send_replace(Request::default());
    }

    pub(super) fn take_for(&self, source: &str) -> Option<PreparedStream> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let result = if state.source.as_deref() == Some(source) {
            state.ready.take().filter(PreparedStream::is_fresh)
        } else {
            None
        };
        state.source = None;
        state.ready = None;
        state.retry_after = None;
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
            && state.ready.as_ref().is_some_and(PreparedStream::is_fresh)
    }

    fn current(&self, token: u64) -> bool {
        !self.closed.load(Ordering::Acquire) && self.token.load(Ordering::Acquire) == token
    }

    fn complete(&self, token: u64, stream: Option<PreparedStream>, now: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !self.current(token) {
            return;
        }
        // The timer may retry the same candidate after a bounded delay. Until
        // then it deduplicates failures just like an extraction in progress.
        state.retry_after = stream.is_none().then(|| now + RETRY_DELAY);
        state.ready = stream;
    }
}

#[cfg(windows)]
pub(super) async fn run(
    runtimes: super::RuntimePaths,
    preparation: Arc<Preparation>,
    requests: watch::Receiver<Request>,
    cookie_updates: Arc<CookieUpdates>,
) {
    let resolver_preparation = Arc::clone(&preparation);
    run_with_resolver(preparation, requests, move |request| {
        let resolver_preparation = Arc::clone(&resolver_preparation);
        let runtimes = runtimes.clone();
        let cookie_updates = Arc::clone(&cookie_updates);
        async move {
            tokio::task::spawn_blocking(move || {
                resolve(&runtimes, &request, &resolver_preparation, &cookie_updates)
            })
            .await
            .ok()
            .flatten()
        }
    })
    .await;
}

#[cfg(any(windows, test))]
async fn run_with_resolver<F, Fut>(
    preparation: Arc<Preparation>,
    mut requests: watch::Receiver<Request>,
    mut resolver: F,
) where
    F: FnMut(Request) -> Fut,
    Fut: std::future::Future<Output = Option<PreparedStream>>,
{
    loop {
        let request = requests.borrow_and_update().clone();
        if preparation.closed.load(Ordering::Acquire) {
            break;
        }
        if request.source.is_some() && preparation.current(request.token) {
            let token = request.token;
            // Await cleanup before starting another resolver: only one exists.
            let stream = resolver(request).await;
            preparation.complete(token, stream, Instant::now());
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
    cookie_updates: &CookieUpdates,
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
    let mut cookie_copy = match runtimes
        .cookies
        .as_ref()
        .map(|path| CookieCopy::create(path))
        .transpose()
    {
        Ok(copy) => copy,
        Err(_) => return None,
    };
    if let Some(cookies) = &cookie_copy {
        command.arg("--cookies").arg(cookies.path());
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
    let mut child = match super::OwnedProcess::spawn(&mut command) {
        Ok(child) => child,
        Err(_) => return None,
    };
    let stdout = child.take_stdout()?;
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_OUTPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > MAX_OUTPUT_BYTES as usize {
            return None;
        }
        Some(bytes)
    });
    let started = Instant::now();
    let completed_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None)
                if started.elapsed() < RESOLVE_TIMEOUT && preparation.current(request.token) =>
            {
                thread::sleep(Duration::from_millis(25))
            }
            _ => break None,
        }
    };
    // A launcher may exit before descendants release stdout. Kill the whole
    // owned job even on successful exit, then join the bounded reader.
    let stopped = child.terminate_tree().is_ok();
    if !stopped {
        drop(child);
    }
    // Cookie rotation belongs to the account even if the prepared track was
    // superseded or its stream metadata cannot be used anymore.
    if completed_status.is_some() && stopped {
        if let Some(copy) = &mut cookie_copy {
            copy.publish_update(runtimes.account_index, cookie_updates);
        }
    }
    let bytes = output.join().ok()??;
    if !completed_status.is_some_and(|status| status.success())
        || !preparation.current(request.token)
    {
        return None;
    }
    parse_stream(&bytes)
}

#[cfg(windows)]
pub(super) struct CookieCopy {
    path: std::path::PathBuf,
    baseline: Zeroizing<Vec<u8>>,
}

#[cfg(windows)]
impl CookieCopy {
    pub(super) fn create(source: &std::path::Path) -> std::io::Result<Self> {
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
        let mut copy = Self {
            path,
            baseline: Zeroizing::new(Vec::new()),
        };
        let result = (|| {
            std::fs::File::open(source)?
                .take(MAX_COOKIE_BYTES as u64 + 1)
                .read_to_end(&mut copy.baseline)?;
            if copy.baseline.len() > MAX_COOKIE_BYTES {
                return Err(std::io::Error::other("Cookie lease exceeds size limit"));
            }
            use std::io::Write;
            destination.write_all(&copy.baseline)?;
            Ok(())
        })();
        drop(destination);
        result?;
        Ok(copy)
    }

    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub(super) fn publish_update(&mut self, account_index: u32, queue: &CookieUpdates) -> bool {
        use std::io::Read;
        let mut updated = Zeroizing::new(Vec::new());
        let Ok(file) = std::fs::File::open(&self.path) else {
            return false;
        };
        if file
            .take(MAX_COOKIE_BYTES as u64 + 1)
            .read_to_end(&mut updated)
            .is_err()
            || updated.is_empty()
            || updated.len() > MAX_COOKIE_BYTES
        {
            return false;
        }
        let Some(update) =
            CookieUpdate::new(account_index, self.baseline.to_vec(), updated.to_vec())
        else {
            return false;
        };
        if !queue.push(update) {
            return false;
        }
        self.baseline = updated;
        true
    }
}

#[cfg(windows)]
impl Drop for CookieCopy {
    fn drop(&mut self) {
        // Windows may need a moment to release descendants' handles after the
        // owning job is cancelled. Keep cleanup bounded to the owned file.
        for attempt in 0..5 {
            match std::fs::remove_file(&self.path) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if attempt < 4 => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
    }
}

fn parse_stream(bytes: &[u8]) -> Option<PreparedStream> {
    parse_stream_at(bytes, Instant::now(), SystemTime::now())
}

fn parse_stream_at(bytes: &[u8], prepared_at: Instant, now: SystemTime) -> Option<PreparedStream> {
    let info: Value = serde_json::from_slice(bytes).ok()?;
    let url = info.get("url").and_then(Value::as_str)?;
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
    let (fresh_for, safe_until) = stream_lifetime(&parsed, now)?;
    if info.get("acodec").and_then(Value::as_str)? == "none"
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
        prepared_at,
        fresh_for,
        safe_until,
    })
}

/// Bound reuse by elapsed time and the CDN's signed deadline. A monotonic
/// lifetime prevents a backwards clock correction extending the original
/// expiry window; checking wall time also catches a forward correction.
fn stream_lifetime(url: &url::Url, now: SystemTime) -> Option<(Duration, Option<SystemTime>)> {
    let mut expiration = None;
    for (name, value) in url.query_pairs() {
        if name != "expire" {
            continue;
        }
        if expiration.is_some()
            || value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let seconds = value.parse::<u64>().ok()?;
        expiration = Some(UNIX_EPOCH.checked_add(Duration::from_secs(seconds))?);
    }
    let Some(expiration) = expiration else {
        return Some((FRESH_FOR, None));
    };
    let safe_until = expiration.checked_sub(EXPIRY_MARGIN)?;
    let remaining = safe_until.duration_since(now).ok()?;
    if remaining.is_zero() {
        return None;
    }
    Some((FRESH_FOR.min(remaining), Some(safe_until)))
}

impl PreparedStream {
    fn is_fresh(&self) -> bool {
        self.is_fresh_at(Instant::now(), SystemTime::now())
    }

    fn is_fresh_at(&self, now: Instant, wall_time: SystemTime) -> bool {
        now.saturating_duration_since(self.prepared_at) < self.fresh_for
            && self.safe_until.is_none_or(|deadline| wall_time < deadline)
    }
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

    #[tokio::test(flavor = "current_thread")]
    async fn transient_failure_retries_once_after_backoff_without_timer_hammering() {
        let (preparation, requests) = Preparation::new();
        let calls = Arc::new(AtomicU64::new(0));
        let resolver_calls = Arc::clone(&calls);
        preparation.set(Some("song".into()));
        let worker = tokio::spawn(run_with_resolver(
            Arc::clone(&preparation),
            requests,
            move |_| {
                let attempt = resolver_calls.fetch_add(1, Ordering::AcqRel);
                std::future::ready((attempt > 0).then(stream))
            },
        ));
        tokio::time::timeout(Duration::from_secs(1), async {
            while preparation.state.lock().unwrap().retry_after.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let deadline = preparation.state.lock().unwrap().retry_after.unwrap();
        let failed_token = preparation.token.load(Ordering::Acquire);
        for _ in 0..100 {
            preparation.set_at(Some("song".into()), deadline - Duration::from_nanos(1));
            tokio::task::yield_now().await;
        }
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(preparation.token.load(Ordering::Acquire), failed_token);
        assert!(!preparation.is_ready("song"));

        preparation.set_at(Some("song".into()), deadline);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !preparation.is_ready("song") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        for _ in 0..100 {
            preparation.set(Some("song".into()));
        }
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Acquire), 2);
        assert!(preparation.take_for("song").is_some());
        preparation.close();
        worker.await.unwrap();
    }

    #[test]
    fn cancelled_attempt_never_schedules_retry_for_another_candidate() {
        let (preparation, _) = Preparation::new();
        preparation.set(Some("old-song".into()));
        let stale = preparation.token.load(Ordering::Acquire);
        preparation.set(Some("new-song".into()));
        preparation.complete(stale, None, Instant::now());
        assert!(preparation.state.lock().unwrap().retry_after.is_none());
        let current = preparation.token.load(Ordering::Acquire);
        preparation.complete(current, None, Instant::now());
        assert!(preparation.state.lock().unwrap().retry_after.is_some());
        preparation.clear();
        assert!(preparation.state.lock().unwrap().retry_after.is_none());
        let stopped = preparation.token.load(Ordering::Acquire);
        preparation.set_at(None, Instant::now() + RETRY_DELAY * 2);
        assert_eq!(preparation.token.load(Ordering::Acquire), stopped);
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
    fn early_preparation_survives_minutes_without_restarting_extraction() {
        let (preparation, _) = Preparation::new();
        preparation.set(Some("song".into()));
        let token = preparation.token.load(Ordering::Acquire);
        let mut prepared = stream();
        prepared.prepared_at = Instant::now() - Duration::from_secs(5 * 60);
        preparation.state.lock().unwrap().ready = Some(prepared);
        for _ in 0..100 {
            preparation.set(Some("song".into()));
        }
        assert_eq!(preparation.token.load(Ordering::Acquire), token);
        assert!(preparation.is_ready("song"));
        assert!(preparation.take_for("song").is_some());
    }

    #[test]
    fn signed_expiry_reserves_thirty_seconds_and_caps_reuse_at_ten_minutes() {
        let now = UNIX_EPOCH + Duration::from_secs(10_000);
        let lifetime = |query: &str| {
            stream_lifetime(
                &url::Url::parse(&format!("https://r1.googlevideo.com/a?{query}")).unwrap(),
                now,
            )
        };
        assert_eq!(lifetime("other=value"), Some((FRESH_FOR, None)));
        assert!(lifetime("expire=9999").is_none());
        assert!(lifetime("expire=10029").is_none());
        assert!(lifetime("expire=10030").is_none());
        assert_eq!(
            lifetime("expire=10031"),
            Some((Duration::from_secs(1), Some(now + Duration::from_secs(1))))
        );
        assert_eq!(
            lifetime("expire=10330"),
            Some((
                Duration::from_secs(300),
                Some(now + Duration::from_secs(300))
            ))
        );
        assert_eq!(lifetime("expire=20000").unwrap().0, FRESH_FOR);
        for query in [
            "expire=",
            "expire=NaN",
            "expire=-1",
            "expire=%2B10031",
            "expire=1e10",
            "expire=18446744073709551616",
            "expire=10031&expire=20000",
        ] {
            assert!(lifetime(query).is_none(), "Malformed expiry was accepted");
        }
    }

    #[test]
    fn freshness_checks_both_elapsed_age_and_signed_wall_clock_deadline() {
        let wall_time = UNIX_EPOCH + Duration::from_secs(10_000);
        let prepared_at = Instant::now();
        let info = br#"{"url":"https://r1.googlevideo.com/a?expire=10330","acodec":"opus"}"#;
        let prepared = parse_stream_at(info, prepared_at, wall_time).unwrap();
        assert!(prepared.is_fresh_at(
            prepared_at + Duration::from_secs(299),
            wall_time + Duration::from_secs(299)
        ));
        assert!(!prepared.is_fresh_at(
            prepared_at + Duration::from_secs(300),
            wall_time + Duration::from_secs(300)
        ));
        // A forward clock correction invalidates the URL immediately.
        assert!(!prepared.is_fresh_at(prepared_at, wall_time + Duration::from_secs(300)));
        // A backwards correction cannot extend the captured expiry window.
        assert!(!prepared.is_fresh_at(prepared_at + Duration::from_secs(300), wall_time));
        let without_expiry = parse_stream_at(
            br#"{"url":"https://r1.googlevideo.com/a","acodec":"opus"}"#,
            prepared_at,
            wall_time,
        )
        .unwrap();
        assert!(without_expiry
            .is_fresh_at(prepared_at + FRESH_FOR - Duration::from_nanos(1), wall_time));
        assert!(!without_expiry.is_fresh_at(prepared_at + FRESH_FOR, wall_time));
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
        let path = copy.path().to_owned();
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

    #[cfg(windows)]
    #[test]
    fn extractor_snapshot_cannot_overwrite_refreshed_canonical_lease() {
        let directory = std::env::temp_dir().join(format!(
            "tide-cookie-refresh-race-test-{}-{}",
            std::process::id(),
            super::super::PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let canonical = directory.join("session.txt");
        std::fs::write(&canonical, b"old synthetic session").unwrap();
        let first = CookieCopy::create(&canonical).unwrap();
        let first_path = first.path().to_owned();
        let refreshed = directory.join("session.pending");
        std::fs::write(&refreshed, b"new canonical synthetic session").unwrap();
        std::fs::rename(&refreshed, &canonical).unwrap();
        // Simulate yt-dlp saving an earlier in-memory snapshot after API refresh.
        std::fs::write(first.path(), b"stale extractor synthetic session").unwrap();
        assert_eq!(
            std::fs::read(&canonical).unwrap(),
            b"new canonical synthetic session"
        );
        let next = CookieCopy::create(&canonical).unwrap();
        let next_path = next.path().to_owned();
        assert_eq!(
            std::fs::read(next.path()).unwrap(),
            b"new canonical synthetic session"
        );
        drop(first);
        drop(next);
        assert!(!first_path.exists());
        assert!(!next_path.exists());
        assert_eq!(
            std::fs::read(&canonical).unwrap(),
            b"new canonical synthetic session"
        );
        std::fs::write(&canonical, vec![b'x'; MAX_COOKIE_BYTES + 1]).unwrap();
        assert!(CookieCopy::create(&canonical).is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn completed_cookie_rotation_survives_superseded_stream_and_is_not_published_twice() {
        let directory = std::env::temp_dir().join(format!(
            "tide-cookie-publish-test-{}-{}",
            std::process::id(),
            super::super::PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let canonical = directory.join("session.txt");
        std::fs::write(&canonical, b"synthetic baseline").unwrap();
        let mut copy = CookieCopy::create(&canonical).unwrap();
        let private_path = copy.path().to_owned();
        let queue = CookieUpdates::default();
        let (preparation, _) = Preparation::new();
        preparation.set(Some("old track".into()));
        let token = preparation.token.load(Ordering::Acquire);
        preparation.set(Some("new track".into()));
        // A completed extractor can rotate credentials even though its stream
        // is no longer the requested next track. Publish before stream filtering.
        std::fs::write(copy.path(), b"synthetic rotated").unwrap();
        assert!(copy.publish_update(2, &queue));
        preparation.complete(token, Some(stream()), Instant::now());
        assert!(preparation.state.lock().unwrap().ready.is_none());
        let updates = queue.take();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].account_index, 2);
        assert_eq!(updates[0].baseline(), b"synthetic baseline");
        assert_eq!(updates[0].updated(), b"synthetic rotated");
        assert!(!copy.publish_update(2, &queue));
        // Final termination observes a further change against the advanced
        // baseline, rather than repeating the file-loaded update.
        std::fs::write(copy.path(), b"synthetic second rotation").unwrap();
        assert!(copy.publish_update(2, &queue));
        let updates = queue.take();
        assert_eq!(updates[0].baseline(), b"synthetic rotated");
        assert_eq!(updates[0].updated(), b"synthetic second rotation");
        std::fs::write(copy.path(), vec![b'x'; MAX_COOKIE_BYTES + 1]).unwrap();
        assert!(!copy.publish_update(2, &queue));
        assert!(queue.take().is_empty());
        drop(copy);
        assert!(!private_path.exists());
        assert_eq!(std::fs::read(&canonical).unwrap(), b"synthetic baseline");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
