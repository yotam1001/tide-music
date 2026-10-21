//! Generated certificates and trusted roots exist in this test module only.
//! The proxy fixture forwards real TLS/HTTP traffic to the real relay; it is
//! not a validation of a particular production reverse-proxy installation.
use super::*;
use crate::relay::{self, PrivateRelay, SharedRelay};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

struct Backend {
    address: SocketAddr,
    stop: tokio::sync::watch::Sender<bool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Backend {
    fn start(config: PrivateRelay) -> Self {
        Self::spawn(Some(config), None)
    }
    fn start_shared(config: SharedRelay) -> Self {
        Self::spawn(None, Some(config))
    }
    fn spawn(private: Option<PrivateRelay>, shared: Option<SharedRelay>) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let (stop, receiver) = tokio::sync::watch::channel(false);
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    tx.send(listener.local_addr().unwrap()).unwrap();
                    if let Some(config) = private {
                        relay::serve_private(listener, receiver, config)
                            .await
                            .unwrap();
                    } else {
                        relay::serve_shared(listener, receiver, shared.unwrap())
                            .await
                            .unwrap();
                    }
                });
        });
        Self {
            address: rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            stop,
            worker: Some(worker),
        }
    }
    fn raw(&self, host: &str, target: &str, extra: &str, auth: Option<&Secret>) -> Vec<u8> {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut request = Zeroizing::new(format!("POST {target} HTTP/1.1\r\nHost: {host}\r\nx-tide-jam-protocol: 1\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n{extra}"));
        if let Some(auth) = auth {
            request.push_str(&format!("Authorization: Bearer {}\r\n", auth.expose()));
        }
        request.push_str("\r\n{}");
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream
            .take((MAX_BODY + 8192) as u64)
            .read_to_end(&mut response);
        response
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.worker.take().unwrap().join().unwrap();
    }
}

struct Proxy {
    endpoint: String,
    cert: Vec<u8>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    backend: Backend,
}
impl Proxy {
    fn start(operator: Secret, san: &str, expired: bool, redirect: Option<String>) -> Self {
        Self::start_inner(Some(operator), san, expired, redirect)
    }
    fn start_shared() -> Self {
        Self::start_inner(None, "127.0.0.1", false, None)
    }
    fn start_inner(
        operator: Option<Secret>,
        san: &str,
        expired: bool,
        redirect: Option<String>,
    ) -> Self {
        let shared_mode = operator.is_none();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("https://{}", listener.local_addr().unwrap());
        let backend = match operator {
            Some(operator) => Backend::start(PrivateRelay::new(&endpoint, operator).unwrap()),
            None => Backend::start_shared(SharedRelay::new(&endpoint).unwrap()),
        };
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec![san.to_owned()]).unwrap();
        if expired {
            params.not_before = rcgen::date_time_ymd(2019, 1, 1);
            params.not_after = rcgen::date_time_ymd(2020, 1, 1);
        }
        let cert = params.self_signed(&key).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
        let config = Arc::new(config);
        let cert = cert.der().to_vec();
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let address = backend.address;
        let worker = thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut tls = rustls::StreamOwned::new(
                            rustls::ServerConnection::new(config.clone()).unwrap(),
                            socket,
                        );
                        let mut request = Zeroizing::new(Vec::new());
                        let mut byte = [0];
                        while request.len() < 8192 && !request.ends_with(b"\r\n\r\n") {
                            if tls.read_exact(&mut byte).is_err() {
                                break;
                            }
                            request.push(byte[0]);
                        }
                        if !request.ends_with(b"\r\n\r\n") {
                            continue;
                        }
                        let head = std::str::from_utf8(&request).unwrap();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .map_or(0, |value| value.parse::<usize>().unwrap());
                        assert!(length <= MAX_BODY);
                        let start = request.len();
                        request.resize(start + length, 0);
                        if tls.read_exact(&mut request[start..]).is_err() {
                            continue;
                        }
                        if shared_mode {
                            request.splice(
                                start - 2..start - 2,
                                b"x-tide-jam-peer: 203.0.113.7\r\n".iter().copied(),
                            );
                        }
                        if let Some(location) = &redirect {
                            let response = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            let _ = tls.write_all(response.as_bytes());
                        } else {
                            let mut upstream = TcpStream::connect(address).unwrap();
                            upstream
                                .set_read_timeout(Some(Duration::from_secs(4)))
                                .unwrap();
                            upstream.write_all(&request).unwrap();
                            let mut response = Zeroizing::new(Vec::new());
                            upstream
                                .take((MAX_BODY + 8192) as u64)
                                .read_to_end(&mut response)
                                .unwrap();
                            let _ = tls.write_all(&response);
                        }
                        let _ = tls.flush();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("test listener failed: {error}"),
                }
            }
        });
        Self {
            endpoint,
            cert,
            stop,
            worker: Some(worker),
            backend,
        }
    }
    fn client(&self) -> Client {
        // Only the fixture's generated certificate is trusted in this unit test.
        // Production exposes no custom roots or certificate-verification bypass.
        Client::build(
            &self.endpoint,
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::new_with_certs(&[
                    ureq::tls::Certificate::from_der(&self.cert).to_owned(),
                ]))
                .build(),
        )
        .unwrap()
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn tls_proxy_roles_and_scoped_operator_key_work_end_to_end() {
    let operator = Secret::random().unwrap();
    let proxy = Proxy::start(operator.clone(), "127.0.0.1", false, None);
    let client = proxy.client();
    assert_eq!(client.create().unwrap_err(), Error::Invalid);
    assert!(client.create_with_key(&Secret::random().unwrap()).is_err());
    let room = client.create_with_key(&operator).unwrap();
    let invite = Invite {
        endpoint: proxy.endpoint.clone(),
        room: room.room.clone(),
        invite: room.invite.clone(),
    };
    let wire = encode(&invite).unwrap();
    let text = std::str::from_utf8(&wire).unwrap();
    assert!(!text.contains(operator.expose()));
    assert!(!text.contains(room.host.expose()));
    let guest = proxy.client().join(&invite).unwrap();
    for wrong in [&room.invite, &guest.member, &room.host] {
        assert!(client.create_with_key(wrong).is_err());
    }
    let timeline = Timeline {
        revision: 1,
        play_id: 1,
        queue: vec![VideoId::parse("abcdefghijk").unwrap()],
        index: 0,
        position_ms: 1500,
        paused: false,
        loading: false,
    };
    assert!(client.publish(&room.room, &operator, &timeline).is_err());
    assert!(client
        .publish(&room.room, &guest.member, &timeline)
        .is_err());
    client.publish(&room.room, &room.host, &timeline).unwrap();
    let view = client.poll(&room.room, &guest.member).unwrap();
    assert_eq!(view.timeline.unwrap(), timeline);
    let view = encode(&client.poll(&room.room, &guest.member).unwrap()).unwrap();
    for forbidden in [&operator, &room.host, &room.invite, &guest.member] {
        assert!(!std::str::from_utf8(&view)
            .unwrap()
            .contains(forbidden.expose()));
    }
    client.leave(&room.room, &guest.member).unwrap();
    assert!(client.poll(&room.room, &guest.member).is_err());
    client.close(&room.room, &room.host).unwrap();
    assert!(client.join(&invite).is_err());
}

#[test]
fn tls_rejects_untrusted_expired_wrong_name_and_plaintext_servers() {
    let operator = Secret::random().unwrap();
    let valid = Proxy::start(operator.clone(), "127.0.0.1", false, None);
    assert!(Client::new(&valid.endpoint)
        .unwrap()
        .create_with_key(&operator)
        .is_err());
    assert!(valid.client().create_with_key(&operator).is_ok());
    for (san, expired) in [("wrong.example.test", false), ("127.0.0.1", true)] {
        let invalid = Proxy::start(operator.clone(), san, expired, None);
        assert!(invalid.client().create_with_key(&operator).is_err());
    }
    let plain = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("https://{}", plain.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut socket, _) = plain.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buffer = [0; 1024];
        let _ = socket.read(&mut buffer);
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
    });
    assert!(Client::new(&endpoint)
        .unwrap()
        .create_with_key(&operator)
        .is_err());
    worker.join().unwrap();
}

#[test]
fn tls_redirect_does_not_contact_a_second_server_or_forward_capabilities() {
    let trap = TcpListener::bind("127.0.0.1:0").unwrap();
    trap.set_nonblocking(true).unwrap();
    let operator = Secret::random().unwrap();
    let proxy = Proxy::start(
        operator.clone(),
        "127.0.0.1",
        false,
        Some(format!("https://{}/stolen", trap.local_addr().unwrap())),
    );
    assert!(proxy.client().create_with_key(&operator).is_err());
    assert!(matches!(trap.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
}

#[test]
fn private_backend_authenticates_before_allocating_and_rejects_ambiguous_headers() {
    let operator = Secret::random().unwrap();
    let proxy = Proxy::start(operator.clone(), "127.0.0.1", false, None);
    let host = Endpoint::parse(&proxy.endpoint)
        .unwrap()
        .authority()
        .to_owned();
    for _ in 0..32 {
        assert!(proxy
            .backend
            .raw(&host, "/v1/rooms", "", None)
            .starts_with(b"HTTP/1.1 404"));
    }
    // Failed auth did not consume the 16/minute room-create budget.
    assert!(proxy.client().create_with_key(&operator).is_ok());
    let duplicate_auth = Zeroizing::new(format!("Authorization: Bearer {}\r\n", operator.expose()));
    for (target, headers) in [
        ("/v1/rooms?ignored=1", ""),
        ("/v1/%72ooms", ""),
        ("/v1//rooms", ""),
        ("https://another.example.test/v1/rooms", ""),
        ("/v1/rooms", "Host: another.example.test\r\n"),
        ("/v1/rooms", "x-tide-jam-protocol: 1\r\n"),
        ("/v1/rooms", "Origin: https://another.example.test\r\n"),
        ("/v1/rooms", "X-Forwarded-Host: jam.example.test\r\n"),
        ("/v1/rooms", "X-Forwarded-Proto: https\r\n"),
        (
            "/v1/rooms",
            "Forwarded: host=jam.example.test;proto=https\r\n",
        ),
        ("/v1/rooms", duplicate_auth.as_str()),
    ] {
        let response = proxy.backend.raw(&host, target, headers, Some(&operator));
        // Hyper itself rejects duplicate Host before the application guard.
        assert!(response.starts_with(b"HTTP/1.1 403") || response.starts_with(b"HTTP/1.1 400"));
    }
    assert!(proxy
        .backend
        .raw("another.example.test", "/v1/rooms", "", Some(&operator))
        .starts_with(b"HTTP/1.1 403"));
    assert!(!format!(
        "{:?}",
        PrivateRelay::new(&proxy.endpoint, operator.clone()).unwrap()
    )
    .contains(operator.expose()));
    assert!(PrivateRelay::new("http://127.0.0.1:1234", operator).is_err());
}

#[test]
fn neither_relay_mode_accepts_a_non_loopback_listener() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (_, stop) = tokio::sync::watch::channel(false);
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        assert!(relay::serve(listener, stop).await.is_err());
        let (_, stop) = tokio::sync::watch::channel(false);
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        assert!(relay::serve_private(
            listener,
            stop,
            PrivateRelay::new("https://jam.example.test", Secret::random().unwrap()).unwrap()
        )
        .await
        .is_err());
    });
}

#[test]
fn shared_https_needs_no_operator_secret_and_keeps_host_guest_roles() {
    let proxy = Proxy::start_shared();
    let client = proxy.client();
    let room = client.create_shared().unwrap();
    let invite = Invite {
        endpoint: proxy.endpoint.clone(),
        room: room.room.clone(),
        invite: room.invite.clone(),
    };
    let code = invite.to_code().unwrap();
    assert!(!code.contains(room.host.expose()));
    let decoded = Invite::from_code(&code).unwrap();
    let guest = client.join(&decoded).unwrap();
    let timeline = Timeline {
        revision: 1,
        play_id: 1,
        queue: vec![VideoId::parse("abcdefghijk").unwrap()],
        index: 0,
        position_ms: 10,
        paused: true,
        loading: false,
    };
    assert!(client
        .publish(&room.room, &guest.member, &timeline)
        .is_err());
    client.publish(&room.room, &room.host, &timeline).unwrap();
    let view = client.poll(&room.room, &guest.member).unwrap();
    assert_eq!(view.timeline.unwrap(), timeline);
    let serialized =
        serde_json::to_string(&client.poll(&room.room, &guest.member).unwrap()).unwrap();
    assert!(!serialized.contains(room.host.expose()));
    client.leave(&room.room, &guest.member).unwrap();
    assert!(client.poll(&room.room, &guest.member).is_err());
    client.close(&room.room, &room.host).unwrap();
    assert!(client.poll(&room.room, &room.host).is_err());
    client.create_shared().unwrap();
    assert!(
        client.create_shared().is_err(),
        "closing rooms must not bypass creation rate limits"
    );
}
#[test]
fn shared_backend_requires_one_canonical_proxy_peer_and_rejects_browser_origins() {
    let proxy = Proxy::start_shared();
    let host = proxy.endpoint.strip_prefix("https://").unwrap();
    for extra in [
        "",
        "x-tide-jam-peer: spoofed\r\n",
        "x-tide-jam-peer: 203.0.113.8\r\nx-tide-jam-peer: 203.0.113.9\r\n",
        "x-tide-jam-peer: 203.0.113.8\r\nOrigin: https://evil.example\r\n",
    ] {
        assert!(proxy
            .backend
            .raw(host, "/v1/rooms", extra, None)
            .starts_with(b"HTTP/1.1 403"));
    }
    assert!(proxy
        .backend
        .raw(host, "/v1/rooms", "x-tide-jam-peer: 203.0.113.8\r\n", None)
        .starts_with(b"HTTP/1.1 200"));
    let secret = Secret::random().unwrap();
    assert!(proxy
        .backend
        .raw(
            host,
            "/v1/rooms",
            "x-tide-jam-peer: 203.0.113.8\r\n",
            Some(&secret)
        )
        .starts_with(b"HTTP/1.1 404"));
    let private = Proxy::start(secret, "127.0.0.1", false, None);
    assert!(
        private.client().create_shared().is_err(),
        "shared client path must not bypass private operator authentication"
    );
}
