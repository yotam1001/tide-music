//! Settings → Audio equalizer presets, synthetic and without playback.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::RefCell, rc::Rc};
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
fn equalizer_chips_choose_each_preset() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_settings_open(true);
    ui.set_settings_audio_open(true);
    let chosen = Rc::new(RefCell::new(Vec::new()));
    let captured = chosen.clone();
    let weak = ui.as_weak();
    ui.on_choose_equalizer(move |index| {
        captured.borrow_mut().push(index);
        weak.upgrade().unwrap().set_equalizer(index);
    });
    ui.show().unwrap();
    draw(&window, 1140, 800, "equalizer-off");
    // Two rows of three chips: Off, Bass boost, Bass reducer / Treble boost,
    // Vocal, Loudness.
    for (index, (x, y)) in [
        (1, (944.0, 587.0)),
        (2, (1056.0, 587.0)),
        (3, (832.0, 631.0)),
        (4, (944.0, 631.0)),
        (5, (1056.0, 631.0)),
        (0, (832.0, 587.0)),
    ] {
        click(&ui, x, y);
        draw(&window, 1140, 800, &format!("equalizer-{index}"));
        assert_eq!(
            chosen.borrow().last(),
            Some(&index),
            "chip for preset {index}"
        );
        assert_eq!(ui.get_equalizer(), index);
    }
    assert_eq!(*chosen.borrow(), [1, 2, 3, 4, 5, 0]);
}
