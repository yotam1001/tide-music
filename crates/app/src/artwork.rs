//! Memory-only thumbnails. The UI thread owns Slint images; one worker handles
//! public CDN requests and decoding, with bounded queues and allocation limits.
use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};
use std::{
    collections::{HashSet, VecDeque},
    io::Cursor,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

const CACHE_CAPACITY: usize = 48;
const PENDING_CAPACITY: usize = 32;
const SIDE: u32 = 96;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_PIXELS: u64 = 1_000_000;
const MAX_URL_BYTES: usize = 4096;
const NEGATIVE_TTL: Duration = Duration::from_secs(60);

struct Request {
    url: String,
    generation: u64,
}

// One background fetch at a time. The current song jumps ahead of list covers.
#[derive(Default)]
struct RequestQueue {
    items: Mutex<VecDeque<Request>>,
    wake: Condvar,
}

impl RequestQueue {
    fn enqueue(&self, request: Request, priority: bool) -> bool {
        let mut items = self.items.lock().unwrap();
        if let Some(index) = items.iter().position(|item| item.url == request.url) {
            if priority {
                let existing = items.remove(index).unwrap();
                items.push_front(existing);
            }
            return true;
        }
        // Reserve one slot for the playing song even during rapid scrolling.
        if items.len() >= PENDING_CAPACITY + usize::from(priority) {
            return false;
        }
        if priority {
            items.push_front(request);
        } else {
            items.push_back(request);
        }
        self.wake.notify_one();
        true
    }

    fn next(&self, stopped: &AtomicBool) -> Option<Request> {
        let mut items = self.items.lock().unwrap();
        loop {
            if stopped.load(Ordering::Acquire) {
                return None;
            }
            if let Some(request) = items.pop_front() {
                return Some(request);
            }
            items = self.wake.wait(items).unwrap();
        }
    }
}

struct Completion {
    url: String,
    generation: u64,
    pixels: Option<Vec<u8>>,
}

struct Entry {
    url: String,
    image: Option<Image>,
    stored: Instant,
}

/// Call all methods from the UI thread. `poll()` reports completed requests;
/// refresh visible model rows when it returns true to display images and refill
/// available request slots. Clones handed to models share
/// their pixel storage; model rows should also be bounded by the caller.
pub struct ArtworkCache {
    entries: VecDeque<Entry>,
    pending: HashSet<String>,
    requests: Option<Arc<RequestQueue>>,
    results: Receiver<Completion>,
    generation: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
}

impl ArtworkCache {
    /// A cache with networking disabled, for smoke fixtures and offline mode.
    pub fn new_disabled() -> Self {
        let (_, results) = mpsc::sync_channel(PENDING_CAPACITY);
        Self {
            entries: VecDeque::with_capacity(CACHE_CAPACITY),
            pending: HashSet::with_capacity(PENDING_CAPACITY),
            requests: None,
            results,
            generation: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new() -> Self {
        let requests = Arc::new(RequestQueue::default());
        let request_rx = requests.clone();
        let (result_tx, results) = mpsc::sync_channel(PENDING_CAPACITY);
        let generation = Arc::new(AtomicU64::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_generation = generation.clone();
        let worker_stopped = stopped.clone();
        // Dropping the handle detaches the worker. Shutdown never joins an HTTP
        // request on the UI thread; closed channels and the flag end the worker.
        let spawned = std::thread::Builder::new()
            .name("artwork".into())
            .spawn(move || worker(request_rx, result_tx, worker_generation, worker_stopped))
            .is_ok();
        Self {
            entries: VecDeque::with_capacity(CACHE_CAPACITY),
            pending: HashSet::with_capacity(PENDING_CAPACITY),
            requests: spawned.then_some(requests),
            results,
            generation,
            stopped,
        }
    }

    /// Return a shared thumbnail or an empty image (the UI draws its placeholder).
    /// Missing entries enqueue work without blocking, and duplicate requests coalesce.
    pub fn get_or_request(&mut self, url: &str) -> Image {
        self.request(url, false)
    }

    pub fn get_or_request_current(&mut self, url: &str) -> Image {
        self.request(url, true)
    }

    fn request(&mut self, url: &str, priority: bool) -> Image {
        // Check the cache before reparsing already validated URLs on every refresh.
        if let Some(index) = self.entries.iter().position(|entry| entry.url == url) {
            let entry = self.entries.remove(index).unwrap();
            if entry.image.is_some() || entry.stored.elapsed() < NEGATIVE_TTL {
                let image = entry.image.clone().unwrap_or_default();
                self.entries.push_back(entry);
                return image;
            }
        }
        if priority && self.pending.contains(url) {
            if let Some(requests) = &self.requests {
                let mut items = requests.items.lock().unwrap();
                if let Some(index) = items.iter().position(|item| item.url == url) {
                    let request = items.remove(index).unwrap();
                    items.push_front(request);
                }
            }
            return Image::default();
        }
        if !allowed_url(url)
            || (!priority && (self.pending.len() >= PENDING_CAPACITY || self.pending.contains(url)))
            || (priority && self.pending.len() > PENDING_CAPACITY && !self.pending.contains(url))
        {
            return Image::default();
        }
        let request = Request {
            url: url.to_owned(),
            generation: self.generation.load(Ordering::Acquire),
        };
        if self
            .requests
            .as_ref()
            .is_some_and(|sender| sender.enqueue(request, priority))
        {
            self.pending.insert(url.to_owned());
        }
        Image::default()
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.results.try_recv() {
            if result.generation != self.generation.load(Ordering::Acquire) {
                continue;
            }
            self.pending.remove(&result.url);
            let image = result.pixels.map(|pixels| {
                Image::from_rgb8(SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                    &pixels, SIDE, SIDE,
                ))
            });
            changed = true;
            self.insert(result.url, image);
        }
        changed
    }

    /// Clear account-related artwork and invalidate queued/in-progress completions.
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.entries.clear();
        self.pending.clear();
        if let Some(requests) = &self.requests {
            requests.items.lock().unwrap().clear();
        }
        while self.results.try_recv().is_ok() {}
    }

    fn insert(&mut self, url: String, image: Option<Image>) {
        if let Some(index) = self.entries.iter().position(|entry| entry.url == url) {
            self.entries.remove(index);
        }
        if self.entries.len() == CACHE_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            url,
            image,
            stored: Instant::now(),
        });
    }
}

impl Drop for ArtworkCache {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(requests) = self.requests.take() {
            let _guard = requests.items.lock().unwrap();
            requests.wake.notify_one();
        }
    }
}

fn allowed_url(value: &str) -> bool {
    if value.len() > MAX_URL_BYTES
        || value.contains('\\')
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
    {
        return false;
    }
    url.host_str().is_some_and(|host| {
        ["ytimg.com", "ggpht.com", "googleusercontent.com"]
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    })
}

fn worker(
    requests: Arc<RequestQueue>,
    results: SyncSender<Completion>,
    generation: Arc<AtomicU64>,
    stopped: Arc<AtomicBool>,
) {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(5)))
        .max_idle_connections(1)
        .max_idle_connections_per_host(1)
        .input_buffer_size(16 * 1024)
        .output_buffer_size(4096)
        .max_response_header_size(16 * 1024)
        .build();
    let agent: ureq::Agent = config.into();
    while let Some(request) = requests.next(&stopped) {
        if stopped.load(Ordering::Acquire) {
            break;
        }
        if request.generation != generation.load(Ordering::Acquire) {
            continue;
        }
        let pixels = fetch(&agent, &request.url);
        if stopped.load(Ordering::Acquire) {
            break;
        }
        if request.generation != generation.load(Ordering::Acquire) {
            continue;
        }
        if results
            .send(Completion {
                url: request.url,
                generation: request.generation,
                pixels,
            })
            .is_err()
        {
            break;
        }
    }
}

fn fetch(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    // No cookies, account authorization, Referer, or custom private headers.
    if !allowed_url(url) {
        return None;
    }
    // Feed the HTTP library the same canonical URL that passed host validation.
    let canonical = url::Url::parse(url).ok()?;
    let mut response = agent.get(canonical.as_str()).call().ok()?;
    if !response.status().is_success()
        || response
            .body()
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return None;
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES as u64)
        .read_to_vec()
        .ok()?;
    decode_thumbnail(&bytes)
}

fn decode_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(16 * 1024 * 1024);
    let reader = || {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        reader.limits(limits.clone());
        reader
    };
    // Reject huge dimensions before decoding (including small compressed bombs).
    let (width, height) = reader().into_dimensions().ok()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        return None;
    }
    let image = reader().decode().ok()?;
    // Crop before resizing: resize_to_fill on a 4096×1 input would first
    // allocate a 393216×96 intermediate image despite the source pixel cap.
    let edge = width.min(height);
    // Borrow the crop instead of copying up to a million source pixels. Match
    // the source format so filtering retains its original channel precision.
    macro_rules! resized_rgb {
        ($pixels:expr, $variant:ident) => {{
            let view = image::imageops::crop_imm(
                &$pixels,
                (width - edge) / 2,
                (height - edge) / 2,
                edge,
                edge,
            );
            image::DynamicImage::$variant(image::imageops::resize(
                &*view,
                SIDE,
                SIDE,
                FilterType::Triangle,
            ))
            .into_rgb8()
            .into_raw()
        }};
    }
    use image::DynamicImage::*;
    Some(match image {
        ImageLuma8(pixels) => resized_rgb!(pixels, ImageLuma8),
        ImageLumaA8(pixels) => resized_rgb!(pixels, ImageLumaA8),
        ImageRgb8(pixels) => resized_rgb!(pixels, ImageRgb8),
        ImageRgba8(pixels) => resized_rgb!(pixels, ImageRgba8),
        ImageLuma16(pixels) => resized_rgb!(pixels, ImageLuma16),
        ImageLumaA16(pixels) => resized_rgb!(pixels, ImageLumaA16),
        ImageRgb16(pixels) => resized_rgb!(pixels, ImageRgb16),
        ImageRgba16(pixels) => resized_rgb!(pixels, ImageRgba16),
        ImageRgb32F(pixels) => resized_rgb!(pixels, ImageRgb32F),
        ImageRgba32F(pixels) => resized_rgb!(pixels, ImageRgba32F),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_https_thumbnail_hosts_are_allowed() {
        for value in [
            "https://i.ytimg.com/vi/test/hqdefault.jpg",
            "https://lh3.googleusercontent.com/art=w120-h120",
            "https://yt3.ggpht.com/art",
            "https://ytimg.com/art",
        ] {
            assert!(allowed_url(value), "{value}");
        }
        for value in [
            "http://i.ytimg.com/art",
            "https://i.ytimg.com.evil.example/art",
            "https://evilytimg.com/art",
            "https://127.0.0.1/art",
            "https://[::1]/art",
            "https://user:password@i.ytimg.com/art",
            "https://i.ytimg.com:8443/art",
            "https://i.ytimg.com/art#fragment",
            "https://i.ytimg.com\\@evil.example/art",
            " https://i.ytimg.com/art",
            "https://i.ytimg.com/\nart",
            "file:///image.png",
            "",
        ] {
            assert!(!allowed_url(value), "{value}");
        }
        assert!(!allowed_url(&format!(
            "https://i.ytimg.com/{}",
            "a".repeat(4096)
        )));
    }

    #[test]
    fn compressed_large_images_are_rejected_before_decode() {
        let source = image::RgbImage::new(1025, 1025);
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, ImageFormat::Png).unwrap();
        assert!(bytes.get_ref().len() < MAX_RESPONSE_BYTES);
        assert!(decode_thumbnail(bytes.get_ref()).is_none());
        assert!(decode_thumbnail(&vec![0; MAX_RESPONSE_BYTES + 1]).is_none());
    }

    #[test]
    fn decoded_thumbnails_have_fixed_memory_cost() {
        for (width, height) in [(120, 200), (4096, 1)] {
            let source = image::RgbImage::new(width, height);
            let mut bytes = Cursor::new(Vec::new());
            source.write_to(&mut bytes, ImageFormat::Png).unwrap();
            let pixels = decode_thumbnail(bytes.get_ref()).unwrap();
            assert_eq!(pixels.len(), (SIDE * SIDE * 3) as usize);
        }
        assert!(decode_thumbnail(b"not an image").is_none());
    }

    #[test]
    fn borrowed_crop_preserves_thumbnail_pixels_and_channel_precision() {
        use image::{DynamicImage, ImageBuffer, Luma, LumaA, Rgb, Rgba};

        let check = |source: DynamicImage, format| {
            let mut bytes = Cursor::new(Vec::new());
            source.write_to(&mut bytes, format).unwrap();
            let decoded = image::load_from_memory(bytes.get_ref()).unwrap();
            let edge = decoded.width().min(decoded.height());
            let expected = decoded
                .crop_imm(
                    (decoded.width() - edge) / 2,
                    (decoded.height() - edge) / 2,
                    edge,
                    edge,
                )
                .resize_exact(SIDE, SIDE, FilterType::Triangle)
                .to_rgb8()
                .into_raw();
            assert_eq!(decode_thumbnail(bytes.get_ref()).unwrap(), expected);
        };
        for (width, height) in [(137, 211), (211, 137), (96, 96), (4096, 1)] {
            let value = |x, y| ((x * 123 + y * 456) % 65536) as u16;
            for source in [
                DynamicImage::ImageLuma8(ImageBuffer::from_fn(width, height, |x, y| {
                    Luma([value(x, y) as u8])
                })),
                DynamicImage::ImageLumaA8(ImageBuffer::from_fn(width, height, |x, y| {
                    LumaA([value(x, y) as u8, 173])
                })),
                DynamicImage::ImageRgb8(ImageBuffer::from_fn(width, height, |x, y| {
                    Rgb([value(x, y) as u8, 71, 213])
                })),
                DynamicImage::ImageRgba8(ImageBuffer::from_fn(width, height, |x, y| {
                    Rgba([value(x, y) as u8, 71, 213, 173])
                })),
                DynamicImage::ImageLuma16(ImageBuffer::from_fn(width, height, |x, y| {
                    Luma([value(x, y)])
                })),
                DynamicImage::ImageLumaA16(ImageBuffer::from_fn(width, height, |x, y| {
                    LumaA([value(x, y), 41357])
                })),
                DynamicImage::ImageRgb16(ImageBuffer::from_fn(width, height, |x, y| {
                    Rgb([value(x, y), 13271, 51357])
                })),
                DynamicImage::ImageRgba16(ImageBuffer::from_fn(width, height, |x, y| {
                    Rgba([value(x, y), 13271, 51357, 41357])
                })),
            ] {
                check(source, ImageFormat::Png);
            }
        }
        for format in [ImageFormat::Jpeg, ImageFormat::WebP] {
            check(
                DynamicImage::ImageRgb8(ImageBuffer::from_fn(137, 211, |x, y| {
                    Rgb([x as u8, y as u8, 71])
                })),
                format,
            );
        }
    }

    // Use disconnected channels so cache tests cannot perform network requests.
    fn offline_cache() -> ArtworkCache {
        ArtworkCache::new_disabled()
    }

    #[test]
    fn failure_cache_is_bounded_and_recent_entries_survive_eviction() {
        let mut cache = offline_cache();
        for index in 0..CACHE_CAPACITY {
            cache.insert(format!("https://i.ytimg.com/{index}"), None);
        }
        cache.get_or_request("https://i.ytimg.com/0");
        cache.insert("https://i.ytimg.com/new".into(), None);
        assert_eq!(cache.entries.len(), CACHE_CAPACITY);
        assert!(cache.entries.iter().any(|entry| entry.url.ends_with("/0")));
        assert!(!cache.entries.iter().any(|entry| entry.url.ends_with("/1")));
        cache.clear();
        assert!(cache.entries.is_empty());
        assert!(cache.pending.is_empty());
        assert_eq!(cache.generation.load(Ordering::Acquire), 1);
    }

    #[test]
    fn requests_coalesce_and_the_pending_queue_stays_bounded() {
        let mut cache = offline_cache();
        let requests = Arc::new(RequestQueue::default());
        cache.requests = Some(requests.clone());
        for index in 0..PENDING_CAPACITY + 5 {
            let url = format!("https://i.ytimg.com/{index}");
            cache.get_or_request(&url);
            cache.get_or_request(&url);
        }
        assert_eq!(cache.pending.len(), PENDING_CAPACITY);
        assert_eq!(requests.items.lock().unwrap().len(), PENDING_CAPACITY);
        cache.insert("https://i.ytimg.com/failure".into(), None);
        cache.pending.clear();
        cache.get_or_request("https://i.ytimg.com/failure");
        assert_eq!(requests.items.lock().unwrap().len(), PENDING_CAPACITY);
    }

    #[test]
    fn current_cover_is_promoted_ahead_of_scrolling_without_unbounding_work() {
        let mut cache = offline_cache();
        let requests = Arc::new(RequestQueue::default());
        cache.requests = Some(requests.clone());
        for index in 0..PENDING_CAPACITY {
            cache.get_or_request(&format!("https://i.ytimg.com/{index}"));
        }
        cache.get_or_request_current("https://i.ytimg.com/20");
        assert_eq!(
            requests.items.lock().unwrap().front().unwrap().url,
            "https://i.ytimg.com/20"
        );
        assert_eq!(requests.items.lock().unwrap().len(), PENDING_CAPACITY);
        cache.get_or_request_current("https://i.ytimg.com/playing");
        assert_eq!(
            requests.items.lock().unwrap().front().unwrap().url,
            "https://i.ytimg.com/playing"
        );
        assert_eq!(cache.pending.len(), PENDING_CAPACITY + 1);
        cache.get_or_request_current("https://i.ytimg.com/another");
        assert_eq!(cache.pending.len(), PENDING_CAPACITY + 1);
        cache.clear();
        assert!(requests.items.lock().unwrap().is_empty());
    }

    #[test]
    fn stale_results_cannot_repopulate_a_cleared_cache() {
        let mut cache = offline_cache();
        let (sender, receiver) = mpsc::sync_channel(PENDING_CAPACITY);
        cache.results = receiver;
        cache.clear();
        sender
            .send(Completion {
                url: "https://i.ytimg.com/old".into(),
                generation: 0,
                pixels: Some(vec![0; (SIDE * SIDE * 3) as usize]),
            })
            .unwrap();
        assert!(!cache.poll());
        assert!(cache.entries.is_empty());
    }
}
