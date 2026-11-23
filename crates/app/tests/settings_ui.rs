//! Synthetic Settings sheet: categories, switches and shortcut capture. No network,
//! account or installer is used.
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
// The sheet is centred: at 1140x800 the rail starts at x=100 and the page
// content at x=312, both from y=50. The account card heads the rail.
const RAIL_X: f32 = 200.0;
const SWITCH_X: f32 = 970.0;
fn rail_y(page: SettingsPage) -> f32 {
    match page {
        SettingsPage::Account => 151.0,
        SettingsPage::Playback => 220.0,
        SettingsPage::Audio => 264.0,
        SettingsPage::Appearance => 308.0,
        SettingsPage::Controls => 352.0,
        _ => 396.0,
    }
}

#[test]
fn settings_sheet_navigates_toggles_and_captures_shortcuts() {
    use slint::platform::Key;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_app_version("0.4.34".into());
    ui.set_windows_theme_supported(true);
    ui.set_crash_log_available(true);
    ui.set_audio_device_names(slint::ModelRc::new(slint::VecModel::from(vec![
        "System default".into(),
        "Headphones".into(),
    ])));
    ui.set_audio_status("Applies only to Tide".into());
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
    let switches = Rc::new(RefCell::new(0));
    let c = switches.clone();
    let weak = ui.as_weak();
    ui.on_toggle_history_feedback(move || {
        *c.borrow_mut() += 1;
        let u = weak.upgrade().unwrap();
        u.set_history_feedback_enabled(!u.get_history_feedback_enabled());
    });
    // Theme cards drive the existing theme callbacks to the chosen state.
    let weak = ui.as_weak();
    ui.on_switch_mode(move || {
        let u = weak.upgrade().unwrap();
        u.set_follow_windows_theme(false);
        u.set_light_mode(!u.get_light_mode());
    });
    let weak = ui.as_weak();
    ui.on_toggle_follow_windows_theme(move || {
        let u = weak.upgrade().unwrap();
        u.set_follow_windows_theme(!u.get_follow_windows_theme());
    });
    ui.show().unwrap();
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    for light in [false, true] {
        ui.set_light_mode(light);
        let theme = if light { "light" } else { "dark" };
        ui.set_settings_page(SettingsPage::Playback);
        ui.set_settings_open(true);
        for (name, page) in [
            ("playback", SettingsPage::Playback),
            ("audio", SettingsPage::Audio),
            ("appearance", SettingsPage::Appearance),
            ("controls", SettingsPage::Controls),
            ("about", SettingsPage::About),
        ] {
            click(&ui, RAIL_X, rail_y(page));
            assert_eq!(ui.get_settings_page(), page, "rail did not open {name}");
            assert!(ui.get_settings_open() && !ui.get_account_open());
            draw(&window, 1140, 800, &format!("settings-{name}-{theme}"));
        }
        // Account shares the sheet: the rail switches between them.
        click(&ui, RAIL_X, rail_y(SettingsPage::Account));
        assert!(ui.get_account_open() && !ui.get_settings_open());
        assert_eq!(ui.get_settings_shown_page(), SettingsPage::Account);
        draw(&window, 1140, 800, &format!("settings-account-{theme}"));
        click(&ui, RAIL_X, rail_y(SettingsPage::Playback));
        assert!(ui.get_settings_open() && !ui.get_account_open());

        let before = *switches.borrow();
        click(&ui, SWITCH_X, 415.0);
        assert_eq!(
            *switches.borrow(),
            before + 1,
            "Switch target did not activate exactly once"
        );
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
        assert_eq!(
            *switches.borrow(),
            before + 2,
            "Visual switch lost native Space-key support"
        );
        // The label is part of the row's target, not just the switch.
        click(&ui, 500.0, 415.0);
        assert_eq!(*switches.borrow(), before + 3);
        ui.set_history_feedback_enabled(false);

        // Shortcuts sit inline on the Controls page.
        click(&ui, RAIL_X, rail_y(SettingsPage::Controls));
        click(&ui, 880.0, 188.0);
        assert_eq!(ui.get_shortcut_recording_index(), 0);
        draw(&window, 1140, 800, &format!("settings-recording-{theme}"));
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
        click(&ui, 880.0, 242.0);
        assert_eq!(ui.get_shortcut_recording_index(), 1);
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        assert!(ui.get_settings_open());
        assert_eq!(ui.get_shortcut_toggle(), "Ctrl+Alt+P");
        draw(&window, 1140, 800, &format!("settings-shortcut-set-{theme}"));
        click(&ui, 977.0, 188.0);
        assert_eq!(cleared.borrow().last(), Some(&0));
        assert!(ui.get_shortcut_toggle().is_empty());
        click(&ui, 880.0, 296.0);
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
        // Leaving the Controls page cancels a capture in progress.
        click(&ui, 880.0, 296.0);
        assert_eq!(ui.get_shortcut_recording_index(), 2);
        click(&ui, RAIL_X, rail_y(SettingsPage::Audio));
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        // Closing from the scrim cancels too.
        click(&ui, RAIL_X, rail_y(SettingsPage::Controls));
        click(&ui, 880.0, 296.0);
        click(&ui, 40.0, 400.0);
        assert!(!ui.get_settings_open());
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        draw(&window, 1140, 800, &format!("settings-closed-{theme}"));
    }
    // Theme cards: Dark, the system, then Light.
    ui.set_light_mode(true);
    ui.set_settings_page(SettingsPage::Appearance);
    ui.set_settings_open(true);
    draw(&window, 1140, 800, "settings-theme-before");
    click(&ui, 462.0, 260.0);
    assert!(!ui.get_light_mode() && !ui.get_follow_windows_theme(), "Dark card");
    click(&ui, 889.0, 260.0);
    assert!(ui.get_follow_windows_theme(), "System card");
    click(&ui, 675.0, 260.0);
    assert!(ui.get_light_mode() && !ui.get_follow_windows_theme(), "Light card");
    ui.set_light_mode(false);
    // Narrow windows replace the rail with a tab strip.
    ui.set_settings_page(SettingsPage::Audio);
    draw(&window, 650, 420, "settings-small-window");
    click(&ui, 70.0, 38.0);
    assert!(ui.get_account_open(), "Account tab at minimum width");
    draw(&window, 650, 420, "settings-small-account");
    click(&ui, 186.0, 38.0);
    assert!(ui.get_settings_open());
    assert_eq!(ui.get_settings_page(), SettingsPage::Playback);
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
    ui.set_settings_page(SettingsPage::Controls);
    draw(&window, 650, 420, "settings-controls-small-window");
    assert_eq!(
        ui.get_settings_scroll_y(),
        0.0,
        "A page inherited the previous scroll position"
    );
}
