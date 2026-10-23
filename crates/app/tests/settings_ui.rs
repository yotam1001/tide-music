//! Synthetic native updater controls. No network, account or installer is used.
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
fn shortcuts_capture_keys_cancel_and_clear_without_transport() {
    use slint::platform::Key;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_settings_open(true);
    ui.set_app_version("test".into());
    let weak = ui.as_weak();
    ui.on_begin_shortcut(move |i| weak.upgrade().unwrap().set_shortcut_recording_index(i));
    let weak = ui.as_weak();
    ui.on_cancel_shortcut(move || weak.upgrade().unwrap().set_shortcut_recording_index(-1));
    let captured = Rc::new(RefCell::new(Vec::new()));
    let c = captured.clone();
    let weak = ui.as_weak();
    ui.on_record_shortcut(move |text, ctrl, alt, shift, meta| {
        if text == "p" {
            c.borrow_mut()
                .push((text.to_string(), ctrl, alt, shift, meta));
            let u = weak.upgrade().unwrap();
            u.set_shortcut_toggle("Ctrl+Alt+P".into());
            u.set_shortcut_recording_index(-1);
        }
    });
    let cleared = Rc::new(RefCell::new(Vec::new()));
    let c = cleared.clone();
    let weak = ui.as_weak();
    ui.on_clear_shortcut(move |i| {
        c.borrow_mut().push(i);
        weak.upgrade().unwrap().set_shortcut_toggle("".into());
    });
    ui.show().unwrap();
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    for light in [false, true] {
        ui.set_light_mode(light);
        ui.set_settings_shortcuts_open(false);
        draw(
            &window,
            1140,
            800,
            if light {
                "settings-simple-light"
            } else {
                "settings-simple-dark"
            },
        );
        // The top-level panel opens shortcuts without revealing technical fields.
        click(&ui, 980.0, 414.0);
        assert!(ui.get_settings_shortcuts_open());
        draw(&window, 1140, 800, "settings-shortcuts");
        click(&ui, 1000.0, 296.0);
        assert_eq!(ui.get_shortcut_recording_index(), 0);
        draw(&window, 1140, 800, "settings-recording");
        for text in [Key::Control.into(), Key::Alt.into(), "p".into()] {
            ui.window().dispatch_event(WindowEvent::KeyPressed { text });
        }
        for text in ["p".into(), Key::Alt.into(), Key::Control.into()] {
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
        assert_eq!(
            captured.borrow().last(),
            Some(&("p".into(), true, true, false, false))
        );
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        click(&ui, 1000.0, 352.0);
        assert_eq!(ui.get_shortcut_recording_index(), 1);
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        assert!(ui.get_settings_open());
        assert_eq!(ui.get_shortcut_toggle(), "Ctrl+Alt+P");
        click(&ui, 1100.0, 296.0);
        assert_eq!(cleared.borrow().last(), Some(&0));
        assert!(ui.get_shortcut_toggle().is_empty());
        click(&ui, 1000.0, 408.0);
        assert_eq!(ui.get_shortcut_recording_index(), 2);
        ui.window()
            .dispatch_event(WindowEvent::WindowActiveChanged(false));
        assert_eq!(
            ui.get_shortcut_recording_index(),
            -1,
            "Losing focus left shortcuts suspended"
        );
        ui.window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
        click(&ui, 1000.0, 408.0);
        ui.set_settings_open(false);
        draw(&window, 1140, 800, "settings-closed");
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        ui.set_settings_open(true);
    }
    ui.set_settings_shortcuts_open(false);
    draw(&window, 650, 420, "settings-small-window");
    ui.set_settings_more_open(true);
    ui.set_settings_scroll_y(-180.0);
    ui.set_settings_open(false);
    draw(&window, 650, 420, "settings-small-closed");
    ui.set_settings_open(true);
    draw(&window, 650, 420, "settings-reopened");
    assert_eq!(
        ui.get_settings_scroll_y(),
        0.0,
        "Reopened Settings must start at the top"
    );
    ui.set_settings_scroll_y(-180.0);
    ui.set_settings_shortcuts_open(true);
    draw(&window, 650, 420, "settings-shortcuts-small-window");
    assert_eq!(
        ui.get_settings_scroll_y(),
        0.0,
        "Shortcut page inherited the previous scroll position"
    );
}
