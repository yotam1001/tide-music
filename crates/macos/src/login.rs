//! Ephemeral helper-only WebKit sign-in. No native bridge or persistent browser store.
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
#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "macos", test))]
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
#[cfg(not(target_os = "macos"))]
pub fn run(_: &Path, _: bool) -> Result<Outcome, String> {
    Err("Native sign-in requires macOS".into())
}
#[cfg(target_os = "macos")]
pub use native::run;

#[cfg(target_os = "macos")]
mod native {
    use super::capture::{self, HEADER_LIMITS};
    use super::*;
    use block2::{Block, RcBlock};
    use objc2::{
        class, define_class, msg_send,
        rc::{Allocated, Retained},
        runtime::{AnyObject, Bool},
        sel, MainThreadMarker, MainThreadOnly,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
    use objc2_web_kit::{WKNavigationDelegate, WKUIDelegate};
    use std::{
        cell::RefCell,
        collections::BTreeMap,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use zeroize::Zeroize;
    type Obj = Retained<AnyObject>;
    const PROMPT_DEFAULT: &str = "Sign in, open Library and choose your Music profile.";
    const PROMPT_READY: &str = "Choose your Music profile, then click Connect this profile.";
    const PROMPT_BLOCKED: &str = "Google blocked sign-in in this window (common for Family Link). Close it and connect from your regular browser in Tide.";
    const PROMPT_LOAD_ERROR: &str =
        "Sign-in page could not load. Retry or connect from your regular browser in Tide.";

    #[derive(Default)]
    struct State {
        web: Option<Obj>,
        button: Option<Obj>,
        label: Option<Obj>,
        reading: bool,
        blocked: bool,
        generation: u64,
        done: bool,
        result: Option<Result<Outcome, String>>,
    }
    thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        struct Delegate;
        unsafe impl NSObjectProtocol for Delegate {}
        unsafe impl WKNavigationDelegate for Delegate {}
        unsafe impl WKUIDelegate for Delegate {}
        impl Delegate {
            #[unsafe(method(connect:))]
            fn connect(&self, _: Option<&AnyObject>) { connect(); }
            #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
            fn action(&self, _: &AnyObject, action: &AnyObject, handler: &Block<dyn Fn(isize)>) {
                unsafe {
                    let request: Obj = msg_send![action, request];
                    let url: Option<Obj> = msg_send![&*request, URL];
                    let raw = url.map(|u| url_string(&u)).unwrap_or_default();
                    let download: Bool = msg_send![action, shouldPerformDownload];
                    let target: Option<Obj> = msg_send![action, targetFrame];
                    let is_main = target
                        .map(|frame| {
                            let main: Bool = msg_send![&*frame, isMainFrame];
                            main.as_bool()
                        })
                        .unwrap_or(true);
                    let permitted = !download.as_bool()
                        && if is_main {
                            allowed(&raw)
                        } else {
                            subframe_allowed(&raw)
                        };
                    if permitted && is_main {
                        STATE.with(|s| {
                            let mut s = s.borrow_mut();
                            s.generation += 1;
                        });
                        if is_google_refusal_url(&raw) {
                            mark_blocked();
                        }
                    }
                    handler.call((if permitted { 1 } else { 0 },));
                }
            }
            #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
            fn response(&self, _: &AnyObject, response: &AnyObject, handler: &Block<dyn Fn(isize)>) {
                unsafe {
                    let show: Bool = msg_send![response, canShowMIMEType];
                    let is_main: Bool = msg_send![response, isForMainFrame];
                    let reply: Obj = msg_send![response, response];
                    let url: Option<Obj> = msg_send![&*reply, URL];
                    let raw = url.map(|u| url_string(&u)).unwrap_or_default();
                    let trusted = if is_main.as_bool() {
                        allowed(&raw)
                    } else {
                        subframe_allowed(&raw)
                    };
                    let http: Bool = msg_send![&*reply, isKindOfClass: class!(NSHTTPURLResponse)];
                    let about_subframe = !is_main.as_bool() && matches!(raw.as_str(), "about:blank" | "about:srcdoc");
                    let attachment = if http.as_bool() {
                        let disposition: Option<Retained<NSString>> = msg_send![&*reply, valueForHTTPHeaderField: &*string("Content-Disposition")];
                        disposition.is_some_and(|d| d.to_string().to_ascii_lowercase().contains("attachment"))
                    } else {
                        !about_subframe
                    };
                    if trusted && is_main.as_bool() && is_google_refusal_url(&raw) {
                        mark_blocked();
                    }
                    handler.call((if trusted && show.as_bool() && !attachment { 1 } else { 0 },));
                }
            }
            #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
            fn fail_provisional(&self, _: &AnyObject, _: Option<&AnyObject>, error: &AnyObject) {
                unsafe { handle_load_error(error); }
            }
            #[unsafe(method(webView:didFailNavigation:withError:))]
            fn fail_navigation(&self, _: &AnyObject, _: Option<&AnyObject>, error: &AnyObject) {
                unsafe { handle_load_error(error); }
            }
            #[unsafe(method(webViewWebContentProcessDidTerminate:))]
            fn content_terminated(&self, _: &AnyObject) {
                unsafe {
                    set_status(PROMPT_LOAD_ERROR);
                    STATE.with(|s| {
                        let s = s.borrow();
                        if let Some(button) = &s.button {
                            let _: () = msg_send![&**button, setEnabled: false];
                        }
                    });
                }
            }
            #[unsafe(method(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
            fn new_window(&self, web: &AnyObject, _: &AnyObject, action: &AnyObject, _: &AnyObject) -> *mut AnyObject {
                unsafe {
                    let target: Option<Obj> = msg_send![action, targetFrame];
                    if target.is_none() {
                        let request: Obj = msg_send![action, request];
                        let url: Option<Obj> = msg_send![&*request, URL];
                        let raw = url.map(|u| url_string(&u)).unwrap_or_default();
                        if allowed(&raw) {
                            let _: Option<Obj> = msg_send![web, loadRequest: &*request];
                        }
                    }
                }
                std::ptr::null_mut()
            }
        }
    );
    fn string(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }
    unsafe fn url_string(u: &AnyObject) -> String {
        let s: Retained<NSString> = msg_send![u, absoluteString];
        s.to_string()
    }
    unsafe fn set_status(text: &str) {
        STATE.with(|s| {
            let s = s.borrow();
            if let Some(label) = &s.label {
                let _: () = msg_send![&**label, setStringValue: &*string(text)];
            }
        });
    }
    unsafe fn mark_blocked() {
        STATE.with(|s| s.borrow_mut().blocked = true);
        set_status(PROMPT_BLOCKED);
    }
    unsafe fn handle_load_error(error: &AnyObject) {
        let code: isize = msg_send![error, code];
        // Ignore expected cancellations (NSURLErrorCancelled = -999, WKErrorFrameLoadInterruptedByPolicyChange = 102).
        if code == -999 || code == 102 {
            return;
        }
        let blocked = STATE.with(|s| s.borrow().blocked);
        if !blocked {
            set_status(PROMPT_LOAD_ERROR);
        }
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
    fn update_ready() {
        let web = STATE.with(|s| {
            let s = s.borrow();
            if s.reading || s.done {
                None
            } else {
                s.web.clone()
            }
        });
        let Some(web) = web else {
            return;
        };
        unsafe {
            let current_url: Option<Obj> = msg_send![&*web, URL];
            if current_url.is_some_and(|u| is_google_refusal_url(&url_string(&u))) {
                mark_blocked();
            }
            let callback = RcBlock::new(|value: *mut AnyObject, error: *mut AnyObject| {
                let ready = error.is_null() && !value.is_null() && {
                    let value: Bool = msg_send![value, boolValue];
                    value.as_bool()
                };
                let (blocked, active) = STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    if ready {
                        s.blocked = false;
                    }
                    if let Some(button) = &s.button {
                        let _: () =
                            msg_send![&**button, setEnabled: ready && !s.reading && !s.done];
                    }
                    (s.blocked, !s.done)
                });
                if active {
                    if ready {
                        set_status(PROMPT_READY);
                    } else if blocked {
                        set_status(PROMPT_BLOCKED);
                    }
                }
            });
            let _: () = msg_send![&*web, evaluateJavaScript: &*string("location.origin === 'https://music.youtube.com' && !!window.__tideLoginHeaders"), completionHandler: &*callback];
        }
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
        unsafe {
            let url: Option<Obj> = msg_send![&*web, URL];
            if !url.is_some_and(|u| is_music_url(&url_string(&u))) {
                STATE.with(|s| s.borrow_mut().reading = false);
                return;
            }
            let callback = RcBlock::new(move |value: *mut AnyObject, error: *mut AnyObject| {
                if !error.is_null() || value.is_null() {
                    finish(Err(
                        "Open Library and choose your Music profile before connecting".into(),
                    ));
                    return;
                }
                let is_string: Bool = msg_send![value, isKindOfClass: class!(NSString)];
                if !is_string.as_bool() {
                    finish(Err("Music profile headers were invalid".into()));
                    return;
                }
                let value = &*(value as *const NSString);
                let raw = Zeroizing::new(value.to_string());
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
            });
            let _: () = msg_send![&*web, evaluateJavaScript: &*string("window.__tideLoginHeaders || ''"), completionHandler: &*callback];
        }
    }
    unsafe fn read_cookies(headers: capture::Headers, generation: u64) {
        let web = STATE.with(|s| {
            let s = s.borrow();
            if s.done || s.generation != generation {
                None
            } else {
                s.web.clone()
            }
        });
        let Some(web) = web else {
            finish(Err("Music profile changed during connection".into()));
            return;
        };
        let config: Obj = msg_send![&*web, configuration];
        let store: Obj = msg_send![&*config, websiteDataStore];
        let cookies: Obj = msg_send![&*store, httpCookieStore];
        let headers = RefCell::new(Some(headers));
        let callback = RcBlock::new(move |list: *mut AnyObject| {
            let Some(headers) = headers.borrow_mut().take() else {
                return;
            };
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let count: usize = msg_send![list, count];
            let mut selected: Vec<capture::ScopedCookie> = Vec::new();
            if count <= 4096 {
                for i in 0..count {
                    let cookie: Obj = msg_send![list, objectAtIndex: i];
                    let domain: Retained<NSString> = msg_send![&*cookie, domain];
                    let path: Retained<NSString> = msg_send![&*cookie, path];
                    let expires_date: Option<Obj> = msg_send![&*cookie, expiresDate];
                    let expires_at = expires_date.map(|date| {
                        let ts: f64 = msg_send![&*date, timeIntervalSince1970];
                        ts
                    });
                    let Some(scope) = capture::cookie_scope(
                        &domain.to_string(),
                        &path.to_string(),
                        expires_at,
                        now,
                    ) else {
                        continue;
                    };
                    let name: Retained<NSString> = msg_send![&*cookie, name];
                    let value: Retained<NSString> = msg_send![&*cookie, value];
                    selected.push((scope, name.to_string(), Zeroizing::new(value.to_string())));
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
        let _: () = msg_send![&*cookies, getAllCookies: &*callback];
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
        let mtm = MainThreadMarker::new().ok_or("Native sign-in requires the main thread")?;
        if probe {
            offline_cookie_probe()?;
        }
        STATE.with(|s| *s.borrow_mut() = State::default());
        unsafe {
            let app: Obj = msg_send![class!(NSApplication), sharedApplication];
            let _: Bool = msg_send![&*app, setActivationPolicy: 0isize];
            let _: () = msg_send![&*app, finishLaunching];
            let delegate: Retained<Delegate> = msg_send![Delegate::alloc(mtm), init];
            let config: Obj = msg_send![class!(WKWebViewConfiguration), new];
            let store: Obj = msg_send![class!(WKWebsiteDataStore), nonPersistentDataStore];
            let _: () = msg_send![&*config, setWebsiteDataStore: &*store];
            let content: Obj = msg_send![&*config, userContentController];
            let script: Allocated<AnyObject> = msg_send![class!(WKUserScript), alloc];
            let script: Obj = msg_send![script, initWithSource: &*string(OBSERVER), injectionTime: 0isize, forMainFrameOnly: true];
            let _: () = msg_send![&*content, addUserScript: &*script];
            let preferences: Obj = msg_send![&*config, preferences];
            let _: () = msg_send![&*preferences, setJavaScriptCanOpenWindowsAutomatically: false];
            let frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(800., 800.));
            let window: Allocated<AnyObject> = msg_send![class!(NSWindow), alloc];
            let window: Obj = msg_send![window, initWithContentRect: frame, styleMask: 3usize, backing: 2usize, defer: false];
            let _: () = msg_send![&*window, setReleasedWhenClosed: false];
            let _: () = msg_send![&*window, setTitle: &*string("Connect YouTube Music to Tide")];
            let root: Obj = msg_send![&*window, contentView];
            let web: Allocated<AnyObject> = msg_send![class!(WKWebView), alloc];
            let web: Obj = msg_send![web, initWithFrame: NSRect::new(NSPoint::new(0.,96.),NSSize::new(800.,704.)), configuration: &*config];
            let _: () = msg_send![&*web, setNavigationDelegate: &*delegate];
            let _: () = msg_send![&*web, setUIDelegate: &*delegate];
            let _: () = msg_send![&*root, addSubview: &*web];
            let label: Obj =
                msg_send![class!(NSTextField), wrappingLabelWithString: &*string(PROMPT_DEFAULT)];
            let _: () = msg_send![&*label, setFrame: NSRect::new(NSPoint::new(16.,44.),NSSize::new(768.,42.))];
            let _: () = msg_send![&*root, addSubview: &*label];
            let button: Obj = msg_send![class!(NSButton), buttonWithTitle: &*string("Connect this profile"), target: &*delegate, action: sel!(connect:)];
            let _: () = msg_send![&*button, setFrame: NSRect::new(NSPoint::new(16.,8.),NSSize::new(210.,30.))];
            let _: () = msg_send![&*root, addSubview: &*button];
            let _: () = msg_send![&*button, setEnabled: false];
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.web = Some(web.clone());
                s.button = Some(button);
                s.label = Some(label);
            });
            if !probe {
                let url: Obj = msg_send![class!(NSURL), URLWithString: &*string("https://accounts.google.com/ServiceLogin?continue=https://music.youtube.com/")];
                let request: Obj = msg_send![class!(NSURLRequest), requestWithURL: &*url];
                let _: Option<Obj> = msg_send![&*web, loadRequest: &*request];
                let _: () = msg_send![&*window, center];
                let _: () =
                    msg_send![&*window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                #[allow(deprecated)]
                let _: () = msg_send![&*app, activateIgnoringOtherApps: true];
            }
            let mut readiness = Instant::now();
            let deadline = Instant::now() + Duration::from_secs(if probe { 1 } else { 8 * 60 });
            // Pump NSApplication on the helper main thread; no player event loop exists here.
            while !STATE.with(|s| s.borrow().done) {
                objc2::rc::autoreleasepool(|_| {
                    let date: Obj =
                        msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 0.05f64];
                    let event: Option<Obj> = msg_send![&*app, nextEventMatchingMask: usize::MAX, untilDate: &*date, inMode: &*string("kCFRunLoopDefaultMode"), dequeue: true];
                    if let Some(event) = event {
                        let _: () = msg_send![&*app, sendEvent: &*event];
                    }
                    let _: () = msg_send![&*app, updateWindows];
                });
                if !probe && readiness.elapsed() >= Duration::from_millis(500) {
                    update_ready();
                    readiness = Instant::now();
                }
                let visible: Bool = msg_send![&*window, isVisible];
                if !probe && !visible.as_bool() {
                    let blocked = STATE.with(|s| s.borrow().blocked);
                    finish(Ok(if blocked {
                        Outcome::Blocked
                    } else {
                        Outcome::Cancelled
                    }));
                }
                if Instant::now() >= deadline {
                    finish(Ok(if probe {
                        Outcome::Cancelled
                    } else {
                        Outcome::TimedOut
                    }));
                }
            }
            let _: () = msg_send![&*web, stopLoading];
            let _: () = msg_send![&*web, setNavigationDelegate: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*web, setUIDelegate: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*window, close];
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
