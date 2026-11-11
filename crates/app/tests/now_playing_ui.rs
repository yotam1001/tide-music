//! Native render check of Now Playing's ambient backdrop, with synthetic
//! artwork and no account: it drifts only while a song plays with
//! animations on.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer};
use std::rc::Rc;
use std::time::Duration;
use tide_music::{lyrics_effects::Backdrop, AppWindow, ControlMotion};

struct Platform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn render(window: &MinimalSoftwareWindow) -> Vec<Rgb8Pixel> {
    let mut pixels = vec![Rgb8Pixel::default(); 1140 * 800];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, 1140);
        });
    }
    if let Some(dir) = std::env::var_os("TIDE_UI_REVIEW_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::from_raw(
            1140,
            800,
            pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect(),
        )
        .unwrap()
        .save(dir.join("now-playing-ambient.png"))
        .unwrap();
    }
    pixels
}

/// A strip left of the cover, below the close button: backdrop only.
fn backdrop_strip(pixels: &[Rgb8Pixel]) -> Vec<Rgb8Pixel> {
    (200..680)
        .flat_map(|y| (0..48).map(move |x| pixels[y * 1140 + x]))
        .collect()
}

fn settle(window: &MinimalSoftwareWindow) -> Vec<Rgb8Pixel> {
    std::thread::sleep(Duration::from_millis(1100));
    backdrop_strip(&render(window))
}

#[test]
fn ambient_backdrop_drifts_only_while_playing_with_motion_on() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(1140, 800));
    let mut cover = SharedPixelBuffer::<Rgba8Pixel>::new(96, 96);
    for (i, pixel) in cover.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (i % 96, i / 96);
        *pixel = Rgba8Pixel::new((x * 2) as u8, (y * 2) as u8, 200, 255);
    }
    let cover = slint::Image::from_rgba8(cover);
    ui.set_track_artwork(cover.clone());
    ui.set_lyrics_backdrop(Backdrop::default().update(cover));
    ui.set_track_title("Ambient fixture".into());
    ui.set_track_artist("Synthetic artwork, no account or network".into());
    ui.global::<ControlMotion>().set_enabled(true);
    ui.set_paused(false);
    ui.set_now_playing_open(true);
    render(&window);
    assert!(ui.get_now_playing_presented());

    let frame = render(&window);
    // No one-pixel seam lines across the backdrop (the software renderer
    // draws one at an intermediate gradient stop).
    let column: Vec<i32> = (70..700)
        .map(|y| {
            let p = frame[y * 1140 + 1100];
            i32::from(p.r) + i32::from(p.g) + i32::from(p.b)
        })
        .collect();
    for (offset, rows) in column.windows(3).enumerate() {
        let dip = (rows[1] - rows[0]).abs().min((rows[1] - rows[2]).abs());
        assert!(dip <= 8, "seam line at y={}", 71 + offset);
    }
    let start = backdrop_strip(&frame);
    let later = settle(&window);
    assert_ne!(start, later, "the backdrop did not drift while playing");

    ui.set_paused(true);
    let paused = settle(&window);
    assert_eq!(paused, settle(&window), "the backdrop drifted while paused");

    ui.set_paused(false);
    ui.global::<ControlMotion>().set_enabled(false);
    let still = settle(&window);
    assert_eq!(
        still,
        settle(&window),
        "the backdrop drifted with animations off"
    );
}
