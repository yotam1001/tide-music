//! Synthetic native updater controls. No network, account or installer is used.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
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
fn settings_recheck_recovers_download_and_restart_actions() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    let checks = Rc::new(Cell::new(0));
    let downloads = Rc::new(Cell::new(0));
    let restarts = Rc::new(Cell::new(0));
    let playback = Rc::new(Cell::new(0));
    let count = checks.clone();
    let weak = ui.as_weak();
    ui.on_check_updates(move || {
        count.set(count.get() + 1);
        weak.upgrade().unwrap().set_update_installable(true);
    });
    let d = downloads.clone();
    let r = restarts.clone();
    let weak = ui.as_weak();
    ui.on_install_update(move || {
        if weak.upgrade().unwrap().get_update_phase() == 2 {
            r.set(r.get() + 1);
        } else {
            d.set(d.get() + 1);
        }
    });
    let p = playback.clone();
    ui.on_toggle_pause(move || p.set(p.get() + 1));
    ui.show().unwrap();
    for width in [650, 1140] {
        for light in [false, true] {
            ui.set_light_mode(light);
            ui.set_settings_open(true);
            ui.set_update_available(true);
            ui.set_update_installable(false);
            ui.set_update_phase(0);
            ui.set_update_busy(false);
            ui.set_update_status("Tide fixture is available".into());
            draw(&window, width, 800, "settings-update-recheck");
            let before = checks.get();
            for y in (460..670).step_by(4) {
                click(&ui, width as f32 - 168.0, y as f32);
                if checks.get() > before {
                    break;
                }
            }
            assert_eq!(
                checks.get(),
                before + 1,
                "Settings must offer rechecking when startup eligibility was false"
            );
            assert!(ui.get_update_installable());
            draw(&window, width, 800, "settings-update-download");
            let before = downloads.get();
            for y in (460..670).step_by(4) {
                click(&ui, width as f32 - 168.0, y as f32);
                if downloads.get() > before {
                    break;
                }
            }
            assert_eq!(
                downloads.get(),
                before + 1,
                "Settings must offer a reachable Download action after rechecking"
            );
            ui.set_update_phase(2);
            ui.set_update_progress(100.0);
            draw(&window, width, 800, "settings-update-restart");
            let before = restarts.get();
            for y in (460..670).step_by(4) {
                click(&ui, width as f32 - 200.0, y as f32);
                if restarts.get() > before {
                    break;
                }
            }
            assert_eq!(
                restarts.get(),
                before + 1,
                "Settings must offer the verified restart action"
            );
            assert_eq!(
                playback.get(),
                0,
                "Update controls must not toggle playback"
            );
        }
    }
}
