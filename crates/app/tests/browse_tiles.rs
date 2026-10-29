//! Render the actual cover layout and exercise its source indices at breakpoints.
//! Synthetic review fixtures only; no account, network, or audio engine.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Key, PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, Model, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::{cell::Cell, rc::Rc};
use tide_music::*;
#[path = "../src/browse_layout.rs"]
mod browse_layout;

struct Platform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}
fn render(window: &MinimalSoftwareWindow, width: u32, light: bool, name: &str) {
    window.set_size(PhysicalSize::new(width, 800));
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * 800];
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
        let bytes = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::RgbImage::from_raw(width, 800, bytes)
            .unwrap()
            .save(dir.join(format!("tiles-{name}-{width}-{light}.png")))
            .unwrap();
    }
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
fn cover_grid_and_list_preserve_song_and_playlist_actions() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_recommendations(ModelRc::new(VecModel::from(
        (0..12)
            .map(|i| RecommendationRow {
                cover_variant: i % 4,
                title: format!("Song {i} with a long title for clipping").into(),
                subtitle: "Artist with a long name".into(),
                kind: if i == 1 { "playlist" } else { "song" }.into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    ui.set_library_playlists(ModelRc::new(VecModel::from(
        (0..12)
            .map(|i| PlaylistRow {
                cover_variant: i % 4,
                title: format!("Playlist {i}").into(),
                subtitle: "A collection from your library".into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    let selected = Rc::new(Cell::new((-1, -1)));
    let capture = selected.clone();
    ui.on_select_recommendation(move |i| capture.set((0, i)));
    let capture = selected.clone();
    ui.on_select_library_playlist(move |i| capture.set((1, i)));
    let queued = Rc::new(Cell::new(None));
    let capture = queued.clone();
    ui.on_queue_recommendation(move |i, next| capture.set(Some((i, next))));
    let weak = ui.as_weak();
    ui.on_toggle_library_view(move || {
        let u = weak.upgrade().unwrap();
        u.set_library_grid(!u.get_library_grid());
    });
    ui.show().unwrap();
    for width in [650, 780, 1140, 1536, 1920] {
        for light in [false, true] {
            ui.set_light_mode(light);
            ui.set_library_open(false);
            ui.set_library_grid(true);
            render(&window, width, light, "home-initial");
            let columns = ui.get_home_columns();
            assert!(
                width < 1140 || columns >= 5,
                "Home retained a narrow screen's column count after resize"
            );
            let groups = browse_layout::home_groups(
                &vec!["Listen again".into(); 12],
                &(0..12).map(|i| i == 1).collect::<Vec<_>>(),
                columns as usize,
            );
            let spotlight_count = groups[0].count as i32;
            ui.set_home_browse_rows(ModelRc::new(VecModel::from(
                groups
                    .iter()
                    .map(|g| HomeBrowseRow {
                        start: g.start as i32,
                        count: g.count as i32,
                        title: g.title.clone().into(),
                        layout: match g.layout {
                            browse_layout::Layout::Spotlight => "spotlight",
                            browse_layout::Layout::Compact => "compact",
                            browse_layout::Layout::Tiles => "tiles",
                        }
                        .into(),
                    })
                    .collect::<Vec<_>>(),
            )));
            render(&window, width, light, "home");
            let nav = if width < 900 { 120.0 } else { 152.0 };
            let page = width as f32 - nav;
            let left = nav + (width as f32 - nav - page) / 2.0 + 28.0;
            let available = page - 56.0;
            for index in 0..if spotlight_count > 1 { 2 } else { 1 } {
                selected.set((-1, -1));
                click(
                    &ui,
                    if index == 0 {
                        left + 30.0
                    } else {
                        left + available * 0.48 + 42.0
                    },
                    230.0,
                );
                assert_eq!(
                    selected.get(),
                    (0, index),
                    "Home tile action changed source index at width {width}"
                );
            }
            // The tile's action popup must remain usable despite cover clipping.
            for index in [0, spotlight_count - 1] {
                if index == 1 {
                    continue;
                }
                queued.set(None);
                let menu_x = if index == 0 {
                    left + available * (if spotlight_count > 1 { 0.48 } else { 1.0 }) - 30.0
                } else {
                    left + available - 30.0
                };
                let lane = (ui.global::<LayoutMetrics>().get_home_spotlight_height() - 12.0) / 2.0;
                click(
                    &ui,
                    menu_x,
                    if index == 0 {
                        225.0
                    } else {
                        196.0 + lane + 12.0 + lane / 2.0
                    },
                );
                render(
                    &window,
                    width,
                    light,
                    if index == 0 {
                        "home-menu"
                    } else {
                        "home-menu-edge"
                    },
                );
                for value in [Key::DownArrow, Key::Return] {
                    let text: slint::SharedString = value.into();
                    ui.window()
                        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
                    ui.window()
                        .dispatch_event(WindowEvent::KeyReleased { text });
                }
                assert_eq!(
                    queued.get(),
                    Some((index, true)),
                    "Tile menu lost its queue source index"
                );
            }
            let original = ui.get_recommendations();
            let mut collections = (0..original.row_count())
                .map(|i| original.row_data(i).unwrap())
                .collect::<Vec<_>>();
            collections[0].kind = "playlist".into();
            ui.set_recommendations(ModelRc::new(VecModel::from(collections)));
            render(&window, width, light, "featured-playlist");
            selected.set((-1, -1));
            click(&ui, left + 30.0, 230.0);
            assert_eq!(
                selected.get(),
                (0, 0),
                "Featured playlist must open its source, not play a different song"
            );
            ui.set_recommendations(original);
            ui.set_library_open(true);
            render(&window, width, light, "library-grid");
            let columns = ui.get_library_columns();
            for column in 0..columns {
                selected.set((-1, -1));
                click(
                    &ui,
                    left + column as f32 * (available + 8.0) / columns as f32 + 30.0,
                    330.0,
                );
                assert_eq!(
                    selected.get(),
                    (1, column),
                    "Library tile changed source index at width {width}"
                );
            }
            ui.invoke_toggle_library_view();
            assert!(!ui.get_library_grid());
            render(&window, width, light, "library-list");
            selected.set((-1, -1));
            click(&ui, left + 100.0, 310.0);
            assert_eq!(
                selected.get(),
                (1, 0),
                "List view must keep the first playlist"
            );
        }
    }
    // Exercise the real update control without launching any installer here.
    // Signature/handoff and the actual previous-release install run separately.
    let updates = Rc::new(Cell::new(0));
    let capture = updates.clone();
    ui.on_install_update(move || capture.set(capture.get() + 1));
    ui.set_update_available(true);
    ui.set_update_installable(true);
    ui.set_update_phase(2);
    render(&window, 1140, false, "update-ready");
    click(&ui, 860.0, 32.0);
    assert_eq!(
        updates.get(),
        1,
        "Verified restart action must stay reachable in the header"
    );
    ui.set_update_phase(4);
    ui.set_update_busy(true);
    render(&window, 1140, false, "update-preparing");
    click(&ui, 860.0, 32.0);
    assert_eq!(
        updates.get(),
        1,
        "Preparing restart must reject duplicate clicks"
    );
}
