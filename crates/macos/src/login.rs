//! Ephemeral helper-only WebKit sign-in. No native bridge or persistent browser store.
use std::{collections::BTreeMap, path::Path};
use zeroize::{Zeroize, Zeroizing};

pub enum Outcome {
    Candidate(Zeroizing<Vec<u8>>),
    Cancelled,
    TimedOut,
}
pub fn is_music_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| secure(&u) && u.host_str() == Some("music.youtube.com"))
}
fn secure(u: &url::Url) -> bool {
    u.scheme() == "https"
        && u.port_or_known_default() == Some(443)
        && u.username().is_empty()
        && u.password().is_none()
}
fn allowed(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        secure(&u)
            && matches!(
                u.host_str(),
                Some(
                    "music.youtube.com"
                        | "accounts.google.com"
                        | "consent.google.com"
                        | "youtube.com"
                        | "www.youtube.com"
                        | "consent.youtube.com"
                )
            )
    })
}
const MAX_HEADERS: usize = 65536;
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
fn candidate(
    cookies: &[(String, Zeroizing<String>)],
    mut headers: BTreeMap<String, String>,
) -> Option<Zeroizing<Vec<u8>>> {
    let result = (|| {
        if !headers
            .get("x-goog-authuser")
            .is_some_and(|v| v.len() == 1 && v.bytes().all(|b| b.is_ascii_digit()))
            || cookies.len() > 128
        {
            return None;
        }
        let mut cookie = Zeroizing::new(String::with_capacity(MAX_HEADERS));
        let mut authenticated = false;
        for (name, value) in cookies {
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || value.len() > 8192
                || !value.bytes().all(|b| (33..127).contains(&b) && b != b';')
                || cookie.len() + name.len() + value.len() + 3 > 32768
            {
                return None;
            }
            authenticated |= name == "__Secure-3PAPISID" && !value.is_empty();
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(name);
            cookie.push('=');
            cookie.push_str(value);
        }
        if !authenticated {
            return None;
        }
        headers.insert("cookie".into(), std::mem::take(&mut *cookie));
        let mut encoded = HeaderBuffer(Zeroizing::new(Vec::with_capacity(MAX_HEADERS)));
        serde_json::to_writer(&mut encoded, &headers).ok()?;
        Some(encoded.0)
    })();
    for value in headers.values_mut() {
        value.zeroize();
    }
    result
}
#[cfg(not(target_os = "macos"))]
pub fn run(_: &Path, _: bool) -> Result<Outcome, String> {
    Err("Native sign-in requires macOS".into())
}
#[cfg(target_os = "macos")]
pub use native::run;

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use block2::{Block, RcBlock};
    use objc2::{
        class, define_class, msg_send,
        rc::{Allocated, Retained},
        runtime::{AnyObject, Bool},
        sel, MainThreadMarker, MainThreadOnly,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
    use objc2_web_kit as _;
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };
    type Obj = Retained<AnyObject>;
    #[derive(Default)]
    struct State {
        web: Option<Obj>,
        button: Option<Obj>,
        reading: bool,
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
                    STATE.with(|s| { let mut s = s.borrow_mut(); s.generation += 1; });
                    handler.call((if allowed(&raw) && !download.as_bool() { 1 } else { 0 },));
                }
            }
            #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
            fn response(&self, _: &AnyObject, response: &AnyObject, handler: &Block<dyn Fn(isize)>) {
                unsafe {
                    let show: Bool = msg_send![response, canShowMIMEType];
                    handler.call((if show.as_bool() { 1 } else { 0 },));
                }
            }
            #[unsafe(method(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
            fn new_window(&self, _: &AnyObject, _: &AnyObject, _: &AnyObject, _: &AnyObject) -> *mut AnyObject { std::ptr::null_mut() }
        }
    );
    fn string(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }
    unsafe fn url_string(u: &AnyObject) -> String {
        let s: Retained<NSString> = msg_send![u, absoluteString];
        s.to_string()
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
            let callback = RcBlock::new(|value: *mut AnyObject, error: *mut AnyObject| {
                let ready = error.is_null() && !value.is_null() && {
                    let value: Bool = msg_send![value, boolValue];
                    value.as_bool()
                };
                STATE.with(|s| {
                    let s = s.borrow();
                    if let Some(button) = &s.button {
                        let _: () =
                            msg_send![&**button, setEnabled: ready && !s.reading && !s.done];
                    }
                });
            });
            let _: () = msg_send![&*web, evaluateJavaScript: &*string("location.origin === 'https://music.youtube.com' && !!window.__tideLoginHeaders"), completionHandler: &*callback];
        }
    }
    // WebKit has no public HTTPS request observer. Observe only the five permitted
    // browse-request headers in page JS; never expose a native message handler.
    const OBSERVER: &str = r#"(() => {
      let latest = null;
      const names = ['x-goog-authuser','x-goog-pageid','x-goog-visitor-id','user-agent','accept-language'];
      const record = (url, method, headers) => {
        try { const u = new URL(url, location.href);
          if (u.origin !== 'https://music.youtube.com' || u.pathname !== '/youtubei/v1/browse' || String(method).toUpperCase() !== 'POST') return;
          const h = new Headers(headers), selected = {};
          for (const n of names) if (h.has(n)) selected[n] = h.get(n);
          if (/^[0-9]$/.test(selected['x-goog-authuser'] || '')) latest = selected;
        } catch (_) {}
      };
      const fetch = window.fetch;
      window.fetch = function(input, init) { record(typeof input === 'string' ? input : input.url, init?.method || input.method || 'GET', init?.headers || input.headers); return fetch.apply(this, arguments); };
      const open = XMLHttpRequest.prototype.open, set = XMLHttpRequest.prototype.setRequestHeader, send = XMLHttpRequest.prototype.send;
      const requests = new WeakMap();
      XMLHttpRequest.prototype.open = function(method,url) { requests.set(this,{method,url,headers:{}}); return open.apply(this,arguments); };
      XMLHttpRequest.prototype.setRequestHeader = function(n,v) { const r=requests.get(this); if(r) r.headers[n]=v; return set.apply(this,arguments); };
      XMLHttpRequest.prototype.send = function() { const r=requests.get(this); if(r) record(r.url,r.method,r.headers); return send.apply(this,arguments); };
      Object.defineProperty(window,'__tideLoginHeaders',{get:()=>latest ? JSON.stringify({...latest,'user-agent':navigator.userAgent,'accept-language':navigator.language}) : '',configurable:false});
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
                let value = &*(value as *const NSString);
                let raw = Zeroizing::new(value.to_string());
                if raw.len() > MAX_HEADERS {
                    finish(Err("Music profile headers exceeded the limit".into()));
                    return;
                }
                let Ok(headers) = serde_json::from_str::<BTreeMap<String, String>>(&raw) else {
                    STATE.with(|s| s.borrow_mut().reading = false);
                    return;
                };
                let mut headers = headers;
                headers.retain(|k, v| {
                    matches!(
                        k.as_str(),
                        "x-goog-authuser"
                            | "x-goog-pageid"
                            | "x-goog-visitor-id"
                            | "user-agent"
                            | "accept-language"
                    ) && v.len() <= 4096
                        && v.bytes().all(|b| (32..127).contains(&b))
                });
                read_cookies(headers, generation);
            });
            let _: () = msg_send![&*web, evaluateJavaScript: &*string("window.__tideLoginHeaders || ''"), completionHandler: &*callback];
        }
    }
    unsafe fn read_cookies(mut headers: BTreeMap<String, String>, generation: u64) {
        let web = STATE.with(|s| {
            let s = s.borrow();
            if s.done || s.generation != generation {
                None
            } else {
                s.web.clone()
            }
        });
        let Some(web) = web else {
            for v in headers.values_mut() {
                v.zeroize();
            }
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
            let count: usize = msg_send![list, count];
            let mut selected = Vec::new();
            if count <= 4096 {
                for i in 0..count {
                    let cookie: Obj = msg_send![list, objectAtIndex: i];
                    let domain: Retained<NSString> = msg_send![&*cookie, domain];
                    if !matches!(
                        domain.to_string().as_str(),
                        "music.youtube.com"
                            | ".music.youtube.com"
                            | "youtube.com"
                            | ".youtube.com"
                            | "google.com"
                            | ".google.com"
                    ) {
                        continue;
                    }
                    let path: Retained<NSString> = msg_send![&*cookie, path];
                    if path.to_string() != "/" {
                        continue;
                    }
                    let name: Retained<NSString> = msg_send![&*cookie, name];
                    let value: Retained<NSString> = msg_send![&*cookie, value];
                    selected.push((name.to_string(), Zeroizing::new(value.to_string())));
                }
            }
            let unchanged = STATE.with(|s| {
                let s = s.borrow();
                !s.done && s.generation == generation
            });
            let candidate = candidate(&selected, headers);
            if unchanged {
                finish(
                    candidate
                        .map(Outcome::Candidate)
                        .ok_or_else(|| "Native sign-in could not finish".into()),
                );
            } else {
                finish(Err("Music profile changed during connection".into()));
            }
        });
        let _: () = msg_send![&*cookies, getAllCookies: &*callback];
    }
    pub fn run(_: &Path, probe: bool) -> Result<Outcome, String> {
        let mtm = MainThreadMarker::new().ok_or("Native sign-in requires the main thread")?;
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
            let web: Obj = msg_send![web, initWithFrame: NSRect::new(NSPoint::new(0.,80.),NSSize::new(800.,720.)), configuration: &*config];
            let _: () = msg_send![&*web, setNavigationDelegate: &*delegate];
            let _: () = msg_send![&*web, setUIDelegate: &*delegate];
            let _: () = msg_send![&*root, addSubview: &*web];
            let label: Obj = msg_send![class!(NSTextField), labelWithString: &*string("Sign in, open Library and choose your Music profile.")];
            let _: () = msg_send![&*label, setFrame: NSRect::new(NSPoint::new(16.,42.),NSSize::new(760.,25.))];
            let _: () = msg_send![&*root, addSubview: &*label];
            let button: Obj = msg_send![class!(NSButton), buttonWithTitle: &*string("Connect this profile"), target: &*delegate, action: sel!(connect:)];
            let _: () = msg_send![&*button, setFrame: NSRect::new(NSPoint::new(16.,8.),NSSize::new(210.,30.))];
            let _: () = msg_send![&*root, addSubview: &*button];
            let _: () = msg_send![&*button, setEnabled: false];
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.web = Some(web.clone());
                s.button = Some(button);
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
                    finish(Ok(Outcome::Cancelled));
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
    use super::*;
    #[test]
    fn origins_and_candidate_contract() {
        assert!(is_music_url("https://music.youtube.com/library"));
        for u in [
            "http://music.youtube.com/",
            "https://user@music.youtube.com/",
            "https://music.youtube.com:444/",
            "https://accounts.google.com.evil.org/",
            "file:///tmp/a",
        ] {
            assert!(!allowed(u));
        }
        let cookies = vec![("__Secure-3PAPISID".into(), Zeroizing::new("fixture".into()))];
        assert!(candidate(&cookies, BTreeMap::new()).is_none());
        let c = candidate(
            &cookies,
            BTreeMap::from([("x-goog-authuser".into(), "1".into())]),
        )
        .unwrap();
        assert_eq!(
            &*c,
            concat!(
                r#"{"cookie":"__Secure-3PAPISID"#,
                r#"=fixture","x-goog-authuser":"1"}"#
            )
            .as_bytes()
        );
    }
}
