use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};
use tide_jam_lab::client::Client;

#[test]
fn wakeup_is_scoped_to_canonical_render_endpoints() {
    assert!(Client::new("https://tide-test.onrender.com")
        .unwrap()
        .needs_wakeup());
    for endpoint in [
        "https://onrender.com",
        "https://tide.onrender.com.untrusted.example",
        "https://relay.example",
        "http://127.0.0.1:12345",
    ] {
        assert!(!Client::new(endpoint).unwrap().needs_wakeup());
    }
}

#[test]
fn readiness_requires_exact_marker_and_sends_no_capability() {
    for (body, expected) in [
        ("tide-jam-ready-v1", true),
        ("<html>Render is starting</html>", false),
        ("tide-jam-ready-v1-extra", false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut input = Vec::new();
            let mut chunk = [0; 1024];
            while !input.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0 && input.len() < 8192);
                input.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8(input).unwrap().to_lowercase();
            assert!(request.starts_with("get /healthz http/1.1\r\n"));
            assert!(!request.contains("authorization:") && !request.contains("cookie:"));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        assert_eq!(
            Client::new(&format!("http://{address}"))
                .unwrap()
                .ready()
                .is_ok(),
            expected
        );
        worker.join().unwrap();
    }
}
