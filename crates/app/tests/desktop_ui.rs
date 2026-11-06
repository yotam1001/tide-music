//! Native layout and input checks without a desktop, account, or audio engine.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Key, PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
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
fn draw(window: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<Rgb8Pixel> {
    window.set_size(PhysicalSize::new(width, height));
    let mut pixels = vec![Rgb8Pixel::default(); (width * height) as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    pixels
}
fn export(name: &str, width: u32, height: u32, pixels: &[Rgb8Pixel]) {
    if let Some(folder) = std::env::var_os("TIDE_UI_REVIEW_DIR") {
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let bytes = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::RgbImage::from_raw(width, height, bytes)
            .unwrap()
            .save(folder.join(format!("{name}.png")))
            .unwrap();
    }
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
#[test]
fn sidebar_home_transport_and_keyboard_shortcuts_keep_their_actions() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_account_name("Your account".into());
    ui.set_jam_available(true);
    ui.set_paused(true);
    ui.set_current_index(0);
    ui.set_track_title("A Quiet Sun".into());
    ui.set_track_artist("True North".into());
    ui.set_rows(ModelRc::new(VecModel::from(vec![
        TrackRow {
            title: "A Quiet Sun".into(),
            artist: "True North".into(),
            ..Default::default()
        },
        TrackRow {
            title: "Hollow Coast".into(),
            artist: "Echo Frame".into(),
            ..Default::default()
        },
    ])));
    ui.set_library_playlists(ModelRc::new(VecModel::from(
        (0..300)
            .map(|i| PlaylistRow {
                title: format!("Playlist {i}").into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    let liked = Rc::new(Cell::new(0));
    let n = liked.clone();
    ui.on_play_liked(move || n.set(n.get() + 1));
    let mix = Rc::new(Cell::new(0));
    let n = mix.clone();
    ui.on_open_supermix(move || n.set(n.get() + 1));
    let pauses = Rc::new(Cell::new(0));
    let n = pauses.clone();
    ui.on_toggle_pause(move || n.set(n.get() + 1));
    let shuffle = Rc::new(Cell::new(0));
    let n = shuffle.clone();
    ui.on_shuffle_queue(move || n.set(n.get() + 1));
    let repeats = Rc::new(Cell::new(0));
    let n = repeats.clone();
    let weak = ui.as_weak();
    ui.on_toggle_repeat(move || {
        n.set(n.get() + 1);
        let u = weak.upgrade().unwrap();
        u.set_repeat_enabled(!u.get_repeat_enabled());
    });
    ui.show().unwrap();
    for light in [false, true] {
        ui.set_light_mode(light);
        ui.set_lyrics_open(false);
        ui.set_queue_open(false);
        ui.set_jam_guest(false);
        export(
            if light {
                "desktop-home-light"
            } else {
                "desktop-home-dark"
            },
            1140,
            800,
            &draw(&window, 1140, 800),
        );
        // The compact rail preserves library access without duplicating Account.
        let before = liked.get();
        click(&ui, 44.0, 346.0);
        assert_eq!(liked.get(), before + 1, "Liked songs shortcut");
        let before = mix.get();
        click(&ui, 44.0, 418.0);
        assert_eq!(mix.get(), before + 1, "Supermix shortcut");
        click(&ui, 44.0, 186.0);
        assert!(ui.get_library_open(), "Library is reachable from the rail");
        click(&ui, 44.0, 114.0);
        assert!(!ui.get_library_open(), "Home is reachable from the rail");
        draw(&window, 1140, 800);
        let before = pauses.get();
        click(&ui, 570.0, 724.0);
        assert_eq!(pauses.get(), before + 1, "Play and pause in the footer");
        let before = shuffle.get();
        click(&ui, 468.0, 724.0);
        assert_eq!(shuffle.get(), before + 1, "Shuffle beside transport");
        let before = repeats.get();
        click(&ui, 672.0, 724.0);
        assert_eq!(repeats.get(), before + 1, "Repeat beside transport");
        ui.set_jam_guest(true);
        draw(&window, 1140, 800);
        let before = (pauses.get(), shuffle.get(), repeats.get());
        click(&ui, 570.0, 724.0);
        click(&ui, 468.0, 724.0);
        click(&ui, 672.0, 724.0);
        assert_eq!(
            (pauses.get(), shuffle.get(), repeats.get()),
            before,
            "Host-only controls stay disabled for guests"
        );
        ui.set_jam_guest(false);
        // Ctrl+L is captured even while an editable field has focus.
        click(&ui, 300.0, 32.0);
        let control: slint::SharedString = Key::Control.into();
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: control.clone(),
        });
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "l".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "l".into() });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: control.clone(),
        });
        assert!(ui.get_lyrics_open(), "Ctrl+L opens lyrics");
        key(&ui, Key::Escape);
        assert!(!ui.get_lyrics_open());
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: control.clone(),
        });
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "k".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "k".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: control });
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: "z".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: "z".into() });
        assert!(
            ui.get_source_input().ends_with('z'),
            "Ctrl+K focuses the search field"
        );
        ui.set_source_input("".into());
        export(
            if light {
                "desktop-narrow-light"
            } else {
                "desktop-narrow-dark"
            },
            650,
            420,
            &draw(&window, 650, 420),
        );
    }
}
