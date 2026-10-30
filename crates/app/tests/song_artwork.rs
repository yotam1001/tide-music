use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{
    ComponentHandle, Image, ModelRc, PhysicalSize, Rgb8Pixel, SharedPixelBuffer, VecModel,
};
use std::rc::Rc;

use tide_music::*;

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn cover(color: Rgb8Pixel) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(96, 96);
    buffer.make_mut_slice().fill(color);
    Image::from_rgb8(buffer)
}

fn draw(window: &MinimalSoftwareWindow, pixels: &mut [Rgb8Pixel]) {
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(pixels, 780);
        });
    }
}

#[test]
fn changing_songs_clears_old_cover_until_the_selected_cover_arrives() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    window.set_size(PhysicalSize::new(780, 720));
    ui.show().unwrap();
    let red = Rgb8Pixel::new(255, 0, 0);
    let blue = Rgb8Pixel::new(0, 0, 255);
    let mut pixels = vec![Rgb8Pixel::default(); 780 * 720];
    // Keep stale row artwork present to reproduce the old fallback.
    ui.set_rows(ModelRc::new(VecModel::from(vec![TrackRow {
        artwork: cover(red),
        ..Default::default()
    }])));
    ui.set_current_index(0);
    ui.set_track_artwork(cover(red));
    draw(&window, &mut pixels);
    assert!(pixels.contains(&red));
    ui.set_track_title("Next song".into());
    ui.set_track_artwork(Image::default());
    draw(&window, &mut pixels);
    assert!(
        !pixels.contains(&red),
        "Old row artwork leaked into the new song"
    );
    ui.set_track_artwork(cover(blue));
    draw(&window, &mut pixels);
    assert!(pixels.contains(&blue));
    assert!(!pixels.contains(&red));
    ui.set_track_artwork(Image::default());
    draw(&window, &mut pixels);
    assert!(
        !pixels.contains(&blue),
        "Missing artwork retained the previous cover"
    );
    ui.set_account_connected(true);
    ui.set_library_open(true);
    ui.set_library_playlists(ModelRc::new(VecModel::from(vec![PlaylistRow {
        title: "Playlist without artwork".into(),
        ..Default::default()
    }])));
    draw(&window, &mut pixels);
    // Locate the opaque sleeve color in the rendered asset instead of coupling
    // this artwork check to the library heading's current pixel coordinates.
    let sleeve = Rgb8Pixel::new(208, 192, 189);
    let sleeve_pixels: Vec<usize> = pixels
        .iter()
        .enumerate()
        .filter_map(|(index, pixel)| (*pixel == sleeve).then_some(index))
        .collect();
    assert!(
        sleeve_pixels.len() > 20,
        "Missing playlist cover must render the bundled sleeve artwork"
    );
    let cover_probe = sleeve_pixels[sleeve_pixels.len() / 2];
    ui.set_library_playlists(ModelRc::new(VecModel::from(vec![PlaylistRow {
        title: "Playlist with source artwork".into(),
        artwork: cover(blue),
        ..Default::default()
    }])));
    draw(&window, &mut pixels);
    assert_eq!(
        pixels[cover_probe], blue,
        "Source artwork must replace the shared default cover"
    );
    let mut fallback_hashes = Vec::new();
    let mut source_hashes = Vec::new();
    for variant in 0..4 {
        for real in [false, true] {
            ui.set_library_playlists(ModelRc::new(VecModel::from(vec![PlaylistRow {
                title: "Shared cover variants".into(),
                cover_variant: variant,
                artwork: if real { cover(blue) } else { Image::default() },
                ..Default::default()
            }])));
            draw(&window, &mut pixels);
            let hash = pixels.iter().fold(0u64, |hash, p| {
                hash.wrapping_mul(31)
                    .wrapping_add(u64::from(p.r) + (u64::from(p.g) << 8) + (u64::from(p.b) << 16))
            });
            if real {
                source_hashes.push(hash);
            } else {
                fallback_hashes.push(hash);
            }
        }
    }
    fallback_hashes.sort_unstable();
    fallback_hashes.dedup();
    assert_eq!(
        fallback_hashes.len(),
        4,
        "Fallback illustrations must differ"
    );
    assert!(
        source_hashes.iter().all(|hash| *hash == source_hashes[0]),
        "Real artwork must override every fallback variant"
    );
}
