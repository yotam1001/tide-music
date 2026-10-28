//! Exercise real queue layout without a desktop window, account or playback.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Key, PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::Cell, rc::Rc};
use tide_music::*;

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}
fn draw(window: &MinimalSoftwareWindow, width: u32) -> Vec<Rgb8Pixel> {
    window.set_size(PhysicalSize::new(width, 720));
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * 720];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    pixels
}
fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}
fn key(ui: &AppWindow, value: Key) {
    let text: slint::SharedString = value.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}
fn remove_via_menu(ui: &AppWindow, window: &MinimalSoftwareWindow, width: u32, x: f32, y: f32) {
    click(ui, x, y);
    draw(window, width);
    // Up from an unselected popup selects its last item: Remove.
    key(ui, Key::UpArrow);
    key(ui, Key::Return);
    draw(window, width);
}
fn rows(title: &str, artist: &str) -> slint::ModelRc<TrackRow> {
    Rc::new(slint::VecModel::from(vec![
        TrackRow {
            title: title.into(),
            artist: artist.into(),
            duration: "12:34".into(),
            ..Default::default()
        };
        3
    ]))
    .into()
}
fn export(name: &str, width: u32, pixels: &[Rgb8Pixel]) {
    if let Some(path) = std::env::var_os("TIDE_QUEUE_TEXT_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        let bytes = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::RgbImage::from_raw(width, 720, bytes)
            .unwrap()
            .save(path.join(format!("{name}.png")))
            .unwrap();
    }
}

#[test]
fn long_queue_metadata_keeps_duration_and_remove_controls_in_place() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_queue_open(true);
    ui.set_current_index(0);
    let removed = Rc::new(Cell::new(-1));
    let capture = removed.clone();
    ui.on_remove_queue_track(move |index| capture.set(index));
    ui.show().unwrap();
    for width in [650, 780, 1140] {
        for light in [false, true] {
            ui.set_light_mode(light);
            ui.set_rows(rows("Song", "Artist"));
            draw(&window, width);
            // Locate the first song's actual menu and dispatch its Remove
            // action using real input, without exposing widget internals.
            let x = width as f32 - 38.0;
            let mut remove_y = None;
            for y in (120_usize..260).step_by(5) {
                removed.set(-1);
                remove_via_menu(&ui, &window, width, x, y as f32);
                if removed.get() == 0 {
                    remove_y = Some(y);
                    break;
                }
            }
            let y = remove_y.expect("short queue's remove menu action is inaccessible");
            let reference = draw(&window, width);
            export(&format!("short-{width}-{light}"), width, &reference);
            for (name, title, artist) in [
                (
                    "long",
                    "A very long song title with many words — Live deluxe remastered edition "
                        .repeat(12),
                    "Artist and guest performers featuring an orchestra ".repeat(12),
                ),
                ("unbroken", "W".repeat(600), "M".repeat(600)),
                (
                    "unicode",
                    "שם שיר ארוך במיוחד 🎵 日本語の曲名 ".repeat(30),
                    "אמן ושיתופי פעולה アーティスト ".repeat(30),
                ),
            ] {
                ui.set_rows(rows(&title, &artist));
                let pixels = draw(&window, width);
                export(&format!("{name}-{width}-{light}"), width, &pixels);
                // Time and actions are to the right of the flexible text column.
                // They must render identically regardless of metadata length.
                for row in y.saturating_sub(8)..y + 32 {
                    // One menu now holds Remove, reclaiming the former second
                    // action's width for metadata. Compare the time/menu strip.
                    let range = row * width as usize + width as usize - 112
                        ..row * width as usize + width as usize - 22;
                    assert!(pixels[range.clone()] == reference[range],
                        "metadata displaced queue time/actions: {name}, {width}, light={light}, row={row}");
                    let artwork_and_handle = row * width as usize + width as usize - 326
                        ..row * width as usize + width as usize - 238;
                    assert!(
                        pixels[artwork_and_handle.clone()] == reference[artwork_and_handle],
                        "metadata escaped left over artwork/drag handle: {name}, {width}, light={light}, row={row}"
                    );
                }
                removed.set(-1);
                remove_via_menu(&ui, &window, width, x, y as f32);
                assert_eq!(removed.get(), 0, "long metadata hid remove control");
                assert_eq!(window.size(), PhysicalSize::new(width, 720));
            }
        }
    }
}
