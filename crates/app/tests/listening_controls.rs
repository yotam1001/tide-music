//! Exercise filtered actions and artwork using the real native UI bindings.
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
fn draw(window: &MinimalSoftwareWindow, width: u32, name: &str) {
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
        let bytes = pixels.into_iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        image::RgbImage::from_raw(width, 800, bytes)
            .unwrap()
            .save(dir.join(format!("{name}-{width}.png")))
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
fn filtered_playlist_and_queue_keep_absolute_indices_and_audio_has_its_own_page() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    let rows = ModelRc::new(VecModel::from(
        (0..4)
            .map(|i| TrackRow {
                title: format!("Song {i}").into(),
                artist: "Fixture band".into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    ));
    ui.set_detail_rows(rows.clone());
    ui.set_rows(rows);
    let selected = Rc::new(Cell::new(-1));
    let d = selected.clone();
    ui.on_select_detail_track(move |i| d.set(i));
    let q = selected.clone();
    ui.on_select_track(move |i| q.set(i));
    ui.show().unwrap();
    for width in [650, 1140] {
        for light in [false, true] {
            ui.set_light_mode(light);
            ui.set_detail_open(true);
            ui.set_queue_open(false);
            ui.set_settings_open(false);
            ui.set_detail_title("Test playlist".into());
            draw(&window, width, "playlist-before-filter");
            ui.set_detail_filter("Song 2".into());
            ui.set_detail_matches(ModelRc::new(VecModel::from(vec![2])));
            draw(&window, width, "playlist-filter");
            selected.set(-1);
            for y in (210..450).step_by(5) {
                click(&ui, 300.0, y as f32);
                if selected.get() >= 0 {
                    break;
                }
            }
            assert_eq!(
                selected.get(),
                2,
                "Filtered playlist selected the wrong source row"
            );
            ui.set_detail_open(false);
            ui.set_queue_open(true);
            ui.set_current_index(0);
            ui.set_queue_filter("Song 2".into());
            ui.set_queue_matches(ModelRc::new(VecModel::from(vec![2])));
            draw(&window, width, "queue-filter");
            selected.set(-1);
            for y in (160..470).step_by(5) {
                click(&ui, width as f32 - 170.0, y as f32);
                if selected.get() >= 0 {
                    break;
                }
            }
            assert_eq!(
                selected.get(),
                2,
                "Filtered queue selected the wrong source row"
            );
            ui.set_queue_filter("".into());
            ui.set_queue_open(false);
            ui.set_settings_open(true);
            ui.set_settings_audio_open(true);
            ui.set_audio_device_names(ModelRc::new(VecModel::from(vec![
                "Windows default".into(),
                "Headphones".into(),
            ])));
            ui.set_audio_status("Applies only to Tide".into());
            draw(&window, width, "audio-settings");
            assert!(!ui.get_settings_shortcuts_open());
            ui.set_settings_open(false);
            draw(&window, width, "audio-settings-closed");
            assert!(!ui.get_settings_audio_open());
        }
    }
}
