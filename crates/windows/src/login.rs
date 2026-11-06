//! Opt-in native login prototype. Run only in an owned helper process tree.
//! Pages have no native bridge. Credentials never enter stdout, argv or files here.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
use webview2_com::{
    BrowserProcessExitedEventHandler, CoTaskMemPWSTR, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DownloadStartingEventHandler, GetCookiesCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    NewWindowRequestedEventHandler, SourceChangedEventHandler, WebResourceRequestedEventHandler,
};
use windows::Win32::Graphics::Gdi::{GetSysColorBrush, COLOR_WINDOW};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::{
    core::{w, Interface, BOOL, HSTRING, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        System::{Com::*, LibraryLoader::GetModuleHandleW},
        UI::WindowsAndMessaging::*,
    },
};
use zeroize::{Zeroize, Zeroizing};

const CONNECT: usize = 101;
const TIMER: usize = 1;
const FINISH: u32 = WM_APP + 17;
const MAX_HEADERS: usize = 65536;
const START: &str = "https://accounts.google.com/ServiceLogin?continue=https://music.youtube.com/";
pub enum Outcome {
    Candidate(Zeroizing<Vec<u8>>),
    Cancelled,
    TimedOut,
}
#[derive(Default)]
struct State {
    hwnd: HWND,
    button: HWND,
    status: HWND,
    environment: Option<ICoreWebView2Environment>,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    metadata: BTreeMap<String, String>,
    user_agent: String,
    music: bool,
    reading: bool,
    closing: bool,
    exited: bool,
    failed: bool,
    failure: Option<String>,
    deadline: Option<Instant>,
    timed_out: bool,
    candidate: Option<Zeroizing<Vec<u8>>>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
fn fail<T>(_: T) -> String {
    "Native sign-in could not finish".into()
}

pub fn is_music_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("music.youtube.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
    })
}

/// The caller must own the helper's process tree and stop it before dropping the profile.
/// Probe mode loads only about:blank, verifies hardening and closes without credentials.
pub fn run(profile: &Path, probe: bool) -> Result<Outcome, String> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(fail)?;
    let result = run_window(profile, probe);
    let (candidate, failed, exited, failure, timed_out) = STATE.with(|state| {
        let mut state = state.borrow_mut();
        (
            state.candidate.take(),
            state.failed,
            state.exited,
            state.failure.take(),
            state.timed_out,
        )
    });
    STATE.with(|state| *state.borrow_mut() = State::default());
    unsafe {
        CoUninitialize();
    }
    result?;
    if let Some(failure) = failure {
        return Err(failure);
    }
    if failed || !exited {
        return Err("Native browser shutdown was not confirmed".into());
    }
    Ok(candidate.map_or(
        if timed_out {
            Outcome::TimedOut
        } else {
            Outcome::Cancelled
        },
        Outcome::Candidate,
    ))
}

fn run_window(profile: &Path, probe: bool) -> Result<(), String> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(fail)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: w!("TideNativeSignIn"),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&class);
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("TideNativeSignIn"),
            w!("Connect YouTube Music to Tide"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            680,
            820,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
    }
    .map_err(fail)?;
    let status = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("Sign in, open Library and choose your Music profile."),
            WS_CHILD | WS_VISIBLE,
            16,
            720,
            400,
            48,
            Some(hwnd),
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
    }
    .map_err(fail)?;
    let button = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            w!("Connect this profile"),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            440,
            720,
            200,
            36,
            Some(hwnd),
            Some(HMENU(CONNECT as *mut _)),
            Some(HINSTANCE(instance.0)),
            None,
        )
    }
    .map_err(fail)?;
    STATE.with(|state| {
        *state.borrow_mut() = State {
            hwnd,
            status,
            button,
            deadline: Some(Instant::now() + Duration::from_secs(8 * 60)),
            ..Default::default()
        }
    });
    unsafe {
        SetTimer(Some(hwnd), TIMER, 250, None);
    }
    if !probe {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }
    if let Err(error) = create_webview(hwnd, profile, probe) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.failed = true;
            state.failure = Some(error);
        });
        begin_close();
    }
    if probe {
        begin_close();
    }
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == 0 {
            break;
        }
        if result == -1 {
            return Err(fail(()));
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn create_webview(hwnd: HWND, profile: &Path, probe: bool) -> Result<(), String> {
    let folder = HSTRING::from(profile.as_os_str());
    let (tx, rx) = std::sync::mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            let options: ICoreWebView2EnvironmentOptions =
                CoreWebView2EnvironmentOptions::default().into();
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(folder.as_ptr()),
                &options,
                &handler,
            )
            .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |hr, environment| {
            hr?;
            let _ = tx.send(environment);
            Ok(())
        }),
    )
    .map_err(|_| "WebView2 environment could not start".to_owned())?;
    let environment = rx.recv().map_err(fail)?.ok_or_else(|| fail(()))?;
    let mut token = 0;
    unsafe {
        environment
            .cast::<ICoreWebView2Environment5>()
            .map_err(fail)?
            .add_BrowserProcessExited(
                &BrowserProcessExitedEventHandler::create(Box::new(move |_, _| {
                    STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        if !state.closing {
                            state.failed = true;
                        }
                        state.exited = true;
                    });
                    let _ = PostMessageW(Some(hwnd), FINISH, WPARAM(0), LPARAM(0));
                    Ok(())
                })),
                &mut token,
            )
            .map_err(fail)?;
    }
    STATE.with(|state| state.borrow_mut().environment = Some(environment.clone()));
    let (tx, rx) = std::sync::mpsc::channel();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            environment
                .CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |hr, controller| {
            hr?;
            let _ = tx.send(controller);
            Ok(())
        }),
    )
    .map_err(|_| "WebView2 controller could not start".to_owned())?;
    let controller = rx.recv().map_err(fail)?.ok_or_else(|| fail(()))?;
    unsafe { controller.SetIsVisible(!probe) }.map_err(fail)?;
    let webview = unsafe { controller.CoreWebView2() }.map_err(fail)?;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.controller = Some(controller.clone());
        state.webview = Some(webview.clone());
    });
    unsafe {
        let settings = webview.Settings().map_err(fail)?;
        settings.SetIsWebMessageEnabled(false).map_err(fail)?;
        settings.SetAreHostObjectsAllowed(false).map_err(fail)?;
        let settings4 = settings.cast::<ICoreWebView2Settings4>().map_err(fail)?;
        settings4
            .SetIsPasswordAutosaveEnabled(false)
            .map_err(fail)?;
        settings4.SetIsGeneralAutofillEnabled(false).map_err(fail)?;
        let mut agent = PWSTR::null();
        settings
            .cast::<ICoreWebView2Settings2>()
            .map_err(fail)?
            .UserAgent(&mut agent)
            .map_err(fail)?;
        let agent = CoTaskMemPWSTR::from(agent).to_string();
        if agent.len() > 512 || !agent.bytes().all(|byte| (32..127).contains(&byte)) {
            return Err(fail(()));
        }
        STATE.with(|state| state.borrow_mut().user_agent = agent);
        let mut flag = BOOL(1);
        settings.IsWebMessageEnabled(&mut flag).map_err(fail)?;
        if flag.as_bool() {
            return Err(fail(()));
        }
        settings.AreHostObjectsAllowed(&mut flag).map_err(fail)?;
        if flag.as_bool() {
            return Err(fail(()));
        }
        settings4
            .IsPasswordAutosaveEnabled(&mut flag)
            .map_err(fail)?;
        if flag.as_bool() {
            return Err(fail(()));
        }
        settings4
            .IsGeneralAutofillEnabled(&mut flag)
            .map_err(fail)?;
        if flag.as_bool() {
            return Err(fail(()));
        }
        webview
            .add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(fail)?;
        webview
            .cast::<ICoreWebView2_4>()
            .map_err(fail)?
            .add_DownloadStarting(
                &DownloadStartingEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetCancel(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(fail)?;
        webview
            .add_SourceChanged(
                &SourceChangedEventHandler::create(Box::new(|sender, args| {
                    if let Some(webview) = sender {
                        let mut new_document = BOOL(0);
                        if let Some(args) = args {
                            args.IsNewDocument(&mut new_document)?;
                        }
                        let mut raw = PWSTR::null();
                        webview.Source(&mut raw)?;
                        let source = CoTaskMemPWSTR::from(raw).to_string();
                        STATE.with(|state| {
                            let mut state = state.borrow_mut();
                            state.music = is_music_url(&source);
                            if !state.music || new_document.as_bool() {
                                state.metadata.clear();
                            }
                        });
                        update_controls();
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(fail)?;
        webview
            .AddWebResourceRequestedFilter(
                w!("https://music.youtube.com/youtubei/v1/browse*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            )
            .map_err(fail)?;
        webview
            .add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        observe_request(&args)?;
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(fail)?;
        if probe {
            offline_cookie_probe(&webview)?;
        }
        let destination = HSTRING::from(if probe { "about:blank" } else { START });
        webview
            .Navigate(PCWSTR(destination.as_ptr()))
            .map_err(fail)?;
    }
    resize();
    update_controls();
    Ok(())
}

unsafe fn observe_request(
    args: &ICoreWebView2WebResourceRequestedEventArgs,
) -> windows::core::Result<()> {
    let request = unsafe { args.Request()? };
    let mut raw = PWSTR::null();
    unsafe {
        request.Uri(&mut raw)?;
    }
    let uri = CoTaskMemPWSTR::from(raw).to_string();
    if !is_music_url(&uri)
        || !url::Url::parse(&uri).is_ok_and(|url| url.path() == "/youtubei/v1/browse")
    {
        return Ok(());
    }
    unsafe {
        request.Method(&mut raw)?;
    }
    if CoTaskMemPWSTR::from(raw).to_string() != "POST" {
        return Ok(());
    }
    let headers = unsafe { request.Headers()? };
    let mut selected = BTreeMap::new();
    for name in [
        "x-goog-authuser",
        "x-goog-pageid",
        "x-goog-visitor-id",
        "user-agent",
        "accept-language",
    ] {
        raw = PWSTR::null();
        let key = HSTRING::from(name);
        if unsafe { headers.GetHeader(PCWSTR(key.as_ptr()), &mut raw) }.is_ok() {
            let value = CoTaskMemPWSTR::from(raw).to_string();
            let safe = value.len() <= 4096 && value.bytes().all(|b| (32..127).contains(&b));
            if safe {
                selected.insert(name.to_string(), value);
            }
        }
    }
    if !selected
        .get("x-goog-authuser")
        .is_some_and(|value| value.len() == 1 && value.bytes().all(|b| b.is_ascii_digit()))
    {
        return Ok(());
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.user_agent.is_empty() {
            selected
                .entry("user-agent".into())
                .or_insert_with(|| state.user_agent.clone());
        }
        state.metadata = selected;
    });
    update_controls();
    Ok(())
}

fn update_controls() {
    let (button, status, ready, closing) = STATE.with(|state| {
        let state = state.borrow();
        (
            state.button,
            state.status,
            state.music && state.metadata.contains_key("x-goog-authuser") && !state.reading,
            state.closing,
        )
    });
    unsafe {
        let _ = EnableWindow(button, ready && !closing);
        let _ = SetWindowTextW(
            status,
            if ready {
                w!("Choose your Music profile, then connect.")
            } else {
                w!("Sign in, open Library and choose your Music profile.")
            },
        );
    }
}

fn read_candidate() {
    let webview = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.music || state.metadata.is_empty() || state.reading || state.closing {
            return None;
        }
        state.reading = true;
        let metadata = state.metadata.clone();
        state.webview.clone().map(|webview| (webview, metadata))
    });
    let Some((webview, metadata)) = webview else {
        return;
    };
    update_controls();
    let result = unsafe {
        webview
            .cast::<ICoreWebView2_2>()
            .and_then(|view| view.CookieManager())
            .and_then(|manager| {
                manager.GetCookies(
                    w!("https://music.youtube.com/"),
                    &GetCookiesCompletedHandler::create(Box::new(move |hr, cookies| {
                        let candidate = hr
                            .and_then(|_| {
                                cookies.ok_or_else(|| windows::core::Error::from(E_POINTER))
                            })
                            .ok()
                            .and_then(|cookies| cookie_candidate(&cookies, metadata.clone()));
                        STATE.with(|state| {
                            let mut state = state.borrow_mut();
                            if !state.closing {
                                state.candidate = candidate;
                                state.failed = state.candidate.is_none();
                            }
                        });
                        post_finish();
                        Ok(())
                    })),
                )
            })
    };
    if result.is_err() {
        STATE.with(|state| state.borrow_mut().failed = true);
        post_finish();
    }
}

fn cookie_candidate(
    list: &ICoreWebView2CookieList,
    mut headers: BTreeMap<String, String>,
) -> Option<Zeroizing<Vec<u8>>> {
    if !headers
        .get("x-goog-authuser")
        .is_some_and(|value| value.len() == 1 && value.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let mut count = 0;
    unsafe {
        list.Count(&mut count).ok()?;
    }
    if count > 128 {
        return None;
    }
    let mut cookie = Zeroizing::new(String::with_capacity(MAX_HEADERS));
    let mut authenticated = false;
    for index in 0..count {
        let entry = unsafe { list.GetValueAtIndex(index).ok()? };
        let (mut name, mut value) = (PWSTR::null(), PWSTR::null());
        unsafe {
            entry.Name(&mut name).ok()?;
        }
        let name = CoTaskMemPWSTR::from(name).to_string();
        unsafe {
            entry.Value(&mut value).ok()?;
        }
        let value = Zeroizing::new(CoTaskMemPWSTR::from(value).to_string());
        if name.len() > 128
            || name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || value.len() > 8192
            || !value.bytes().all(|b| (33..127).contains(&b) && b != b';')
        {
            return None;
        }
        if name == "__Secure-3PAPISID" && !value.is_empty() {
            authenticated = true;
        }
        if cookie.len() + name.len() + value.len() + 3 > 32768 {
            return None;
        }
        if !cookie.is_empty() {
            cookie.push_str("; ");
        }
        cookie.push_str(&name);
        cookie.push('=');
        cookie.push_str(&value);
    }
    if !authenticated {
        return None;
    }
    headers.insert("cookie".into(), std::mem::take(&mut *cookie));
    let mut encoded = HeaderBuffer(Zeroizing::new(Vec::with_capacity(MAX_HEADERS)));
    let result = serde_json::to_writer(&mut encoded, &headers);
    for value in headers.values_mut() {
        value.zeroize();
    }
    result.is_ok().then_some(encoded.0)
}

// Exercise cookie extraction without a Google request or any real account data.
fn offline_cookie_probe(webview: &ICoreWebView2) -> Result<(), String> {
    let manager = unsafe {
        webview
            .cast::<ICoreWebView2_2>()
            .and_then(|view| view.CookieManager())
    }
    .map_err(fail)?;
    let cookie = unsafe {
        manager.CreateCookie(
            w!("__Secure-3PAPISID"),
            w!("tide-offline-fixture"),
            w!(".youtube.com"),
            w!("/"),
        )
    }
    .map_err(fail)?;
    unsafe { cookie.SetIsSecure(true) }.map_err(fail)?;
    unsafe { manager.AddOrUpdateCookie(&cookie) }.map_err(fail)?;
    let (tx, rx) = std::sync::mpsc::channel();
    GetCookiesCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            manager
                .GetCookies(w!("https://music.youtube.com/"), &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |hr, cookies| {
            hr?;
            let _ = tx.send(cookies);
            Ok(())
        }),
    )
    .map_err(fail)?;
    let cookies = rx.recv().map_err(fail)?.ok_or_else(|| fail(()))?;
    let metadata = BTreeMap::from([("x-goog-authuser".to_owned(), "1".to_owned())]);
    // The account chosen at Connect is independent of later page/request changes.
    STATE.with(|state| {
        state
            .borrow_mut()
            .metadata
            .insert("x-goog-authuser".into(), "2".into())
    });
    let candidate = cookie_candidate(&cookies, metadata).ok_or_else(|| fail(()))?;
    let document: serde_json::Value = serde_json::from_slice(&candidate).map_err(fail)?;
    if document["x-goog-authuser"] != "1"
        || !document["cookie"]
            .as_str()
            .is_some_and(|value| value.contains("tide-offline-fixture"))
    {
        return Err(fail(()));
    }
    if cookie_candidate(&cookies, BTreeMap::new()).is_some() {
        return Err(fail(()));
    }
    STATE.with(|state| state.borrow_mut().metadata.clear());
    Ok(())
}

// Reject before a serializer can grow/reallocate a plaintext credential buffer.
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

fn post_finish() {
    let hwnd = STATE.with(|state| state.borrow().hwnd);
    unsafe {
        let _ = PostMessageW(Some(hwnd), FINISH, WPARAM(0), LPARAM(0));
    }
}
fn begin_close() {
    let controller = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.closing {
            return None;
        }
        state.closing = true;
        state.deadline = Some(Instant::now() + Duration::from_secs(5));
        state.webview = None;
        state.controller.take()
    });
    if let Some(controller) = controller {
        unsafe {
            let _ = controller.Close();
        }
    }
}
fn resize() {
    let (hwnd, button, status, controller) = STATE.with(|state| {
        let state = state.borrow();
        (
            state.hwnd,
            state.button,
            state.status,
            state.controller.clone(),
        )
    });
    let mut rect = RECT::default();
    unsafe {
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }
        let bottom = (rect.bottom - 64).max(0);
        let _ = MoveWindow(
            status,
            16,
            bottom + 10,
            (rect.right - 250).max(40),
            48,
            true,
        );
        let _ = MoveWindow(
            button,
            (rect.right - 220).max(0),
            bottom + 12,
            204,
            36,
            true,
        );
        if let Some(controller) = controller {
            rect.bottom = bottom;
            let _ = controller.SetBounds(rect);
        }
    }
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = std::panic::catch_unwind(|| match message {
        WM_SIZE => {
            resize();
            Some(LRESULT(0))
        }
        WM_COMMAND if wparam.0 & 0xffff == CONNECT => {
            read_candidate();
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            begin_close();
            Some(LRESULT(0))
        }
        FINISH => {
            let closing = STATE.with(|state| state.borrow().closing);
            if !closing {
                begin_close();
            }
            Some(LRESULT(0))
        }
        WM_TIMER if wparam.0 == TIMER => {
            let destroy = STATE.with(|state| {
                let mut state = state.borrow_mut();
                if state.closing && state.exited {
                    return true;
                }
                if state
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                    && state.closing
                {
                    state.failed = true;
                    return true;
                }
                false
            });
            if destroy {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            } else if STATE.with(|state| {
                state
                    .borrow()
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            }) {
                STATE.with(|state| state.borrow_mut().timed_out = true);
                begin_close();
            }
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            Some(LRESULT(0))
        }
        _ => None,
    });
    result
        .ok()
        .flatten()
        .unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, message, wparam, lparam) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn music_identity_never_matches_lookalike_or_untrusted_origins() {
        assert!(is_music_url("https://music.youtube.com/library"));
        for url in [
            "http://music.youtube.com/",
            "https://music.youtube.com.evil.invalid/",
            "https://user@music.youtube.com/",
            "https://music.youtube.com:444/",
            "file:///music.youtube.com",
        ] {
            assert!(!is_music_url(url));
        }
    }
    #[test]
    fn credential_buffer_rejects_overflow_before_reallocation() {
        use std::io::Write;
        let mut buffer = HeaderBuffer(Zeroizing::new(Vec::with_capacity(MAX_HEADERS)));
        let capacity = buffer.0.capacity();
        buffer.write_all(&vec![b'x'; MAX_HEADERS]).unwrap();
        assert!(buffer.write_all(b"x").is_err());
        assert_eq!(buffer.0.len(), MAX_HEADERS);
        assert_eq!(buffer.0.capacity(), capacity);
    }
}
