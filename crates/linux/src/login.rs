//! Ephemeral helper-only WebKitGTK sign-in. No native bridge or persistent browser store.
use std::path::Path;
use zeroize::Zeroizing;

pub enum Outcome {
    Candidate(Zeroizing<Vec<u8>>),
    Cancelled,
    TimedOut,
    Blocked,
}
pub fn is_music_url(raw: &str) -> bool {
    secure_url(raw).is_some_and(|u| u.host_str() == Some("music.youtube.com"))
}
fn secure_url(raw: &str) -> Option<url::Url> {
    let (_, authority_and_path) = raw.split_once("://")?;
    let authority = authority_and_path.split(['/', '?', '#']).next()?;
    let u = url::Url::parse(raw).ok()?;
    // The URL parser strips empty userinfo, so check the original authority too.
    (!authority.is_empty()
        && !authority.contains(['@', '\\'])
        && !authority.bytes().any(|b| b.is_ascii_control())
        && u.scheme() == "https"
        && u.port_or_known_default() == Some(443)
        && u.username().is_empty()
        && u.password().is_none())
    .then_some(u)
}
fn allowed(raw: &str) -> bool {
    secure_url(raw).is_some_and(|u| {
        matches!(
            u.host_str(),
            Some(
                "music.youtube.com"
                    | "accounts.google.com"
                    | "consent.google.com"
                    | "youtube.com"
                    | "www.youtube.com"
                    | "consent.youtube.com"
                    | "accounts.youtube.com"
                    | "families.google.com"
                    | "familylink.google.com"
                    | "myaccount.google.com"
                    | "gds.google.com"
                    | "ogs.google.com"
            )
        )
    })
}
pub fn subframe_allowed(raw: &str) -> bool {
    matches!(raw, "about:blank" | "about:srcdoc")
        || secure_url(raw).is_some_and(|u| {
            u.host_str().is_some_and(|host| {
                matches!(
                    host,
                    "google.com"
                        | "youtube.com"
                        | "gstatic.com"
                        | "recaptcha.net"
                        | "www.recaptcha.net"
                ) || [
                    ".google.com",
                    ".youtube.com",
                    ".gstatic.com",
                    ".googleusercontent.com",
                ]
                .iter()
                .any(|suffix| {
                    host.strip_suffix(suffix)
                        .is_some_and(|prefix| !prefix.is_empty())
                })
            })
        })
}
pub fn is_google_refusal_url(raw: &str) -> bool {
    secure_url(raw).is_some_and(|u| {
        u.host_str() == Some("accounts.google.com") && {
            let path = u.path().to_ascii_lowercase();
            [
                "/signin/rejected",
                "/deniedsigninrejected",
                "/signinrejected",
            ]
            .iter()
            .any(|marker| path.contains(marker))
        }
    })
}

mod capture {
    use super::Zeroizing;
    use std::collections::{btree_map::Entry, BTreeMap};

    const MAX_HEADERS: usize = 65536;
    const MAX_COOKIE_HEADER: usize = 32768;
    pub(super) const HEADER_LIMITS: [(&str, usize); 5] = [
        ("x-goog-authuser", 1),
        ("x-goog-pageid", 128),
        ("x-goog-visitor-id", 4096),
        ("user-agent", 1024),
        ("accept-language", 1024),
    ];
    pub(super) type Headers = BTreeMap<&'static str, Zeroizing<String>>;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum CookieScope {
        Music,
        YouTube,
    }
    pub(super) fn cookie_scope(
        domain: &str,
        path: &str,
        expires_at: Option<f64>,
        now: f64,
    ) -> Option<CookieScope> {
        if path != "/" || expires_at.is_some_and(|expiry| expiry <= now || !expiry.is_finite()) {
            return None;
        }
        if domain.eq_ignore_ascii_case("music.youtube.com")
            || domain.eq_ignore_ascii_case(".music.youtube.com")
        {
            Some(CookieScope::Music)
        } else if domain.eq_ignore_ascii_case(".youtube.com") {
            Some(CookieScope::YouTube)
        } else {
            None
        }
    }
    pub(super) type ScopedCookie = (CookieScope, String, Zeroizing<String>);

    fn is_auth_cookie(name: &str) -> bool {
        matches!(
            name,
            "__Secure-3PAPISID"
                | "SAPISID"
                | "SID"
                | "HSID"
                | "SSID"
                | "APISID"
                | "SIDCC"
                | "LOGIN_INFO"
                | "__Secure-1PAPISID"
                | "__Secure-1PSID"
                | "__Secure-3PSID"
                | "__Secure-1PSIDTS"
                | "__Secure-3PSIDTS"
                | "__Secure-1PSIDCC"
                | "__Secure-3PSIDCC"
        )
    }
    fn deduplicate_cookies(
        cookies: Vec<ScopedCookie>,
    ) -> Result<BTreeMap<String, Zeroizing<String>>, &'static str> {
        let mut music = BTreeMap::new();
        let mut youtube = BTreeMap::new();
        for (scope, name, value) in cookies {
            let by_name = match scope {
                CookieScope::Music => &mut music,
                CookieScope::YouTube => &mut youtube,
            };
            match by_name.entry(name) {
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
                Entry::Occupied(entry) => {
                    if is_auth_cookie(entry.key()) && entry.get().as_str() != value.as_str() {
                        return Err("Conflicting YouTube Music sign-in cookies");
                    }
                }
            }
        }
        youtube.extend(music);
        Ok(youtube)
    }
    struct HeaderBuffer(Zeroizing<Vec<u8>>);
    impl std::io::Write for HeaderBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_HEADERS.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("Header limit exceeded"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    pub(super) fn candidate(
        cookies: Vec<ScopedCookie>,
        headers: &Headers,
    ) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        if !headers.contains_key("x-goog-authuser") {
            return Err("Music profile headers were missing");
        }
        for (name, value) in headers {
            let limit = HEADER_LIMITS
                .iter()
                .find_map(|(key, limit)| (name == key).then_some(*limit));
            if !limit.is_some_and(|limit| !value.is_empty() && value.len() <= limit)
                || !value.bytes().all(|b| (32..127).contains(&b))
                || (matches!(*name, "x-goog-authuser" | "x-goog-pageid")
                    && !value.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err("Music profile headers were invalid");
            }
        }
        let cookies = deduplicate_cookies(cookies)?;
        if cookies.len() > 128
            || !cookies
                .get("__Secure-3PAPISID")
                .is_some_and(|v| !v.is_empty())
        {
            return Err("YouTube Music sign-in cookies were missing or exceeded the limit");
        }
        let mut cookie = Zeroizing::new(String::with_capacity(MAX_COOKIE_HEADER));
        for (name, value) in cookies {
            let separator = if cookie.is_empty() { "" } else { "; " };
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || value.len() > 8192
                || !value.bytes().all(|b| (33..127).contains(&b) && b != b';')
                || cookie.len() + separator.len() + name.len() + 1 + value.len() > MAX_COOKIE_HEADER
            {
                return Err("YouTube Music sign-in cookies were invalid");
            }
            cookie.push_str(separator);
            cookie.push_str(&name);
            cookie.push('=');
            cookie.push_str(&value);
        }
        let mut fields: BTreeMap<_, _> = headers
            .iter()
            .map(|(&name, value)| (name, value.as_str()))
            .collect();
        fields.insert("cookie", cookie.as_str());
        let mut encoded = HeaderBuffer(Zeroizing::new(Vec::with_capacity(MAX_HEADERS)));
        serde_json::to_writer(&mut encoded, &fields)
            .map_err(|_| "Music profile headers exceeded the limit")?;
        Ok(encoded.0)
    }
}
pub use native::run;

mod native {
    use super::capture::{self, HEADER_LIMITS};
    use super::*;
    use gtk::prelude::*;
    use javascriptcore::ValueExt;
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use webkit2gtk::{
        CookieManagerExt, DownloadExt, LoadEvent, NavigationPolicyDecision,
        NavigationPolicyDecisionExt, NetworkError, PolicyDecision, PolicyDecisionExt,
        PolicyDecisionType, PolicyError, ResponsePolicyDecision, ResponsePolicyDecisionExt,
        SettingsExt, URIRequestExt, URIResponseExt, UserContentInjectedFrames, UserContentManager,
        UserContentManagerExt, UserScript, UserScriptInjectionTime, WebContext, WebContextExt,
        WebView, WebViewExt,
    };
    use zeroize::Zeroize;

    const PROMPT_DEFAULT: &str = "Sign in, open Library and choose your Music profile.";
    const PROMPT_READY: &str = "Choose your Music profile, then click Connect this profile.";
    const PROMPT_BLOCKED: &str = "Google blocked sign-in in this window (common for Family Link). Close it and connect from your regular browser in Tide.";
    const PROMPT_LOAD_ERROR: &str =
        "Sign-in page could not load. Retry or connect from your regular browser in Tide.";
    const START_URL: &str =
        "https://accounts.google.com/ServiceLogin?continue=https://music.youtube.com/";
    const MUSIC_URL: &str = "https://music.youtube.com/";
    const READY_SCRIPT: &str =
        "location.origin === 'https://music.youtube.com' && !!window.__tideLoginHeaders";
    const HEADERS_SCRIPT: &str = "window.__tideLoginHeaders || ''";
    const LOGIN_DEADLINE: Duration = Duration::from_secs(10 * 60);
    const PROBE_DEADLINE: Duration = Duration::from_secs(30);

    #[derive(Default)]
    struct State {
        web: Option<WebView>,
        context: Option<WebContext>,
        button: Option<gtk::Button>,
        label: Option<gtk::Label>,
        reading: bool,
        blocked: bool,
        generation: u64,
        done: bool,
        result: Option<Result<Outcome, String>>,
    }
    thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

    fn set_status(text: &str) {
        let label = STATE.with(|s| s.borrow().label.clone());
        if let Some(label) = label {
            label.set_text(text);
        }
    }
    fn set_button(enabled: bool) {
        let button = STATE.with(|s| s.borrow().button.clone());
        if let Some(button) = button {
            button.set_sensitive(enabled);
        }
    }
    fn mark_blocked() {
        STATE.with(|s| s.borrow_mut().blocked = true);
        set_status(PROMPT_BLOCKED);
    }
    fn finish(result: Result<Outcome, String>) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.done {
                s.done = true;
                s.result = Some(result);
            }
        });
    }
    fn active_web() -> Option<WebView> {
        STATE.with(|s| {
            let s = s.borrow();
            (!s.done).then(|| s.web.clone()).flatten()
        })
    }

    /// WebKitGTK reports navigation actions for both the main frame and
    /// subframes without saying which. Admit either list here; the response
    /// decision below knows the main frame and enforces the strict list there,
    /// before anything from a non-allowed origin can be committed.
    fn decide_navigation(decision: &PolicyDecision) {
        let raw = decision
            .downcast_ref::<NavigationPolicyDecision>()
            .and_then(|navigation| navigation.navigation_action())
            .and_then(|action| action.request())
            .and_then(|request| request.uri())
            .map(|uri| uri.to_string())
            .unwrap_or_default();
        if allowed(&raw) || subframe_allowed(&raw) {
            decision.use_();
        } else {
            decision.ignore();
        }
    }
    /// window.open and target=_blank load in place, only when the target is a
    /// permitted main-frame origin. No second window is ever created.
    fn decide_new_window(web: &WebView, decision: &PolicyDecision) {
        let raw = decision
            .downcast_ref::<NavigationPolicyDecision>()
            .and_then(|navigation| navigation.navigation_action())
            .and_then(|action| action.request())
            .and_then(|request| request.uri())
            .map(|uri| uri.to_string())
            .unwrap_or_default();
        decision.ignore();
        if allowed(&raw) && active_web().is_some() {
            web.load_uri(&raw);
        }
    }
    fn decide_response(decision: &PolicyDecision) {
        let Some(response) = decision.downcast_ref::<ResponsePolicyDecision>() else {
            decision.ignore();
            return;
        };
        let is_main = response.is_main_frame_main_resource();
        let reply = response.response();
        let raw = reply
            .as_ref()
            .and_then(|reply| reply.uri())
            .map(|uri| uri.to_string())
            .unwrap_or_default();
        let trusted = if is_main {
            allowed(&raw)
        } else {
            subframe_allowed(&raw)
        };
        let about_subframe = !is_main && matches!(raw.as_str(), "about:blank" | "about:srcdoc");
        let attachment = match reply.as_ref().and_then(|reply| reply.http_headers()) {
            Some(headers) => headers
                .one("Content-Disposition")
                .is_some_and(|d| d.to_ascii_lowercase().contains("attachment")),
            None => !about_subframe,
        };
        if trusted && is_main && is_google_refusal_url(&raw) {
            mark_blocked();
        }
        if trusted && response.is_mime_type_supported() && !attachment {
            decision.use_();
        } else {
            decision.ignore();
        }
    }
    fn handle_load_error(error: &glib::Error) {
        // Expected cancellations: a newer navigation, or a response this helper ignored.
        if error.matches(NetworkError::Cancelled)
            || error.matches(PolicyError::FrameLoadInterruptedByPolicyChange)
        {
            return;
        }
        let blocked = STATE.with(|s| s.borrow().blocked);
        if !blocked {
            set_status(PROMPT_LOAD_ERROR);
        }
    }
    fn update_ready() {
        let Some(web) = STATE.with(|s| {
            let s = s.borrow();
            if s.reading || s.done {
                None
            } else {
                s.web.clone()
            }
        }) else {
            return;
        };
        if web.uri().is_some_and(|u| is_google_refusal_url(&u)) {
            mark_blocked();
        }
        web.evaluate_javascript(
            READY_SCRIPT,
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            |result| {
                let ready = result.is_ok_and(|value| value.is_boolean() && value.to_boolean());
                let (blocked, active, enabled) = STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    if ready {
                        s.blocked = false;
                    }
                    (s.blocked, !s.done, ready && !s.reading && !s.done)
                });
                set_button(enabled);
                if active {
                    if ready {
                        set_status(PROMPT_READY);
                    } else if blocked {
                        set_status(PROMPT_BLOCKED);
                    }
                }
            },
        );
    }
    // WebKit has no public HTTPS request observer. Observe only the five permitted
    // browse-request headers in page JS after an HTTP 200 completion; never expose
    // a native message handler.
    const OBSERVER: &str = r#"(() => {
      let latest = null;
      const names = ['x-goog-authuser','x-goog-pageid','x-goog-visitor-id','user-agent','accept-language'];
      const extract = (url, method, headers) => {
        try {
          if (location.origin !== 'https://music.youtube.com') return null;
          const u = new URL(url, location.href);
          if (u.origin !== 'https://music.youtube.com' || u.pathname !== '/youtubei/v1/browse' || String(method).toUpperCase() !== 'POST') return null;
          const h = new Headers(headers), selected = {};
          for (const n of names) if (h.has(n)) selected[n] = h.get(n);
          return /^[0-9]$/.test(selected['x-goog-authuser'] || '') ? selected : null;
        } catch (_) { return null; }
      };
      const fetch = window.fetch;
      window.fetch = function(input, init) {
        const rawUrl = typeof input === 'string' ? input : (input instanceof URL ? input.href : input?.url);
        const method = init?.method || (input && typeof input === 'object' ? input.method : null) || 'GET';
        const headers = init?.headers || (input && typeof input === 'object' ? input.headers : null);
        const pending = extract(rawUrl, method, headers);
        const promise = fetch.apply(this, arguments);
        if (pending) {
          promise.then(res => { if (res && res.status === 200) latest = pending; }).catch(() => {});
        }
        return promise;
      };
      const open = XMLHttpRequest.prototype.open, set = XMLHttpRequest.prototype.setRequestHeader, send = XMLHttpRequest.prototype.send;
      const requests = new WeakMap();
      XMLHttpRequest.prototype.open = function(method,url) {
        const rawUrl = url instanceof URL ? url.href : String(url || '');
        requests.set(this,{method,url:rawUrl,headers:{}});
        return open.apply(this,arguments);
      };
      XMLHttpRequest.prototype.setRequestHeader = function(n,v) { const r=requests.get(this); if(r) r.headers[n]=v; return set.apply(this,arguments); };
      XMLHttpRequest.prototype.send = function() {
        const r = requests.get(this);
        const pending = r ? extract(r.url, r.method, r.headers) : null;
        if (pending) {
          this.addEventListener('load', () => { if (this.status === 200) latest = pending; }, { once: true });
        }
        return send.apply(this,arguments);
      };
      Object.defineProperty(window,'__tideLoginHeaders',{
        get:() => (location.origin === 'https://music.youtube.com' && latest)
          ? JSON.stringify({...latest,'user-agent':latest['user-agent'] || navigator.userAgent})
          : '',
        configurable:false
      });
    })()"#;
    fn connect() {
        let snapshot = STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.done || s.reading {
                return None;
            }
            s.reading = true;
            s.web.clone().map(|w| (w, s.generation))
        });
        let Some((web, generation)) = snapshot else {
            return;
        };
        set_button(false);
        if !web.uri().is_some_and(|u| is_music_url(&u)) {
            STATE.with(|s| s.borrow_mut().reading = false);
            return;
        }
        web.evaluate_javascript(
            HEADERS_SCRIPT,
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            move |result| {
                let Ok(value) = result else {
                    finish(Err(
                        "Open Library and choose your Music profile before connecting".into(),
                    ));
                    return;
                };
                if !value.is_string() {
                    finish(Err("Music profile headers were invalid".into()));
                    return;
                }
                let raw = {
                    let text = value.to_str();
                    Zeroizing::new(text.as_str().to_owned())
                };
                if raw.len() > 65536 {
                    finish(Err("Music profile headers exceeded the limit".into()));
                    return;
                }
                let Ok(mut raw_headers) = serde_json::from_str::<BTreeMap<String, String>>(&raw)
                else {
                    STATE.with(|s| s.borrow_mut().reading = false);
                    return;
                };
                let mut headers: capture::Headers = BTreeMap::new();
                for (k, mut v) in std::mem::take(&mut raw_headers) {
                    if let Some(&(allowed_key, limit)) =
                        HEADER_LIMITS.iter().find(|(name, _)| *name == k.as_str())
                    {
                        if !v.is_empty()
                            && v.len() <= limit
                            && v.bytes().all(|b| (32..127).contains(&b))
                        {
                            headers.insert(allowed_key, Zeroizing::new(std::mem::take(&mut v)));
                        }
                    }
                    v.zeroize();
                }
                read_cookies(headers, generation);
            },
        );
    }
    fn read_cookies(headers: capture::Headers, generation: u64) {
        let manager = STATE.with(|s| {
            let s = s.borrow();
            if s.done || s.generation != generation {
                None
            } else {
                s.context
                    .as_ref()
                    .and_then(|context| context.cookie_manager())
            }
        });
        let Some(manager) = manager else {
            finish(Err("Music profile changed during connection".into()));
            return;
        };
        // Cookies the ephemeral session would send to music.youtube.com,
        // including HttpOnly ones; cookie_scope keeps only the two permitted scopes.
        manager.cookies(MUSIC_URL, None::<&gtk::gio::Cancellable>, move |result| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let mut selected: Vec<capture::ScopedCookie> = Vec::new();
            let list = result.unwrap_or_default();
            if list.len() <= 4096 {
                for mut cookie in list {
                    let expires_at = cookie.expires().map(|date| date.to_unix() as f64);
                    let (Some(domain), Some(path)) = (cookie.domain(), cookie.path()) else {
                        continue;
                    };
                    let Some(scope) = capture::cookie_scope(&domain, &path, expires_at, now) else {
                        continue;
                    };
                    let (Some(name), Some(value)) = (cookie.name(), cookie.value()) else {
                        continue;
                    };
                    selected.push((
                        scope,
                        name.as_str().to_owned(),
                        Zeroizing::new(value.as_str().to_owned()),
                    ));
                }
            }
            let unchanged = STATE.with(|s| {
                let s = s.borrow();
                !s.done && s.generation == generation
            });
            let result = capture::candidate(selected, &headers);
            if unchanged {
                finish(result.map(Outcome::Candidate).map_err(str::to_owned));
            } else {
                finish(Err("Music profile changed during connection".into()));
            }
        });
    }
    fn offline_cookie_probe() -> Result<(), String> {
        let now = 1_700_000_000.0;
        if capture::cookie_scope(".google.com", "/", None, now).is_some()
            || capture::cookie_scope("youtube.com", "/", None, now).is_some()
            || capture::cookie_scope(".youtube.com", "/", Some(now - 10.0), now).is_some()
        {
            return Err("Offline cookie scope probe failed".into());
        }
        let youtube_scope = capture::cookie_scope(".youtube.com", "/", Some(now + 3600.0), now)
            .ok_or("Offline cookie scope probe rejected .youtube.com")?;
        let music_scope = capture::cookie_scope("music.youtube.com", "/", None, now)
            .ok_or("Offline cookie scope probe rejected music.youtube.com")?;
        let cookies = vec![
            (
                youtube_scope,
                "__Secure-3PAPISID".into(),
                Zeroizing::new("yt".into()),
            ),
            (
                music_scope,
                "__Secure-3PAPISID".into(),
                Zeroizing::new("music".into()),
            ),
        ];
        let headers = BTreeMap::from([("x-goog-authuser", Zeroizing::new("1".into()))]);
        let candidate = capture::candidate(cookies, &headers).map_err(str::to_owned)?;
        let doc: serde_json::Value =
            serde_json::from_slice(&candidate).map_err(|_| "Offline candidate JSON invalid")?;
        if doc["x-goog-authuser"] != "1" || doc["cookie"] != "__Secure-3PAPISID=music" {
            return Err("Offline cookie candidate mismatch".into());
        }
        Ok(())
    }
    pub fn run(_: &Path, probe: bool) -> Result<Outcome, String> {
        if probe {
            offline_cookie_probe()?;
        }
        gtk::init().map_err(|_| "Native sign-in needs a graphical desktop session")?;
        STATE.with(|s| *s.borrow_mut() = State::default());

        // Memory-only cookies, cache and storage; nothing reaches the profile path.
        let context = WebContext::new_ephemeral();
        context.connect_download_started(|_, download| download.cancel());
        let content = UserContentManager::new();
        content.add_script(&UserScript::new(
            OBSERVER,
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        let web = WebView::builder()
            .web_context(&context)
            .user_content_manager(&content)
            .build();
        if !web.is_ephemeral() {
            return Err("Native sign-in could not create a private browser session".into());
        }
        if let Some(settings) = WebViewExt::settings(&web) {
            settings.set_javascript_can_open_windows_automatically(false);
        }
        web.connect_decide_policy(|web, decision, kind| {
            match kind {
                PolicyDecisionType::NavigationAction => decide_navigation(decision),
                PolicyDecisionType::NewWindowAction => decide_new_window(web, decision),
                PolicyDecisionType::Response => decide_response(decision),
                _ => decision.ignore(),
            }
            true
        });
        web.connect_load_changed(|web, event| {
            if matches!(event, LoadEvent::Started | LoadEvent::Redirected) {
                STATE.with(|s| s.borrow_mut().generation += 1);
            }
            if web.uri().is_some_and(|u| is_google_refusal_url(&u)) {
                mark_blocked();
            }
        });
        web.connect_load_failed(|_, _, _, error| {
            handle_load_error(error);
            false
        });
        web.connect_web_process_terminated(|_, _| {
            set_status(PROMPT_LOAD_ERROR);
            set_button(false);
        });

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Connect YouTube Music to Tide");
        window.set_default_size(800, 800);
        window.set_position(gtk::WindowPosition::Center);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let bar = gtk::Box::new(gtk::Orientation::Vertical, 6);
        bar.set_margin_top(10);
        bar.set_margin_start(16);
        bar.set_margin_end(16);
        let label = gtk::Label::new(Some(PROMPT_DEFAULT));
        label.set_line_wrap(true);
        label.set_xalign(0.0);
        let button = gtk::Button::with_label("Connect this profile");
        button.set_halign(gtk::Align::Start);
        button.set_sensitive(false);
        button.connect_clicked(|_| connect());
        bar.pack_start(&label, false, false, 0);
        bar.pack_start(&button, false, false, 0);
        root.pack_start(&bar, false, false, 0);
        web.set_vexpand(true);
        root.pack_start(&web, true, true, 0);
        window.add(&root);
        window.connect_delete_event(|_, _| {
            let blocked = STATE.with(|s| s.borrow().blocked);
            finish(Ok(if blocked {
                Outcome::Blocked
            } else {
                Outcome::Cancelled
            }));
            glib::Propagation::Proceed
        });
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.web = Some(web.clone());
            s.context = Some(context.clone());
            s.button = Some(button);
            s.label = Some(label);
        });

        if probe {
            // Start the WebKit web process on a blank page without any network
            // traffic, prove it finishes loading, then quit. The window stays hidden.
            web.connect_load_changed(|web, event| {
                if event == LoadEvent::Finished && web.uri().is_some_and(|u| u == "about:blank") {
                    finish(Ok(Outcome::Cancelled));
                }
            });
            web.load_uri("about:blank");
        } else {
            web.load_uri(START_URL);
            window.show_all();
            window.present();
        }
        let deadline = Instant::now()
            + if probe {
                PROBE_DEADLINE
            } else {
                LOGIN_DEADLINE
            };
        let tick = glib::timeout_add_local(Duration::from_secs(1), move || {
            if Instant::now() >= deadline {
                finish(if probe {
                    Err("WebKitGTK did not load a blank page".into())
                } else {
                    Ok(Outcome::TimedOut)
                });
            } else if !probe {
                update_ready();
            }
            glib::ControlFlow::Continue
        });
        // Pump GTK on the helper main thread; no player event loop exists here.
        while !STATE.with(|s| s.borrow().done) {
            gtk::main_iteration_do(true);
        }
        tick.remove();
        web.stop_loading();
        window.hide();
        // SAFETY: the window and its children are not used after this point;
        // the State references are cleared below.
        unsafe { window.destroy() };
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        STATE.with(|s| {
            let result = s
                .borrow_mut()
                .result
                .take()
                .unwrap_or_else(|| Err("Native sign-in could not finish".into()));
            *s.borrow_mut() = State::default();
            result
        })
    }
}
#[cfg(test)]
mod tests {
    use super::capture::*;
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn origins_subframes_and_refusals() {
        assert!(is_music_url("https://music.youtube.com/library"));
        for u in [
            "https://accounts.youtube.com/accounts/SetSID",
            "https://families.google.com/supervision",
            "https://familylink.google.com/",
            "https://myaccount.google.com/",
            "https://gds.google.com/web/chip",
            "https://ogs.google.com/u/0/widget/account",
        ] {
            assert!(allowed(u), "expected main-frame host allowed: {u}");
        }
        for u in [
            "http://music.youtube.com/",
            "https://user@music.youtube.com/",
            "https://@music.youtube.com/",
            "https://music.youtube.com:444/",
            "https://accounts.google.com.evil.org/",
            "file:///tmp/a",
        ] {
            assert!(!allowed(u), "expected main-frame host rejected: {u}");
        }
        for u in [
            "about:blank",
            "about:srcdoc",
            "https://www.gstatic.com/recaptcha/api2/anchor",
            "https://lh3.googleusercontent.com/frame",
            "https://www.recaptcha.net/recaptcha/api2/bframe",
            "https://ogs.google.com/widget",
        ] {
            assert!(subframe_allowed(u), "expected subframe allowed: {u}");
        }
        for u in [
            "http://www.gstatic.com/a",
            "https://evil.example/",
            "https://google.com.evil.org/",
            "javascript:alert(1)",
        ] {
            assert!(!subframe_allowed(u), "expected subframe rejected: {u}");
        }
        assert!(is_google_refusal_url(
            "https://accounts.google.com/v3/signin/rejected?continue=https://music.youtube.com/"
        ));
        assert!(is_google_refusal_url(
            "https://accounts.google.com/signin/v2/deniedsigninrejected"
        ));
        assert!(!is_google_refusal_url(
            "https://accounts.google.com/ServiceLogin?continue=https://music.youtube.com/"
        ));
        assert!(!is_google_refusal_url(
            "https://evil.example/v3/signin/rejected"
        ));
    }

    #[test]
    fn cookie_scope_deduplication_and_candidate_contract() {
        let now = 1_700_000_000.0;
        assert_eq!(
            cookie_scope("music.youtube.com", "/", None, now),
            Some(CookieScope::Music)
        );
        assert_eq!(
            cookie_scope(".YOUTUBE.COM", "/", Some(now + 60.0), now),
            Some(CookieScope::YouTube)
        );
        // Never leak Google-domain, bare youtube.com, subpath, or expired cookies.
        for (domain, path, exp) in [
            (".google.com", "/", None),
            ("google.com", "/", None),
            ("youtube.com", "/", None),
            (".youtube.com", "/watch", None),
            (".youtube.com", "/", Some(now - 1.0)),
            (".youtube.com", "/", Some(f64::NAN)),
        ] {
            assert_eq!(cookie_scope(domain, path, exp, now), None);
        }

        // Missing authuser fails.
        let base_cookies = vec![(
            CookieScope::YouTube,
            "__Secure-3PAPISID".into(),
            Zeroizing::new("fixture".into()),
        )];
        assert!(candidate(base_cookies.clone(), &BTreeMap::new()).is_err());

        // Host-specific music.youtube.com overrides .youtube.com cleanly.
        let layered = vec![
            (
                CookieScope::YouTube,
                "__Secure-3PAPISID".into(),
                Zeroizing::new("old-yt".into()),
            ),
            (
                CookieScope::Music,
                "__Secure-3PAPISID".into(),
                Zeroizing::new("music-specific".into()),
            ),
        ];
        let headers = BTreeMap::from([("x-goog-authuser", Zeroizing::new("1".into()))]);
        let encoded = candidate(layered, &headers).unwrap();
        assert_eq!(
            &*encoded,
            concat!(
                r#"{"cookie":"__Secure-3PAPISID"#,
                r#"=music-specific","x-goog-authuser":"1"}"#
            )
            .as_bytes()
        );

        // Conflicting auth cookies within the same scope are rejected.
        let conflicting = vec![
            (
                CookieScope::YouTube,
                "__Secure-3PAPISID".into(),
                Zeroizing::new("one".into()),
            ),
            (
                CookieScope::YouTube,
                "__Secure-3PAPISID".into(),
                Zeroizing::new("two".into()),
            ),
        ];
        assert!(candidate(conflicting, &headers).is_err());

        // Provider-aligned header length limits reject oversized user-agent.
        let oversized = BTreeMap::from([
            ("x-goog-authuser", Zeroizing::new("0".into())),
            ("user-agent", Zeroizing::new("a".repeat(1025))),
        ]);
        assert!(candidate(base_cookies, &oversized).is_err());
    }
}
