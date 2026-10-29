//! Render the actual cover layout and exercise its source indices at breakpoints.
//! Synthetic review fixtures only; no account, network, or audio engine.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
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
    let weak = ui.as_weak();
    ui.on_toggle_library_view(move || {
        let u = weak.upgrade().unwrap();
        u.set_library_grid(!u.get_library_grid());
    });
    ui.show().unwrap();
    for width in [650, 780, 1140, 1536] {
        for light in [false, true] {
            ui.set_light_mode(light);
            ui.set_library_open(false);
            ui.set_library_grid(true);
            render(&window, width, light, "home-initial");
            let columns = ui.get_home_columns();
            ui.set_home_browse_rows(ModelRc::new(VecModel::from(
                (0..((12 + columns - 1) / columns))
                    .map(|r| HomeBrowseRow {
                        start: r * columns,
                        count: (12 - r * columns).min(columns),
                        title: if r == 0 { "Listen again" } else { "" }.into(),
                    })
                    .collect::<Vec<_>>(),
            )));
            render(&window, width, light, "home");
            let left = if width < 900 { 148.0 } else { 180.0 };
            let available = width as f32 - left - 28.0;
            for column in 0..columns {
                selected.set((-1, -1));
                click(
                    &ui,
                    left + column as f32 * (available + 8.0) / columns as f32 + 30.0,
                    230.0,
                );
                assert_eq!(
                    selected.get(),
                    (0, column),
                    "Home tile action changed source index at width {width}"
                );
            }
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
}
