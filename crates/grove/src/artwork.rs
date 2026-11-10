//! Bounded memory-only album art. Never fetch private URLs or forward OAuth tokens.
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    collections::{HashSet, VecDeque},
    io::{Cursor, Read},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub struct Cache {
    entries: VecDeque<(String, Image)>,
    attempted: HashSet<String>,
    tx: Option<mpsc::SyncSender<(String, u64)>>,
    rx: mpsc::Receiver<(String, u64, Option<Vec<u8>>)>,
    generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    task: Option<std::thread::JoinHandle<()>>,
}
impl Cache {
    pub fn new(offline: bool) -> Self {
        let (tx, requests) = mpsc::sync_channel::<(String, u64)>(8);
        let (out, rx) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let task = (!offline).then(|| {
            std::thread::spawn(move || {
                let Ok(http) = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                else {
                    return;
                };
                while !stopping.load(Ordering::Relaxed) {
                    let (url, version) = match requests.recv_timeout(Duration::from_millis(200)) {
                        Ok(url) => url,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    if version != current.load(Ordering::Relaxed) {
                        continue;
                    }
                    let pixels = fetch(&http, &url);
                    let _ = out.try_send((url, version, pixels));
                }
            })
        });
        Self {
            entries: VecDeque::new(),
            attempted: HashSet::new(),
            tx: (!offline).then_some(tx),
            rx,
            stop,
            task,
            generation,
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((url, version, pixels)) = self.rx.try_recv() {
            if version != self.generation.load(Ordering::Relaxed) {
                continue;
            }
            // Failed downloads still free a request slot for later thumbnails.
            changed = true;
            if let Some(pixels) = pixels {
                let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &pixels, 96, 96,
                ));
                self.entries.push_back((url, image));
                while self.entries.len() > 48 {
                    self.entries.pop_front();
                }
                changed = true;
            }
        }
        changed
    }
    pub fn get(&mut self, url: &str) -> Image {
        if let Some((_, image)) = self.entries.iter().find(|(key, _)| key == url) {
            return image.clone();
        }
        if allowed(url) && !self.attempted.contains(url) && self.attempted.len() < 4096 {
            if let Some(tx) = &self.tx {
                if tx
                    .try_send((url.to_owned(), self.generation.load(Ordering::Relaxed)))
                    .is_ok()
                {
                    self.attempted.insert(url.into());
                }
            }
        }
        Image::default()
    }
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.entries.clear();
        self.attempted.clear();
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.tx = None;
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}
fn allowed(input: &str) -> bool {
    url::Url::parse(input).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && matches!(url.host_str(), Some("i.scdn.co" | "mosaic.scdn.co"))
            && url.query().is_none()
            && url.fragment().is_none()
            && input.len() < 2048
    })
}
fn fetch(http: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    if !allowed(url) {
        return None;
    }
    let response = http.get(url).send().ok()?;
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 1024 * 1024)
    {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    Some(
        reader
            .decode()
            .ok()?
            .resize_exact(96, 96, image::imageops::FilterType::Triangle)
            .to_rgba8()
            .into_raw(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_public_spotify_cdn_art_is_fetched() {
        assert!(allowed("https://i.scdn.co/image/album"));
        for url in [
            "http://i.scdn.co/image/a",
            "https://i.scdn.co.evil.test/a",
            "https://127.0.0.1/a",
            "https://user@i.scdn.co/a",
            "https://i.scdn.co/a?token=secret",
        ] {
            assert!(!allowed(url));
        }
    }
}
