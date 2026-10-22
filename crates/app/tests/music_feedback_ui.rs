//! Render/input checks with synthetic data. No provider, session or account.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::RefCell, rc::Rc};
slint::include_modules!();
struct Platform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}
fn draw(window: &MinimalSoftwareWindow, width: u32, height: u32, name: &str) {
    window.set_size(PhysicalSize::new(width, height));
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    if let Some(path) = std::env::var_os("TIDE_UI_REVIEW_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        let bytes = pixels.into_iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::RgbImage::from_raw(width, height, bytes)
            .unwrap()
            .save(path.join(format!("{name}.png")))
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
#[test]
fn native_ratings_remain_reachable_and_do_not_take_over_transport() {
    // draw() allocates a fresh image on every call, so request full frames.
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_track_title("Synthetic feedback song".into());
    ui.set_track_artist("Fixture artist".into());
    ui.set_rating_enabled(true);
    let ratings = Rc::new(RefCell::new(Vec::new()));
    let captured = ratings.clone();
    let weak = ui.as_weak();
    ui.on_rate_song(move |rating| {
        captured.borrow_mut().push(rating);
        weak.upgrade().unwrap().set_current_rating(rating);
    });
    let transport = Rc::new(RefCell::new(0));
    let captured = transport.clone();
    ui.on_toggle_pause(move || *captured.borrow_mut() += 1);
    ui.show().unwrap();
    for (width, height, light) in [(1140, 800, false), (650, 420, true)] {
        ui.set_light_mode(light);
        ui.set_current_rating(0);
        draw(&window, width, height, &format!("ratings-{width}"));
        let x = width as f32 - 210.0;
        let y = height as f32 - 74.0;
        let start = ratings.borrow().len();
        for position in [x, x, x + 40.0, x + 40.0] {
            click(&ui, position, y);
        }
        assert_eq!(
            ratings.borrow().len(),
            start + 4,
            "Rating buttons are not reachable at width {width}"
        );
        assert_eq!(&ratings.borrow()[start..], &[1, 0, -1, 0]);
        assert_eq!(*transport.borrow(), 0);
        ui.set_rating_busy(true);
        let before = ratings.borrow().len();
        click(&ui, x, y);
        assert_eq!(
            ratings.borrow().len(),
            before,
            "Busy ratings issued another write"
        );
        ui.set_rating_busy(false);
    }
    ui.set_light_mode(false);
    ui.set_jam_available(true);
    ui.set_jam_active(true);
    ui.set_jam_guest(true);
    ui.set_collapsed(true);
    draw(&window, 700, 166, "ratings-mini-guest");
    let before = ratings.borrow().len();
    click(&ui, 588.0, 67.0);
    assert_eq!(
        ratings.borrow().len(),
        before + 1,
        "Guest mini-player lost local rating controls"
    );
    click(&ui, 350.0, 67.0);
    assert_eq!(*transport.borrow(), 0, "A rating enabled guest transport");
    ui.set_collapsed(false);
    ui.set_jam_active(false);
    ui.set_jam_guest(false);
    ui.set_settings_open(true);
    let consent = Rc::new(RefCell::new(false));
    let captured = consent.clone();
    let weak = ui.as_weak();
    ui.on_toggle_history_feedback(move || {
        *captured.borrow_mut() = true;
        weak.upgrade().unwrap().set_history_feedback_enabled(true);
    });
    draw(&window, 1140, 1400, "history-settings-off");
    assert!(!ui.get_history_feedback_enabled());
    click(&ui, 920.0, 937.0);
    assert!(
        *consent.borrow(),
        "History consent is not reachable in Settings"
    );
    draw(&window, 1140, 1400, "history-settings-on");
    ui.set_settings_open(false);
    ui.set_collection_enabled(true);
    ui.set_playlist_picker_song(
        "A long selected song title that must stay inside the chooser".into(),
    );
    ui.set_playlist_choices(slint::ModelRc::new(slint::VecModel::from(vec![
        "First editable playlist".into(),
        "Second editable playlist".into(),
    ])));
    let chosen = Rc::new(RefCell::new(Vec::new()));
    let captured = chosen.clone();
    ui.on_pick_playlist(move |index| captured.borrow_mut().push(index));
    let refreshed = Rc::new(RefCell::new(0));
    let captured = refreshed.clone();
    ui.on_refresh_playlist_choices(move || *captured.borrow_mut() += 1);
    let weak = ui.as_weak();
    ui.on_cancel_playlist_picker(move || weak.upgrade().unwrap().set_playlist_picker_open(false));
    for (width, height, light) in [(1140, 800, false), (650, 420, true)] {
        ui.set_light_mode(light);
        ui.set_playlist_picker_open(true);
        draw(&window, width, height, &format!("playlist-picker-{width}"));
        let x = width as f32 - 200.0;
        let before = chosen.borrow().len();
        click(&ui, x, 168.0);
        assert_eq!(
            chosen.borrow().len(),
            before + 1,
            "Playlist selection is clipped"
        );
        assert_eq!(chosen.borrow().last(), Some(&0));
        ui.set_playlist_choices_busy(true);
        click(&ui, x, 168.0);
        assert_eq!(
            chosen.borrow().len(),
            before + 1,
            "Refreshing picker accepted stale choices"
        );
        ui.set_playlist_choices_busy(false);
        let before = *refreshed.borrow();
        click(&ui, x, height as f32 - 174.0);
        assert_eq!(*refreshed.borrow(), before + 1, "Refresh is clipped");
        click(&ui, width as f32 - 28.0, 94.0);
        assert!(!ui.get_playlist_picker_open(), "Chooser cannot be closed");
        ui.set_playlist_picker_open(true);
        draw(&window, width, height, "playlist-picker-keyboard");
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::Escape.into(),
        });
        assert!(
            !ui.get_playlist_picker_open(),
            "Escape did not close chooser"
        );
    }
    ui.set_light_mode(false);
    ui.set_account_connected(true);
    ui.set_search_open(true);
    ui.set_search_rows(slint::ModelRc::new(slint::VecModel::from(vec![TrackRow {
        title: "Synthetic collection song".into(),
        artist: "Fixture artist".into(),
        ..Default::default()
    }])));
    let saved = Rc::new(RefCell::new(Vec::new()));
    let captured = saved.clone();
    ui.on_add_to_playlist(move |origin, index| captured.borrow_mut().push((origin, index)));
    for (guest, down_count) in [(false, 4), (true, 2)] {
        ui.set_jam_guest(guest);
        draw(&window, 1140, 800, "playlist-menu-before");
        click(&ui, 1087.0, 185.0);
        draw(
            &window,
            1140,
            800,
            if guest {
                "playlist-menu-guest"
            } else {
                "playlist-menu-search"
            },
        );
        for _ in 0..down_count {
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: slint::platform::Key::DownArrow.into(),
            });
            ui.window().dispatch_event(WindowEvent::KeyReleased {
                text: slint::platform::Key::DownArrow.into(),
            });
        }
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::Return.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: slint::platform::Key::Return.into(),
        });
        assert_eq!(
            saved.borrow().last(),
            Some(&(2, 0)),
            "Song menu did not preserve source/entry selection"
        );
        assert_eq!(
            saved.borrow().len(),
            if guest { 2 } else { 1 },
            "Guest cannot manage their own playlists"
        );
    }
    ui.set_jam_guest(false);
    ui.set_search_open(false);
    ui.set_detail_open(true);
    ui.set_detail_title("Editable fixture playlist".into());
    ui.set_detail_rows(ui.get_search_rows());
    ui.set_detail_editable(true);
    ui.set_detail_editable_entries(slint::ModelRc::new(slint::VecModel::from(vec![true])));
    let removed = Rc::new(RefCell::new(Vec::new()));
    let captured = removed.clone();
    ui.on_remove_playlist_song(move |index| captured.borrow_mut().push(index));
    draw(&window, 1140, 800, "playlist-editable");
    click(&ui, 1087.0, 235.0);
    draw(&window, 1140, 800, "playlist-menu-owned");
    for _ in 0..5 {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::DownArrow.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: slint::platform::Key::DownArrow.into(),
        });
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    assert_eq!(
        &*removed.borrow(),
        &[0],
        "Owned playlist removal is inaccessible"
    );
}
