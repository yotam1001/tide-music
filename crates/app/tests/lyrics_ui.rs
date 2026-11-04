//! Native lyrics layout and input; no network, account or playback process.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Key, PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::{cell::Cell, rc::Rc};
use tide_music::*;

struct Platform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}
fn draw(window: &MinimalSoftwareWindow, width: u32, height: u32, name: &str) -> Vec<Rgb8Pixel> {
    window.set_size(PhysicalSize::new(width, height));
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut pixels, width as usize);
        });
    }
    if let Some(dir) = std::env::var_os("TIDE_UI_REVIEW_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::from_raw(
            width,
            height,
            pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect(),
        )
        .unwrap()
        .save(dir.join(format!("lyrics-{name}-{width}.png")))
        .unwrap();
    }
    pixels
}
fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    for event in [
        WindowEvent::PointerMoved { position },
        WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        },
        WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        },
    ] {
        ui.window().dispatch_event(event);
    }
}
#[test]
fn lyrics_follow_seek_and_close_in_both_themes_and_window_sizes() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    // Functional hit tests use settled geometry; motion is tested separately.
    ui.set_lyrics_motion_enabled(false);
    ui.set_track_title("A long song title for the lyrics panel".into());
    ui.set_track_artist("Artist name".into());
    ui.set_lyrics_lines(ModelRc::new(VecModel::from(
        (0..40)
            .map(|i| LyricRow {
                text: format!("Lyric line {i}: some words to follow").into(),
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_lyrics_status("Synced lyrics".into());
    ui.set_lyrics_source("Source: native test fixture".into());
    ui.set_lyrics_synced(true);
    ui.show().unwrap();
    let selected = Rc::new(Cell::new(-1));
    let captured = selected.clone();
    ui.on_seek_lyric(move |index| captured.set(index));
    for (width, height) in [(650, 420), (780, 800), (1140, 800), (1920, 1080)] {
        for light in [false, true] {
            ui.set_light_mode(light);
            ui.set_lyrics_open(true);
            ui.set_lyrics_active(0);
            draw(&window, width, height, "initial");
            ui.set_lyrics_active(25);
            let pixels = draw(&window, width, height, if light { "light" } else { "dark" });
            // The active line must have been scrolled into view, not left below
            // the viewport. Restrict the probe to the full-page lyrics surface.
            let background = if light {
                Rgb8Pixel::new(238, 226, 217)
            } else {
                Rgb8Pixel::new(48, 35, 46)
            };
            let x = ((width as usize).saturating_sub(1140) / 2) + 30;
            let y = (60..height as usize - 110)
                .find(|y| pixels[y * width as usize + x] == background)
                .expect("Current lyric did not scroll into view");
            selected.set(-1);
            click(&ui, x as f32 + 30.0, y as f32 + 30.0);
            assert_eq!(
                selected.get(),
                25,
                "Lyric seek index at {width}, light={light}"
            );
            ui.set_jam_guest(true);
            draw(&window, width, height, "guest");
            selected.set(-1);
            click(&ui, x as f32 + 30.0, y as f32 + 30.0);
            assert_eq!(
                selected.get(),
                -1,
                "Guest lyric clicks must not seek the host"
            );
            ui.set_jam_guest(false);
            ui.set_queue_open(true);
            draw(&window, width, height, "queue");
            assert!(!ui.get_lyrics_open(), "Queue and lyrics must not overlap");
            ui.set_lyrics_open(true);
            draw(&window, width, height, "reopen");
            assert!(!ui.get_queue_open());
        }
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Escape.into(),
    });
    assert!(!ui.get_lyrics_open());
}
