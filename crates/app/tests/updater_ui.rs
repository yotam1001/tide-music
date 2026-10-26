//! Synthetic native updater controls. No network, account or installer is used.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::Cell, rc::Rc};
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
    // Settle the capsule/progress transitions before hit-testing and snapshots.
    std::thread::sleep(std::time::Duration::from_millis(180));
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
fn update_pill_download_cancel_and_restart_do_not_touch_playback() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_update_available(true);
    ui.set_update_installable(true);
    ui.set_track_title("Synthetic update fixture".into());
    let downloads = Rc::new(Cell::new(0));
    let restarts = Rc::new(Cell::new(0));
    let cancellations = Rc::new(Cell::new(0));
    let playback = Rc::new(Cell::new(0));
    let d = downloads.clone();
    let r = restarts.clone();
    let weak = ui.as_weak();
    ui.on_install_update(move || {
        let ui = weak.upgrade().unwrap();
        if ui.get_update_phase() == 2 {
            r.set(r.get() + 1);
        } else {
            d.set(d.get() + 1);
            ui.set_update_phase(1);
            ui.set_update_progress(37.0);
            ui.set_update_busy(true);
        }
    });
    let c = cancellations.clone();
    let weak = ui.as_weak();
    ui.on_cancel_update(move || {
        c.set(c.get() + 1);
        let ui = weak.upgrade().unwrap();
        ui.set_update_phase(3);
    });
    let p = playback.clone();
    ui.on_toggle_pause(move || p.set(p.get() + 1));
    ui.show().unwrap();
    for light in [false, true] {
        ui.set_light_mode(light);
        ui.set_update_phase(0);
        ui.set_update_busy(false);
        ui.set_update_progress(0.0);
        draw(
            &window,
            1140,
            800,
            if light {
                "updater-available-light"
            } else {
                "updater-available-dark"
            },
        );
        let d = downloads.get();
        click(&ui, 920.0, 32.0);
        assert_eq!(downloads.get(), d + 1, "Update action is not reachable");
        draw(
            &window,
            1140,
            800,
            if light {
                "updater-downloading-light"
            } else {
                "updater-downloading-dark"
            },
        );
        click(&ui, 920.0, 32.0);
        assert_eq!(
            downloads.get(),
            d + 1,
            "Busy update started another download"
        );
        let c = cancellations.get();
        click(&ui, 1014.0, 32.0);
        assert_eq!(cancellations.get(), c + 1, "Cancel is not reachable");
        draw(&window, 1140, 800, "updater-canceling");
        click(&ui, 1014.0, 32.0);
        assert_eq!(cancellations.get(), c + 1);
        ui.set_update_phase(2);
        ui.set_update_busy(false);
        ui.set_update_progress(100.0);
        draw(
            &window,
            1140,
            800,
            if light {
                "updater-ready-light"
            } else {
                "updater-ready-dark"
            },
        );
        let r = restarts.get();
        click(&ui, 920.0, 32.0);
        assert_eq!(restarts.get(), r + 1, "Restart is not reachable");
        ui.set_update_phase(4);
        ui.set_update_busy(true);
        draw(&window, 1140, 800, "updater-preparing-restart");
        click(&ui, 920.0, 32.0);
        assert_eq!(
            restarts.get(),
            r + 1,
            "Preparing started a duplicate restart"
        );
        let c = cancellations.get();
        click(&ui, 1014.0, 32.0);
        assert_eq!(
            cancellations.get(),
            c + 1,
            "Preparing restart could not be canceled"
        );
        ui.set_update_phase(2);
        ui.set_update_busy(false);
        draw(&window, 1140, 800, "updater-retry-restart");
        click(&ui, 920.0, 32.0);
        assert_eq!(
            restarts.get(),
            r + 2,
            "Restart remained blocked after a failed/canceled attempt"
        );
        assert_eq!(playback.get(), 0);
        assert_eq!(ui.get_track_title(), "Synthetic update fixture");
    }
    draw(&window, 650, 420, "updater-narrow-header");
    click(&ui, 568.0, 32.0);
    assert!(
        ui.get_settings_open(),
        "Narrow window lost Settings update access"
    );
    draw(&window, 650, 1600, "updater-narrow-settings");
}
