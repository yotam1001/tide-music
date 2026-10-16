//! Bounded loopback-only fault injector, never part of the shipped transport.
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tide_jam_lab::protocol::MAX_BODY;

pub struct Link {
    pub endpoint: String,
    pub outage: Arc<AtomicBool>,
    pub delay_ms: Arc<AtomicU64>,
    pub dropped: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Link {
    pub fn start(upstream: &str) -> Self {
        let address: SocketAddr = upstream.strip_prefix("http://").unwrap().parse().unwrap();
        assert_eq!(address.ip(), std::net::Ipv4Addr::LOCALHOST);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let outage = Arc::new(AtomicBool::new(false));
        let delay_ms = Arc::new(AtomicU64::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (fail, delay, rejected, cancel) = (
            outage.clone(),
            delay_ms.clone(),
            dropped.clone(),
            stop.clone(),
        );
        let worker = thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if fail.load(Ordering::Acquire) {
                            rejected.fetch_add(1, Ordering::Relaxed);
                        } else {
                            let delay =
                                Duration::from_millis(delay.load(Ordering::Acquire).min(500));
                            let _ = forward(stream, address, delay);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint,
            outage,
            delay_ms,
            dropped,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn forward(mut stream: TcpStream, address: SocketAddr, delay: Duration) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 1024];
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            if index > 8192 {
                return Ok(());
            }
            break index + 4;
        }
        if bytes.len() > 8192 {
            return Ok(());
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(std::io::Error::other)?;
    let mut length = 0usize;
    let mut outgoing = String::new();
    for line in header.split("\r\n").filter(|line| !line.is_empty()) {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().map_err(std::io::Error::other)?;
            }
            if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("connection") {
                continue;
            }
        }
        outgoing.push_str(line);
        outgoing.push_str("\r\n");
    }
    if length > MAX_BODY {
        return Ok(());
    }
    while bytes.len() < header_end + length {
        let mut chunk = [0u8; 1024];
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > header_end + MAX_BODY {
            return Ok(());
        }
    }
    outgoing.push_str(&format!("Host: {address}\r\nConnection: close\r\n\r\n"));
    let mut upstream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    upstream.set_read_timeout(Some(Duration::from_secs(2)))?;
    upstream.set_write_timeout(Some(Duration::from_secs(1)))?;
    thread::sleep(delay);
    upstream.write_all(outgoing.as_bytes())?;
    upstream.write_all(&bytes[header_end..header_end + length])?;
    let mut response = Vec::new();
    upstream
        .take((MAX_BODY + 8193) as u64)
        .read_to_end(&mut response)?;
    if response.len() > MAX_BODY + 8192 {
        return Ok(());
    }
    thread::sleep(delay);
    stream.write_all(&response)
}
