//! Native layout and input checks without a desktop, account, or audio engine.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
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

#[test]
fn typed_search_tabs_and_collections_keep_actions_at_each_size() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_search_open(true);
    let selected = Rc::new(Cell::new(-1));
    let n = selected.clone();
    let weak = ui.as_weak();
    ui.on_select_search_kind(move |index| {
        n.set(index);
        let ui = weak.upgrade().unwrap();
        ui.set_search_kind(index);
        ui.set_search_cards(ModelRc::new(VecModel::from(vec![RecommendationRow {
            id: "fixture".into(),
            title: "A long collection name that stays inside the search result".into(),
            subtitle: "Fixture artist".into(),
            kind: match index {
                1 => "album",
                2 => "artist",
                _ => "playlist",
            }
            .into(),
            ..Default::default()
        }])));
    });
    let opened = Rc::new(Cell::new(-1));
    let n = opened.clone();
    ui.on_select_search_card(move |index| n.set(index));
    let queued = Rc::new(Cell::new(false));
    let n = queued.clone();
    ui.on_queue_search_track(move |_, _| n.set(true));
    ui.show().unwrap();
    for (width, height) in [(1140, 800), (650, 420)] {
        for light in [false, true] {
            ui.set_light_mode(light);
            draw(&window, width, height);
            let nav = if width < 900 {
                ui.global::<LayoutMetrics>().get_navigation_narrow()
            } else {
                ui.global::<LayoutMetrics>().get_navigation_wide()
            };
            let left = nav + if height >= 650 { 28.0 } else { 18.0 };
            let tabs_y = if height >= 650 { 168.0 } else { 158.0 };
            for (index, offset) in [(4, 28.0), (1, 186.0), (2, 270.0), (3, 364.0)] {
                click(&ui, left + offset, tabs_y);
                assert_eq!(
                    selected.get(),
                    index,
                    "Search tab clipped at {width}x{height}"
                );
                let pixels = draw(&window, width, height);
                export(
                    &format!("search-kind-{index}-{width}-{light}"),
                    width,
                    height,
                    &pixels,
                );
                opened.set(-1);
                click(&ui, left + 30.0, if height >= 650 { 230.0 } else { 220.0 });
                assert_eq!(opened.get(), 0, "Wrong collection identity opened");
                assert!(
                    !queued.get(),
                    "Browsing collections must not enqueue a song"
                );
            }
            ui.set_search_busy(true);
            draw(&window, width, height);
            click(&ui, left + 186.0, tabs_y);
            assert_eq!(
                ui.get_search_kind(),
                3,
                "Busy search allowed overlapping category requests"
            );
            ui.set_search_busy(false);
        }
    }

    // Alt+Left must work even when the search input has focus. Closing a
    // collection first should reveal the same results and selected category.
    let weak = ui.as_weak();
    ui.on_close_detail(move || {
        let ui = weak.upgrade().unwrap();
        ui.set_forward_collection(true);
        ui.set_detail_open(false);
    });
    let weak = ui.as_weak();
    ui.on_close_search(move || weak.upgrade().unwrap().set_search_open(false));
    ui.set_detail_open(true);
    draw(&window, 1140, 800);
    click(&ui, 340.0, 32.0);
    let navigate = |direction: slint::platform::Key| {
        use slint::platform::Key;
        for text in [Key::Alt.into(), direction.into()] {
            ui.window().dispatch_event(WindowEvent::KeyPressed { text });
        }
        for text in [direction.into(), Key::Alt.into()] {
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
    };
    navigate(slint::platform::Key::LeftArrow);
    assert!(!ui.get_detail_open());
    assert!(ui.get_search_open());
    assert_eq!(ui.get_search_kind(), 3);
    navigate(slint::platform::Key::RightArrow);
    assert!(ui.get_detail_open());
    assert!(ui.get_search_open());
    navigate(slint::platform::Key::LeftArrow);
    navigate(slint::platform::Key::LeftArrow);
    assert!(!ui.get_search_open());
    navigate(slint::platform::Key::RightArrow);
    assert!(
        !ui.get_detail_open(),
        "Closing search must invalidate its forward collection"
    );
    assert!(
        ui.get_search_open(),
        "Forward should restore finished search"
    );
    assert_eq!(ui.get_search_kind(), 3);
    ui.set_settings_open(true);
    navigate(slint::platform::Key::LeftArrow);
    assert!(
        ui.get_search_open(),
        "Settings should block browse navigation"
    );
    ui.set_settings_open(false);
    ui.set_search_busy(true);
    navigate(slint::platform::Key::LeftArrow);
    assert!(!ui.get_search_open());
    ui.set_search_busy(false);
    navigate(slint::platform::Key::RightArrow);
    assert!(
        !ui.get_search_open(),
        "Forward must not reopen a canceled search"
    );

    let chosen = Rc::new(Cell::new(None));
    let captured = chosen.clone();
    ui.on_choose_suggestion(move |value| captured.set(Some(value)));
    ui.set_search_suggestions(ModelRc::new(VecModel::from(vec![
        slint::SharedString::from("First suggestion"),
        slint::SharedString::from("Second suggestion"),
    ])));
    ui.set_suggestions_open(true);
    draw(&window, 1140, 800);
    click(&ui, 340.0, 32.0);
    let press = |key: slint::platform::Key| {
        let text: slint::SharedString = key.into();
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    };
    press(slint::platform::Key::DownArrow);
    press(slint::platform::Key::DownArrow);
    press(slint::platform::Key::Return);
    assert_eq!(chosen.take().as_deref(), Some("Second suggestion"));
    assert!(!ui.get_suggestions_open());

    // The list hangs directly under the search field. Without an explicit y
    // it was centred in the window and floated mid-screen.
    ui.set_suggestions_open(true);
    draw(&window, 1140, 800);
    click(&ui, 300.0, 86.0);
    assert_eq!(chosen.take().as_deref(), Some("First suggestion"));
}
