//! Explicit local design review: real account Home data and production thumbnails.
//! No playback, account writes, settings writes, or credential/metadata dump.
//! The preview queue is seeded from Home, not the running player's queue.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    WindowAdapter,
};
use slint::{ComponentHandle, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::{
    path::Path,
    rc::Rc,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
use tide_music::*;
#[allow(dead_code)]
#[path = "../src/artwork.rs"]
mod artwork;
#[path = "../src/browse_layout.rs"]
mod browse_layout;
#[allow(dead_code)]
#[path = "../src/discovery.rs"]
mod discovery;

struct Platform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Platform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn draw(window: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<Rgb8Pixel> {
    window.set_size(PhysicalSize::new(width, height));
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    pixels
}
fn save(
    window: &MinimalSoftwareWindow,
    directory: &Path,
    name: &str,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let pixels = draw(window, width, height);
    let bytes = pixels
        .iter()
        .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
        .collect();
    image::RgbImage::from_raw(width, height, bytes)
        .ok_or("Preview buffer mismatch")?
        .save(directory.join(format!("{name}.png")))
        .map_err(|_| "Could not save preview".to_owned())
}
fn main() {
    if run().is_err() {
        // Provider/network errors can carry private diagnostics. Do not echo them.
        eprintln!("Live UI preview could not finish. It requires a verified local Tide connection and an output directory.");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let directory = std::env::var_os("TIDE_UI_REVIEW_DIR").ok_or("Missing output directory")?;
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).map_err(|_| "Could not prepare output")?;
    let provider = tide_provider::Provider::new()?;
    let response = provider.execute(
        tide_provider::Request::Home,
        Arc::new(AtomicBool::new(false)),
    )?;
    let tide_provider::Response::Home { cards } = response else {
        return Err("Home unavailable".into());
    };
    let cards = discovery::Discovery::default().select(cards);
    if cards.is_empty() {
        return Err("No recommendations".into());
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone())))
        .map_err(|_| "Renderer unavailable")?;
    let ui = AppWindow::new().map_err(|_| "UI unavailable")?;
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_jam_available(true);
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_paused(true);
    ui.set_volume(40.0);
    ui.set_autoplay_enabled(true);
    ui.show().map_err(|_| "Window unavailable")?;
    let mut cache = artwork::ArtworkCache::new();
    for card in &cards {
        if let Some(url) = &card.artwork_url {
            cache.get_or_request(url);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        cache.poll();
        let with_art = cards
            .iter()
            .filter_map(|card| card.artwork_url.as_deref())
            .filter(|url| cache.get_or_request(url).size().width > 0)
            .count();
        if with_art
            >= cards
                .iter()
                .filter(|card| card.artwork_url.is_some())
                .count()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cache.poll();
    let recommendations: Vec<_> = cards
        .iter()
        .map(|card| RecommendationRow {
            id: card.id.clone().into(),
            title: card.title.clone().into(),
            subtitle: card.subtitle.clone().into(),
            kind: card.kind.clone().into(),
            shelf: card.shelf.clone().into(),
            cover_variant: artwork::fallback_variant(&card.id),
            artwork: card
                .artwork_url
                .as_deref()
                .map(|url| cache.get_or_request(url))
                .unwrap_or_default(),
        })
        .collect();
    if !recommendations
        .iter()
        .any(|card| card.artwork.size().width > 0)
    {
        return Err("No artwork fetched".into());
    }
    ui.set_recommendations(ModelRc::new(VecModel::from(recommendations)));
    let songs: Vec<_> = cards
        .iter()
        .filter(|card| card.kind == "song")
        .take(6)
        .collect();
    ui.set_rows(ModelRc::new(VecModel::from(
        songs
            .iter()
            .map(|card| TrackRow {
                title: card.title.clone().into(),
                artist: card.subtitle.clone().into(),
                album: "".into(),
                artist_enabled: true,
                duration: card
                    .duration_seconds
                    .map(|n| format!("{}:{:02}", n / 60, n % 60))
                    .unwrap_or_default()
                    .into(),
                recommended: false,
                artwork: card
                    .artwork_url
                    .as_deref()
                    .map(|url| cache.get_or_request(url))
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>(),
    )));
    if let Some(card) = songs.first() {
        ui.set_current_index(0);
        ui.set_track_title(card.title.clone().into());
        ui.set_track_artist(card.subtitle.clone().into());
        ui.set_duration(card.duration_seconds.unwrap_or(0) as f32);
        ui.set_track_artwork(
            card.artwork_url
                .as_deref()
                .map(|url| cache.get_or_request_current(url))
                .unwrap_or_default(),
        );
    }
    for light in [false, true] {
        ui.set_light_mode(light);
        ui.set_queue_open(true);
        draw(&window, 1440, 900);
        let labels: Vec<_> = cards
            .iter()
            .map(|card| {
                if card.shelf.trim().is_empty() {
                    "Recommended for you".to_owned()
                } else {
                    card.shelf.clone()
                }
            })
            .collect();
        let collections: Vec<_> = cards.iter().map(|card| card.kind != "song").collect();
        ui.set_home_browse_rows(ModelRc::new(VecModel::from(
            browse_layout::home_groups(&labels, &collections, ui.get_home_columns() as usize)
                .into_iter()
                .map(|group| HomeBrowseRow {
                    start: group.start as i32,
                    count: group.count as i32,
                    title: group.title.into(),
                    layout: match group.layout {
                        browse_layout::Layout::Spotlight => "spotlight",
                        browse_layout::Layout::Editorial => "editorial",
                        browse_layout::Layout::Tiles => "tiles",
                        browse_layout::Layout::Compact => "compact",
                    }
                    .into(),
                })
                .collect::<Vec<_>>(),
        )));
        save(
            &window,
            &directory,
            if light {
                "live-home-light"
            } else {
                "live-home-dark"
            },
            1440,
            900,
        )?;
    }
    ui.set_light_mode(false);
    ui.set_collapsed(true);
    save(&window, &directory, "live-mini", 700, 132)?;
    println!("Saved native Home (dark/light) and mini previews with fetched artwork; no playback or settings changes.");
    if std::env::args().any(|arg| arg == "--library") {
        ui.set_collapsed(false);
        ui.set_queue_open(false);
        ui.set_library_open(true);
        ui.set_home_open(false);
        for (index, name, request) in [
            (1, "albums", tide_provider::Request::LibraryAlbums),
            (2, "artists", tide_provider::Request::LibraryArtists),
            (3, "history", tide_provider::Request::History),
        ] {
            let response = provider.execute(request, Arc::new(AtomicBool::new(false)))?;
            let tide_provider::Response::Browse { cards, .. } = response else {
                return Err("Library section unavailable".into());
            };
            // Mirror the player's viewport loading rather than request an entire library's art.
            let visible: Vec<_> = cards.iter().take(12).collect();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                cache.poll();
                let ready = visible
                    .iter()
                    .filter_map(|card| card.artwork_url.as_deref())
                    .map(|url| cache.get_or_request(url).size().width > 0)
                    .fold(true, |ready, loaded| ready & loaded);
                if ready || Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            ui.set_library_section(index);
            ui.set_library_cards(ModelRc::new(VecModel::from(
                visible
                    .iter()
                    .map(|card| RecommendationRow {
                        id: card.id.clone().into(),
                        title: card.title.clone().into(),
                        subtitle: card.subtitle.clone().into(),
                        kind: card.kind.clone().into(),
                        shelf: card.shelf.clone().into(),
                        cover_variant: artwork::fallback_variant(&card.id),
                        artwork: card
                            .artwork_url
                            .as_deref()
                            .map(|url| cache.get_or_request(url))
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>(),
            )));
            for light in [false, true] {
                ui.set_light_mode(light);
                save(
                    &window,
                    &directory,
                    &format!(
                        "live-library-{name}-{}",
                        if light { "light" } else { "dark" }
                    ),
                    1140,
                    800,
                )?;
            }
            println!("Verified read-only {name} response and rendered native previews.");
        }
    }
    if let Some(query) = std::env::args().skip_while(|arg| arg != "--search").nth(1) {
        ui.set_collapsed(false);
        ui.set_library_open(false);
        ui.set_search_open(true);
        ui.set_queue_open(false);
        for (index, name, kind) in [
            (0, "songs", tide_provider::SearchKind::Songs),
            (1, "albums", tide_provider::SearchKind::Albums),
            (2, "artists", tide_provider::SearchKind::Artists),
            (3, "playlists", tide_provider::SearchKind::Playlists),
        ] {
            let response = provider.execute(
                tide_provider::Request::Search {
                    query: query.clone(),
                    kind,
                },
                Arc::new(AtomicBool::new(false)),
            )?;
            let tide_provider::Response::Search {
                kind: actual,
                cards,
            } = response
            else {
                return Err("Search unavailable".into());
            };
            if actual != kind || cards.is_empty() || cards.len() > 40 {
                return Err("Search response mismatch".into());
            }
            let visible: Vec<_> = cards.iter().take(12).collect();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                cache.poll();
                let ready = visible
                    .iter()
                    .filter_map(|card| card.artwork_url.as_deref())
                    .map(|url| cache.get_or_request(url).size().width > 0)
                    .fold(true, |ready, loaded| ready & loaded);
                if ready || Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            ui.set_search_kind(index);
            ui.set_search_rows(ModelRc::new(VecModel::from(
                visible
                    .iter()
                    .map(|card| TrackRow {
                        title: card.title.clone().into(),
                        artist: card.subtitle.clone().into(),
                        artist_enabled: !card.artists.is_empty(),
                        duration: card
                            .duration_seconds
                            .map(|n| format!("{}:{:02}", n / 60, n % 60))
                            .unwrap_or_default()
                            .into(),
                        artwork: card
                            .artwork_url
                            .as_deref()
                            .map(|url| cache.get_or_request(url))
                            .unwrap_or_default(),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>(),
            )));
            ui.set_search_cards(ModelRc::new(VecModel::from(
                visible
                    .iter()
                    .map(|card| RecommendationRow {
                        id: card.id.clone().into(),
                        title: card.title.clone().into(),
                        subtitle: card.subtitle.clone().into(),
                        kind: card.kind.clone().into(),
                        shelf: "".into(),
                        cover_variant: artwork::fallback_variant(&card.id),
                        artwork: card
                            .artwork_url
                            .as_deref()
                            .map(|url| cache.get_or_request(url))
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>(),
            )));
            for light in [false, true] {
                ui.set_light_mode(light);
                save(
                    &window,
                    &directory,
                    &format!(
                        "live-search-{name}-{}",
                        if light { "light" } else { "dark" }
                    ),
                    1140,
                    800,
                )?;
            }
            println!("Verified {name} search and rendered native previews with fetched artwork.");
        }
    }
    Ok(())
}
