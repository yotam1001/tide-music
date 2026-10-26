//! In-memory native rendering/input only: no desktop window or account is used.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
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
    let mut pixels = vec![Rgb8Pixel::default(); width as usize * height as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    pixels
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
fn guest_mini_keeps_leave_and_volume_reachable_and_blocks_transport() {
    // draw() allocates fresh pixels; partial repaint requires a retained buffer.
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_jam_available(true);
    ui.set_jam_active(true);
    ui.set_jam_guest(true);
    ui.set_jam_status("Following host · volume stays local".into());
    ui.set_track_title("Synthetic song".into());
    ui.set_duration(180.0);
    ui.set_position(30.0);
    ui.set_volume(50.0);
    ui.set_collapsed(true);
    let transport = Rc::new(Cell::new(0));
    for kind in 0..3 {
        let transport = transport.clone();
        let callback = move || transport.set(transport.get() + 1);
        match kind {
            0 => ui.on_toggle_pause(callback),
            1 => ui.on_previous_track(callback),
            _ => ui.on_next_track(callback),
        }
    }
    let left = Rc::new(Cell::new(0));
    let copied = left.clone();
    ui.on_jam_leave(move || copied.set(copied.get() + 1));
    let volume = Rc::new(Cell::new(-1.0));
    let copied = volume.clone();
    let weak = ui.as_weak();
    ui.on_change_volume(move |value| {
        copied.set(value);
        if let Some(ui) = weak.upgrade() {
            ui.set_volume(value);
        }
    });
    ui.show().unwrap(); // MinimalSoftwareWindow has no native desktop surface.
    let initial = draw(&window, 700, 166);
    if let Some(path) = std::env::var_os("TIDE_JAM_LAYOUT_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        let bytes: Vec<u8> = initial
            .iter()
            .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
            .collect();
        image::RgbImage::from_raw(700, 166, bytes)
            .unwrap()
            .save(path.join("initial-mini.png"))
            .unwrap();
    }
    for x in [295.0, 350.0, 403.0] {
        click(&ui, x, 66.0);
    }
    assert_eq!(
        transport.get(),
        1,
        "Guest Next must be accessible while Previous and Pause remain host-controlled"
    );
    click(&ui, 620.0, 17.0);
    assert_eq!(left.get(), 1, "Leave is inaccessible in mini mode");
    click(&ui, 615.0, 113.0);
    assert!(
        volume.get() >= 0.0,
        "Guest volume is inaccessible in mini mode"
    );

    let export = std::env::var_os("TIDE_JAM_LAYOUT_DIR").map(std::path::PathBuf::from);
    if let Some(path) = &export {
        std::fs::create_dir_all(path).unwrap();
    }
    for (name, width, height, light, mini, active, guest) in [
        ("host-dark", 1140, 800, false, false, true, false),
        ("guest-light", 780, 720, true, false, true, true),
        ("connect-minimum", 650, 420, false, false, false, false),
        ("guest-mini", 700, 166, false, true, true, true),
    ] {
        ui.set_light_mode(light);
        ui.set_collapsed(mini);
        ui.set_compact_mode(width == 780);
        ui.set_jam_active(active);
        ui.set_jam_guest(guest);
        ui.set_volume(50.0);
        ui.set_jam_status(
            if guest {
                "Following host · volume stays local"
            } else if active {
                "Hosting · 2 guests · private invite saved"
            } else {
                "Start a Jam or paste an invite"
            }
            .into(),
        );
        ui.set_jam_listener_id(if guest { 2 } else { 0 });
        ui.set_jam_locked(!guest && active);
        ui.set_jam_invite_code(
            if active && !guest {
                "tide-jam:v1.synthetic-preview"
            } else {
                ""
            }
            .into(),
        );
        let members: Vec<_> = if active && !guest {
            vec![
                JamMemberRow {
                    id: 1,
                    name: "Listener 1".into(),
                },
                JamMemberRow {
                    id: 2,
                    name: "Listener 2".into(),
                },
            ]
        } else {
            vec![]
        };
        ui.set_jam_members(slint::ModelRc::new(slint::VecModel::from(members)));
        ui.set_jam_open(!mini);
        let pixels = draw(&window, width, height);
        assert!(pixels.iter().any(|pixel| pixel != &Rgb8Pixel::default()));
        if let Some(path) = &export {
            let bytes: Vec<u8> = pixels
                .iter()
                .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
                .collect();
            image::RgbImage::from_raw(width, height, bytes)
                .unwrap()
                .save(path.join(format!("{name}.png")))
                .unwrap();
        }
    }
    // The Jam action must not push Account or Settings outside the minimum
    // supported window. Exercise the visible controls, not only the pixels.
    ui.set_collapsed(false);
    ui.set_jam_active(false);
    ui.set_jam_guest(false);
    ui.set_jam_open(false);
    draw(&window, 650, 420);
    click(&ui, 614.0, 32.0);
    assert!(ui.get_account_open(), "Account is clipped at minimum width");
    ui.set_account_open(false);
    click(&ui, 568.0, 32.0);
    assert!(
        ui.get_settings_open(),
        "Settings is clipped at minimum width"
    );
    ui.set_settings_open(false);
    click(&ui, 60.0, 198.0);
    assert!(ui.get_jam_open(), "Jam is inaccessible at minimum width");
    ui.set_jam_open(false);
    ui.set_jam_guest(true);
    ui.set_jam_open(false);
    let closed = draw(&window, 1140, 800);
    ui.set_jam_open(true);
    let opened = draw(&window, 1140, 800);
    assert_eq!(
        closed[250 * 1140 + 500],
        opened[250 * 1140 + 500],
        "Jam controls darkened the player background"
    );
    ui.set_jam_open(false);
    ui.set_source_input("".into());
    draw(&window, 1140, 800);
    click(&ui, 300.0, 32.0);
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "a".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "a".into() });
    assert_eq!(
        ui.get_source_input().as_str(),
        "a",
        "guest cannot use Search"
    );
    ui.set_jam_open(true);
    draw(&window, 780, 720);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert!(!ui.get_jam_open(), "Escape did not close Jam controls");
}
