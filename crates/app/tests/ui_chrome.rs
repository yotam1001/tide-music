//! Native layout and input checks without a desktop, account, or audio engine.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Key, PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::Cell, rc::Rc};
slint::include_modules!();

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
fn tracks() -> slint::ModelRc<TrackRow> {
    Rc::new(slint::VecModel::from(vec![
        TrackRow {
            title: "A Quiet Sun".into(),
            artist: "True North".into(),
            duration: "4:21".into(),
            ..Default::default()
        },
        TrackRow {
            title: "Hollow Coast".into(),
            artist: "Echo Frame".into(),
            duration: "3:36".into(),
            ..Default::default()
        },
        TrackRow {
            title: "Still Here".into(),
            artist: "Maris".into(),
            duration: "3:52".into(),
            recommended: true,
            ..Default::default()
        },
    ]))
    .into()
}

#[test]
fn settings_theme_account_and_song_menu_remain_independent() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_rows(tracks());
    ui.set_current_index(0);
    ui.set_track_title("A Quiet Sun".into());
    ui.set_track_artist("True North".into());
    ui.set_paused(true);
    ui.set_autoplay_enabled(true);
    ui.set_duration(261.0);
    ui.set_position(78.0);
    ui.set_volume(36.0);
    ui.set_search_rows(tracks());
    ui.set_recommendations(
        Rc::new(slint::VecModel::from(
            [
                ("A Quiet Sun", "True North", "song"),
                ("Hollow Coast", "Echo Frame", "song"),
                ("Your evening mix", "A mix for you", "playlist"),
                ("Still Here", "Maris", "song"),
                ("Drift", "Nova Wells", "song"),
                ("Open Water", "True North", "song"),
                ("Midnight Geometry", "Echo Frame", "song"),
                ("Glass Horizon", "Maris", "song"),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (title, subtitle, kind))| RecommendationRow {
                id: format!("sample-{index}").into(),
                title: title.into(),
                subtitle: subtitle.into(),
                kind: kind.into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
        ))
        .into(),
    );
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_account_name("Sample listener".into());
    let weak = ui.as_weak();
    ui.on_switch_mode(move || {
        let ui = weak.upgrade().unwrap();
        ui.set_light_mode(!ui.get_light_mode());
    });
    let weak = ui.as_weak();
    ui.on_toggle_compact(move || {
        let ui = weak.upgrade().unwrap();
        ui.set_compact_mode(!ui.get_compact_mode());
    });
    let queue_action = Rc::new(Cell::new(None));
    let capture = queue_action.clone();
    ui.on_queue_search_track(move |index, next| capture.set(Some((index, next))));
    let radio_action = Rc::new(Cell::new(None));
    let capture = radio_action.clone();
    ui.on_start_track_radio(move |source, index| capture.set(Some((source, index))));

    ui.show().unwrap();
    let without_status = draw(&window, 1140, 800);
    ui.set_status_text("Playing".into());
    assert_eq!(
        draw(&window, 1140, 800),
        without_status,
        "routine playback status must not add a strip or shift the layout"
    );

    ui.set_account_connected(false);
    ui.set_account_verified(false);
    let idle = draw(&window, 1140, 800);
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(922.0, 32.0),
    });
    assert_ne!(
        draw(&window, 1140, 800),
        idle,
        "theme control needs hover feedback"
    );
    assert!(!ui.get_light_mode(), "hover must not switch the theme");
    click(&ui, 992.0, 32.0);
    assert!(
        ui.get_settings_open(),
        "Settings must work without an account"
    );
    key(&ui, Key::Escape);
    ui.set_account_connected(true);
    ui.set_account_verified(true);

    for (width, height) in [(1536, 1024), (1140, 800), (780, 720), (650, 420)] {
        ui.set_account_open(false);
        ui.set_settings_open(false);
        ui.set_search_open(false);
        ui.set_light_mode(false);
        export(
            &format!("home-{width}"),
            width,
            height,
            &draw(&window, width, height),
        );
        // The theme switch is reachable without opening an account or resizing.
        click(&ui, width as f32 - 218.0, 32.0);
        assert!(
            ui.get_light_mode(),
            "direct theme control is inaccessible at {width}"
        );
        assert!(!ui.get_settings_open() && !ui.get_account_open());
        assert_eq!(window.size(), PhysicalSize::new(width, height));
        assert!(!ui.get_compact_mode());
        export(
            &format!("home-light-{width}"),
            width,
            height,
            &draw(&window, width, height),
        );

        click(&ui, width as f32 - 148.0, 32.0);
        assert!(
            ui.get_settings_open(),
            "Settings must be a top-level control"
        );
        export(
            &format!("settings-light-{width}"),
            width,
            height,
            &draw(&window, width, height),
        );
        key(&ui, Key::Escape);
        assert!(!ui.get_settings_open(), "Escape must close Settings");
        draw(&window, width, height);
        click(&ui, width as f32 - 57.0, 32.0);
        assert!(
            ui.get_account_open(),
            "header Account entry is inaccessible at {width}"
        );
        assert!(!ui.get_settings_open());
        export(
            &format!("account-{width}"),
            width,
            height,
            &draw(&window, width, height),
        );
        // Opening Settings must retire account UI, not stack two panels.
        ui.set_settings_open(true);
        draw(&window, width, height);
        assert!(!ui.get_account_open());
        ui.set_settings_open(false);
        draw(&window, width, height);
    }

    ui.set_update_available(true);
    ui.set_update_installable(true);
    ui.set_update_status("Tide 0.2.0 is available".into());
    ui.set_settings_open(true);
    for light in [true, false] {
        ui.set_light_mode(light);
        export(
            if light {
                "updates-light"
            } else {
                "updates-dark"
            },
            1140,
            800,
            &draw(&window, 1140, 800),
        );
    }
    ui.set_update_available(false);
    ui.set_settings_open(false);
    ui.set_account_connected(false);
    ui.set_account_verified(false);
    ui.set_account_name("".into());
    ui.set_account_open(true);
    export("account-disconnected", 1140, 800, &draw(&window, 1140, 800));
    ui.set_account_open(false);
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_account_name("Sample listener".into());

    ui.set_light_mode(false);
    ui.set_search_open(true);
    let before = draw(&window, 1140, 800);
    click(&ui, 1087.0, 185.0);
    let menu = draw(&window, 1140, 800);
    export("song-menu-dark", 1140, 800, &menu);
    assert_ne!(menu, before, "song action did not display a menu");
    assert_eq!(
        menu[220 * 1140 + 900],
        Rgb8Pixel {
            r: 25,
            g: 28,
            b: 34
        },
        "dark popup must use Tide's surface, not an OS menu theme"
    );
    ui.set_light_mode(true);
    let light_menu = draw(&window, 1140, 800);
    export("song-menu-light", 1140, 800, &light_menu);
    assert_eq!(
        light_menu[220 * 1140 + 900],
        Rgb8Pixel {
            r: 249,
            g: 244,
            b: 238
        },
        "open popup must follow Tide's light surface"
    );
    ui.set_light_mode(false);
    draw(&window, 1140, 800);

    // Opening actions must not move the next song or grow the original row.
    for y in 225..280 {
        let range = y * 1140 + 190..y * 1140 + 600;
        assert_eq!(
            &menu[range.clone()],
            &before[range],
            "song menu displaced the list"
        );
    }
    key(&ui, Key::Escape);
    draw(&window, 1140, 800);
    assert!(
        queue_action.get().is_none(),
        "dismissal must not queue a track"
    );
    click(&ui, 1087.0, 185.0);
    draw(&window, 1140, 800);
    click(&ui, 400.0, 420.0);
    let dismissed = draw(&window, 1140, 800);
    for y in 225..280 {
        let range = y * 1140 + 850..y * 1140 + 1020;
        assert_eq!(
            &dismissed[range.clone()],
            &before[range],
            "outside click did not dismiss the menu"
        );
    }
    assert!(queue_action.get().is_none());
    click(&ui, 1087.0, 185.0);
    draw(&window, 1140, 800);
    key(&ui, Key::DownArrow);
    key(&ui, Key::Return);
    draw(&window, 1140, 800);
    assert_eq!(
        queue_action.get(),
        Some((0, true)),
        "keyboard menu action must target the selected song"
    );
    for light in [false, true] {
        ui.set_light_mode(light);
        draw(&window, 1140, 800);
        click(&ui, 1087.0, 185.0);
        draw(&window, 1140, 800);
        key(&ui, Key::DownArrow);
        key(&ui, Key::DownArrow);
        key(&ui, Key::Return);
        draw(&window, 1140, 800);
        assert_eq!(
            queue_action.get(),
            Some((0, false)),
            "Add to queue must target this search row"
        );
        click(&ui, 1087.0, 185.0);
        draw(&window, 1140, 800);
        key(&ui, Key::UpArrow);
        key(&ui, Key::Return);
        draw(&window, 1140, 800);
        assert_eq!(
            radio_action.get(),
            Some((2, 0)),
            "Radio must target the search song, not the queue"
        );
        radio_action.set(None);
    }
    ui.set_light_mode(false);
    ui.set_search_open(false);
    ui.set_queue_open(true);
    export("queue-dark", 1140, 800, &draw(&window, 1140, 800));
    ui.set_light_mode(true);
    export("queue-light", 1140, 800, &draw(&window, 1140, 800));
    ui.set_queue_open(false);
    // Pinned entries must work even when no saved playlists or Home cards exist.
    let supermix_opens = Rc::new(Cell::new(0));
    let capture = supermix_opens.clone();
    ui.on_open_supermix(move || capture.set(capture.get() + 1));
    let saved_home = ui.get_recommendations();
    ui.set_recommendations(Rc::new(slint::VecModel::<RecommendationRow>::default()).into());
    ui.set_library_open(true);
    for (width, height, light) in [(1140, 800, false), (780, 720, true), (650, 420, false)] {
        ui.set_light_mode(light);
        let pixels = draw(&window, width, height);
        export(&format!("library-supermix-{width}"), width, height, &pixels);
        click(&ui, 240.0, 205.0);
    }
    assert_eq!(
        supermix_opens.get(),
        3,
        "My Supermix stays reachable in Library at every window size"
    );
    ui.set_account_connected(false);
    draw(&window, 650, 420);
    click(&ui, 240.0, 205.0);
    assert_eq!(
        supermix_opens.get(),
        3,
        "Disconnected Supermix must not send a request"
    );
    ui.set_account_connected(true);
    ui.set_recommendations(saved_home);
    ui.set_light_mode(true);
    ui.set_library_playlists(
        Rc::new(slint::VecModel::from(vec![
            PlaylistRow {
                id: "demo-focus".into(),
                title: "Late night focus".into(),
                subtitle: "18 tracks".into(),
                ..Default::default()
            },
            PlaylistRow {
                id: "demo-chill".into(),
                title: "Game night".into(),
                subtitle: "24 tracks".into(),
                ..Default::default()
            },
            PlaylistRow {
                id: "demo-ambient".into(),
                title: "Open water".into(),
                subtitle: "12 tracks".into(),
                ..Default::default()
            },
        ]))
        .into(),
    );
    ui.set_library_open(true);
    export("library-light", 1140, 800, &draw(&window, 1140, 800));
    ui.set_light_mode(false);
    export("library", 1140, 800, &draw(&window, 1140, 800));
    ui.set_compact_mode(true);
    export("compact-library", 780, 720, &draw(&window, 780, 720));
    ui.set_library_open(false);
    ui.set_queue_open(true);
    export("compact-queue", 780, 720, &draw(&window, 780, 720));
    ui.set_queue_open(false);
    ui.set_compact_mode(false);
    ui.set_search_open(true);
    export("search", 1140, 800, &draw(&window, 1140, 800));
    ui.set_search_open(false);
    ui.set_detail_open(true);
    ui.set_detail_title("Late night focus".into());
    ui.set_detail_rows(tracks());
    export("playlist-detail", 1140, 800, &draw(&window, 1140, 800));
    ui.set_detail_open(false);
    ui.set_collapsed(true);
    export("collapsed", 700, 132, &draw(&window, 700, 132));
    ui.set_collapsed(false);
    export("restored", 1140, 800, &draw(&window, 1140, 800));
}
