mod common;
use common::Relay;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};
use tide_jam_lab::{client::Client, protocol::*};

#[test]
fn real_http_role_separation_shared_queue_and_close() {
    let relay = Relay::start();
    let client = Client::new(&relay.endpoint).unwrap();
    let room = client.create().unwrap();
    let invite = Invite {
        endpoint: relay.endpoint.clone(),
        room: room.room.clone(),
        invite: room.invite.clone(),
    };
    let guest = client.join(&invite).unwrap();
    let timeline = Timeline {
        revision: 1,
        play_id: 1,
        queue: vec![
            VideoId::parse("abcdefghijk").unwrap(),
            VideoId::parse("lmnopqrstuv").unwrap(),
        ],
        index: 0,
        position_ms: 2000,
        paused: false,
        loading: false,
    };
    assert!(client
        .publish(&room.room, &guest.member, &timeline)
        .is_err());
    client.publish(&room.room, &room.host, &timeline).unwrap();
    let view = client.poll(&room.room, &guest.member).unwrap();
    assert_eq!(view.timeline.unwrap(), timeline);
    let skip = Skip {
        play_id: timeline.play_id,
        video: timeline.queue[0].clone(),
    };
    client
        .request_skip(&room.room, &guest.member, &skip)
        .unwrap();
    assert_eq!(
        client.skip_request(&room.room, &room.host).unwrap(),
        Some(skip)
    );
    assert!(client.skip_request(&room.room, &guest.member).is_err());
    assert_eq!(
        client.poll(&room.room, &guest.member).unwrap().timeline,
        Some(timeline)
    );
    client.close(&room.room, &room.host).unwrap();
    assert!(client.poll(&room.room, &guest.member).is_err());
}
fn raw(relay: &Relay, headers: &str, body: &[u8]) -> Vec<u8> {
    let address = relay.endpoint.strip_prefix("http://").unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "POST /v1/rooms HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
    let mut response = Vec::new();
    let _ = stream.take(4096).read_to_end(&mut response);
    response
}
#[test]
fn browser_origin_missing_protocol_oversized_and_unknown_fields_are_rejected() {
    let relay = Relay::start();
    let required = "x-tide-jam-protocol: 1\r\nContent-Type: application/json\r\n";
    assert!(raw(&relay, "", b"{}").starts_with(b"HTTP/1.1 403"));
    assert!(raw(
        &relay,
        &format!("{required}Origin: https://example.invalid\r\n"),
        b"{}"
    )
    .starts_with(b"HTTP/1.1 403"));
    assert!(raw(&relay, required, &vec![b' '; MAX_BODY + 1]).starts_with(b"HTTP/1.1 413"));
    assert!(raw(&relay, required, b"{\"source\":\"file:///never\"}").starts_with(b"HTTP/1.1 422"));
}
#[test]
fn spoofed_host_and_slow_request_headers_are_rejected() {
    let relay = Relay::start();
    let address = relay.endpoint.strip_prefix("http://").unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream.write_all(b"POST /v1/rooms HTTP/1.1\r\nHost: example.invalid\r\nx-tide-jam-protocol: 1\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 403"));
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream
        .write_all(b"POST /v1/rooms HTTP/1.1\r\nHost:")
        .unwrap();
    let started = Instant::now();
    let _ = stream.read_to_end(&mut Vec::new());
    assert!(started.elapsed() < Duration::from_millis(3500));
}
#[test]
fn client_neither_follows_redirects_nor_accepts_oversized_or_malicious_replies() {
    for response in [
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 0\r\n\r\n"
            .to_owned(),
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            MAX_BODY + 1,
            " ".repeat(MAX_BODY + 1)
        ),
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_owned(),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        });
        assert!(Client::new(&endpoint).unwrap().create().is_err());
        worker.join().unwrap();
    }
}

#[test]
fn client_validates_untrusted_poll_replies_not_just_its_own_outgoing_state() {
    for body in [
        r#"{"timeline":{"revision":1,"play_id":1,"queue":["file:///bad"],"index":0,"position_ms":0,"paused":false,"loading":false},"age_ms":0,"members":1}"#,
        r#"{"timeline":null,"age_ms":0,"members":999}"#,
        r#"{"timeline":{"revision":1,"play_id":1,"queue":["abcdefghijk"],"index":0,"position_ms":999999999,"paused":false,"loading":false},"age_ms":0,"members":1}"#,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        assert!(Client::new(&endpoint)
            .unwrap()
            .poll(&"0".repeat(32), &Secret::random().unwrap())
            .is_err());
        worker.join().unwrap();
    }
}

#[test]
fn slow_request_bodies_are_bounded_too() {
    let relay = Relay::start();
    let address = relay.endpoint.strip_prefix("http://").unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    write!(stream, "POST /v1/rooms HTTP/1.1\r\nHost: {address}\r\nx-tide-jam-protocol: 1\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{{").unwrap();
    let started = Instant::now();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    assert!(started.elapsed() < Duration::from_millis(3500));
    assert!(response.starts_with(b"HTTP/1.1 408"));
}
