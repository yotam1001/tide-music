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
fn library_sections_keep_collection_and_history_actions_at_every_size() {
    use std::cell::RefCell;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_library_open(true);
    ui.set_collection_enabled(true);
    let selected = Rc::new(Cell::new(-1));
    let n = selected.clone();
    let weak = ui.as_weak();
    ui.on_select_library_section(move |section| {
        n.set(section);
        let ui = weak.upgrade().unwrap();
        ui.set_library_section(section);
        let kind = if section == 1 {
            "album"
        } else if section == 2 {
            "artist"
        } else {
            "song"
        };
        ui.set_library_cards(ModelRc::new(VecModel::from(vec![RecommendationRow {
            id: "fixture".into(),
            title: "A long collection or song title that must stay within its row".into(),
            subtitle: "Fixture artist".into(),
            kind: kind.into(),
            ..Default::default()
        }])));
    });
    let opened = Rc::new(Cell::new(-1));
    let n = opened.clone();
    ui.on_select_library_card(move |index| n.set(index));
    let artist = Rc::new(RefCell::new(None));
    let n = artist.clone();
    ui.on_open_artist(move |source, index| *n.borrow_mut() = Some((source, index)));
    let radio = Rc::new(RefCell::new(None));
    let n = radio.clone();
    ui.on_start_track_radio(move |source, index| *n.borrow_mut() = Some((source, index)));
    ui.show().unwrap();
    for (width, height) in [(1440, 900), (1140, 800), (650, 420)] {
        for light in [false, true] {
            ui.set_light_mode(light);
            draw(&window, width, height);
            let padding = if height >= 650 { 28.0 } else { 18.0 };
            let nav = if width < 900 {
                ui.global::<LayoutMetrics>().get_navigation_narrow()
            } else {
                ui.global::<LayoutMetrics>().get_navigation_wide()
            };
            let left = nav + padding;
            let tabs_y = if height >= 650 { 168.0 } else { 146.0 };
            for (section, offset) in [(1, 134.0), (2, 218.0), (3, 308.0)] {
                click(&ui, left + offset, tabs_y);
                assert_eq!(selected.get(), section, "Tab clipped at {width}x{height}");
                assert_eq!(ui.get_library_section(), section);
                let pixels = draw(&window, width, height);
                export(
                    &format!("library-section-{section}-{width}-{light}"),
                    width,
                    height,
                    &pixels,
                );
                if section == 3 {
                    assert_eq!(
                        ui.get_library_browse_columns(),
                        1,
                        "History must retain vertical listening order"
                    );
                }
                opened.set(-1);
                click(&ui, left + 30.0, if height >= 650 { 230.0 } else { 210.0 });
                assert_eq!(
                    opened.get(),
                    0,
                    "Collection/song action clipped at {width}x{height}"
                );
            }
            // Artist/radio use the history source, never the queue or Home's index zero.
            let row_y = if height >= 650 { 196.0 } else { 174.0 };
            *artist.borrow_mut() = None;
            click(&ui, left + 110.0, row_y + 47.0);
            assert_eq!(
                *artist.borrow(),
                Some((5, 0)),
                "History artist identity was lost"
            );
            click(&ui, width as f32 - padding - 25.0, row_y + 36.0);
            for _ in 0..3 {
                key(&ui, Key::DownArrow);
            }
            key(&ui, Key::Return);
            assert_eq!(
                *radio.borrow(),
                Some((5, 0)),
                "History radio must target its own song"
            );
            *radio.borrow_mut() = None;
            ui.set_account_busy(true);
            click(&ui, left + 44.0, tabs_y);
            assert_eq!(
                ui.get_library_section(),
                3,
                "Busy request changed browsing section"
            );
            ui.set_account_busy(false);
        }
    }

    assert_eq!(ui.get_active_navigation(), "library");
    ui.set_detail_open(true);
    ui.set_detail_navigation("liked".into());
    assert_eq!(ui.get_active_navigation(), "liked");
    ui.set_detail_navigation("supermix".into());
    assert_eq!(ui.get_active_navigation(), "supermix");
    ui.set_detail_navigation("".into());
    assert_eq!(ui.get_active_navigation(), "");
    ui.set_detail_open(false);
    assert_eq!(ui.get_active_navigation(), "library");
}
