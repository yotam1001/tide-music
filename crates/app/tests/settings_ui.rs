//! Synthetic Settings sheet: categories, switches, keycap shortcut capture, keyboard
//! focus and About links. No network,
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
fn key(ui: &AppWindow, text: slint::SharedString) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}
// The sheet is centred: at 1140x800 the rail starts at x=100 and the page
// content at x=312, both from y=50. The account card heads the rail. Rail
// entries have fixed heights, so they do not move with the platform font.
const RAIL_X: f32 = 200.0;
const SWITCH_X: f32 = 970.0;
fn rail_y(page: SettingsPage) -> f32 {
    match page {
        SettingsPage::Account => 157.0,
        SettingsPage::Playback => 226.0,
        SettingsPage::Audio => 268.0,
        SettingsPage::Appearance => 310.0,
        SettingsPage::Controls => 352.0,
        _ => 394.0,
    }
}
// Narrow windows (650x420): an 82px icon rail from x=8, entries 52px tall.
const NARROW_X: f32 = 47.0;
fn narrow_y(page: SettingsPage) -> f32 {
    match page {
        SettingsPage::Account => 45.0,
        SettingsPage::Playback => 108.0,
        SettingsPage::Audio => 162.0,
        SettingsPage::Appearance => 216.0,
        SettingsPage::Controls => 270.0,
        _ => 324.0,
    }
}
// Shortcut rows: the keycap field, then Clear / Cancel beside it. Each row
// is 56px tall; where the first starts follows the platform font.
const KEYS_X: f32 = 834.0;
const CLEAR_X: f32 = 966.0;
const SHORTCUT_ROW: f32 = 56.0;

#[test]
fn settings_sheet_navigates_toggles_and_captures_shortcuts() {
    use slint::platform::Key;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_app_version("1.2.3".into());
    ui.set_windows_theme_supported(true);
    ui.set_crash_log_available(true);
    ui.set_audio_device_names(slint::ModelRc::new(slint::VecModel::from(vec![
        "System default".into(),
        "Headphones".into(),
    ])));
    ui.set_audio_status("Applies only to Tide".into());
    let weak = ui.as_weak();
    ui.on_begin_shortcut(move |i| weak.upgrade().unwrap().set_shortcut_recording_index(i));
    let cancels = Rc::new(RefCell::new(0));
    let c = cancels.clone();
    let weak = ui.as_weak();
    ui.on_cancel_shortcut(move || {
        *c.borrow_mut() += 1;
        weak.upgrade().unwrap().set_shortcut_recording_index(-1)
    });
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
    let links = Rc::new(RefCell::new(Vec::new()));
    let c = links.clone();
    ui.on_open_about_link(move |i| c.borrow_mut().push(i));
    let crash_log = Rc::new(RefCell::new(0));
    let c = crash_log.clone();
    ui.on_open_crash_log(move || *c.borrow_mut() += 1);
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

        // Row heights follow the platform font (macOS differs from Windows),
        // so locate the Listening history switch by its effect first.
        let mut history_y = None;
        for y in (360..540).step_by(4) {
            let before = *switches.borrow();
            click(&ui, SWITCH_X, y as f32);
            if *switches.borrow() > before {
                history_y = Some(y as f32);
                break;
            }
        }
        let history_y = history_y.expect("Listening history switch not found");
        ui.set_history_feedback_enabled(false);
        let before = *switches.borrow();
        click(&ui, SWITCH_X, history_y);
        assert_eq!(
            *switches.borrow(),
            before + 1,
            "Switch target did not activate exactly once"
        );
        key(&ui, " ".into());
        assert_eq!(
            *switches.borrow(),
            before + 2,
            "Visual switch lost native Space-key support"
        );
        // The label is part of the row's target, not just the switch.
        click(&ui, 500.0, history_y);
        assert_eq!(*switches.borrow(), before + 3);
        ui.set_history_feedback_enabled(false);

        // Shortcuts sit inline on the Controls page, as keycaps.
        click(&ui, RAIL_X, rail_y(SettingsPage::Controls));
        // Find the first keycap field by its effect, then aim at its centre
        // (the field is 38px tall).
        let top = (130..280)
            .step_by(2)
            .find(|&y| {
                click(&ui, KEYS_X, y as f32);
                ui.get_shortcut_recording_index() == 0
            })
            .expect("Play / pause shortcut field not found") as f32;
        let shortcut_y = [0.0, 1.0, 2.0].map(|row| top + 18.0 + row * SHORTCUT_ROW);
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
        click(&ui, KEYS_X, shortcut_y[1]);
        assert_eq!(ui.get_shortcut_recording_index(), 1);
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        assert!(ui.get_settings_open());
        assert_eq!(ui.get_shortcut_toggle(), "Ctrl+Alt+P");
        draw(
            &window,
            1140,
            800,
            &format!("settings-shortcut-set-{theme}"),
        );
        // A four-key chord widens its field instead of overflowing it.
        ui.set_shortcut_previous("Ctrl+Alt+Shift+Space".into());
        draw(
            &window,
            1140,
            800,
            &format!("settings-shortcut-chord-{theme}"),
        );
        ui.set_shortcut_previous("".into());
        // While recording, Cancel takes the place of Clear.
        click(&ui, KEYS_X, shortcut_y[1]);
        assert_eq!(ui.get_shortcut_recording_index(), 1);
        draw(
            &window,
            1140,
            800,
            &format!("settings-recording-cancel-{theme}"),
        );
        let before = *cancels.borrow();
        click(&ui, CLEAR_X, shortcut_y[1]);
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        assert!(*cancels.borrow() > before, "Cancel did not cancel");
        assert!(cleared.borrow().is_empty(), "Cancel cleared a shortcut");
        // An unset shortcut has nothing to clear.
        click(&ui, CLEAR_X, shortcut_y[2]);
        assert!(cleared.borrow().is_empty());
        click(&ui, CLEAR_X, shortcut_y[0]);
        assert_eq!(cleared.borrow().last(), Some(&0));
        assert!(ui.get_shortcut_toggle().is_empty());
        cleared.borrow_mut().clear();
        click(&ui, KEYS_X, shortcut_y[2]);
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
        click(&ui, KEYS_X, shortcut_y[2]);
        assert_eq!(ui.get_shortcut_recording_index(), 2);
        click(&ui, RAIL_X, rail_y(SettingsPage::Audio));
        assert_eq!(ui.get_shortcut_recording_index(), -1);
        // Closing from the scrim cancels too.
        click(&ui, RAIL_X, rail_y(SettingsPage::Controls));
        click(&ui, KEYS_X, shortcut_y[2]);
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
    click(&ui, 462.0, 250.0);
    assert!(
        !ui.get_light_mode() && !ui.get_follow_windows_theme(),
        "Dark card"
    );
    click(&ui, 889.0, 250.0);
    assert!(ui.get_follow_windows_theme(), "System card");
    click(&ui, 675.0, 250.0);
    assert!(
        ui.get_light_mode() && !ui.get_follow_windows_theme(),
        "Light card"
    );
    ui.set_light_mode(false);

    // About: each link row opens its own page; the crash log has its own action.
    ui.set_settings_page(SettingsPage::About);
    draw(&window, 1140, 800, "settings-about-links");
    for scroll in [0.0, -240.0] {
        ui.set_settings_scroll_y(scroll);
        draw(&window, 1140, 800, "settings-about-scrolled");
        for y in (380..740).step_by(6) {
            click(&ui, 700.0, y as f32);
        }
    }
    let mut seen = Vec::new();
    for link in links.borrow().iter() {
        if !seen.contains(link) {
            seen.push(*link);
        }
    }
    assert_eq!(seen, [0, 1, 2, 3, 4], "About links in order");
    assert!(*crash_log.borrow() > 0, "Crash log row");
    assert!(ui.get_settings_open(), "A link closed Settings");
    // The rail footer shows the version and update state, and opens About.
    ui.set_settings_page(SettingsPage::Playback);
    ui.set_update_current(true);
    draw(&window, 1140, 800, "settings-footer-current");
    click(&ui, 200.0, 712.0);
    assert_eq!(ui.get_settings_page(), SettingsPage::About, "Rail footer");
    ui.set_update_current(false);
    ui.set_update_available(true);
    draw(&window, 1140, 800, "settings-update-available");
    ui.set_update_available(false);

    // Keyboard: Tab starts in the rail and Up/Down switch categories.
    ui.set_settings_open(false);
    draw(&window, 1140, 800, "settings-keyboard-closed");
    ui.set_settings_page(SettingsPage::Audio);
    ui.set_settings_open(true);
    draw(&window, 1140, 800, "settings-keyboard-open");
    key(&ui, Key::Tab.into());
    key(&ui, Key::DownArrow.into());
    assert_eq!(
        ui.get_settings_page(),
        SettingsPage::Playback,
        "Tab did not focus the first rail entry (Account)"
    );
    key(&ui, Key::DownArrow.into());
    key(&ui, Key::DownArrow.into());
    assert_eq!(ui.get_settings_page(), SettingsPage::Appearance);
    key(&ui, Key::UpArrow.into());
    assert_eq!(ui.get_settings_page(), SettingsPage::Audio);
    draw(&window, 1140, 800, "settings-keyboard-rail");
    for _ in 0..3 {
        key(&ui, Key::UpArrow.into());
    }
    assert!(ui.get_account_open(), "Up arrow did not reach Account");
    draw(&window, 1140, 800, "settings-keyboard-account");
    key(&ui, Key::DownArrow.into());
    draw(&window, 1140, 800, "settings-keyboard-playback");
    assert!(ui.get_settings_open() && !ui.get_account_open());
    // Focus is trapped: Shift+Tab from the first entry wraps to the close
    // button, and Tab from there wraps back to the rail.
    key(&ui, Key::UpArrow.into());
    draw(&window, 1140, 800, "settings-keyboard-account");
    key(&ui, Key::Backtab.into());
    key(&ui, Key::Tab.into());
    key(&ui, Key::DownArrow.into());
    draw(&window, 1140, 800, "settings-keyboard-wrapped");
    assert!(
        ui.get_settings_open() && ui.get_settings_page() == SettingsPage::Playback,
        "Tab past the close button left the sheet"
    );
    // Tab on through the rail to its footer: Up from there returns to the
    // last category, whichever page is on show.
    for _ in 0..5 {
        key(&ui, Key::Tab.into());
    }
    ui.set_settings_page(SettingsPage::Audio);
    draw(&window, 1140, 800, "settings-keyboard-footer");
    key(&ui, Key::UpArrow.into());
    assert_eq!(
        ui.get_settings_page(),
        SettingsPage::About,
        "Up from the rail footer"
    );
    // Many Tabs never reach the window behind.
    for _ in 0..60 {
        key(&ui, Key::Tab.into());
    }
    key(&ui, Key::Escape.into());
    draw(&window, 1140, 800, "settings-keyboard-tabbed");
    assert!(!ui.get_settings_open(), "Escape after tabbing");
    // From a freshly opened sheet, Shift+Tab reaches the close button.
    ui.set_settings_open(true);
    draw(&window, 1140, 800, "settings-keyboard-reopened");
    key(&ui, Key::Backtab.into());
    key(&ui, " ".into());
    assert!(!ui.get_settings_open(), "Shift+Tab did not reach Close");
    draw(&window, 1140, 800, "settings-keyboard-closed");

    // A connected account lists what the connection brings, from data the
    // player already holds; Listening history leads to its setting.
    ui.set_account_connected(true);
    ui.set_account_verified(true);
    ui.set_account_name("Sample listener".into());
    ui.set_history_feedback_enabled(true);
    for (light, theme) in [(false, "dark"), (true, "light")] {
        ui.set_light_mode(light);
        ui.set_account_open(true);
        draw(
            &window,
            1140,
            800,
            &format!("settings-account-connected-{theme}"),
        );
    }
    ui.set_light_mode(false);
    // Rows above it leave Settings, so scan only where the last row can be.
    // Scan bottom-up: below that row there is nothing to click at this x.
    let mut opened = false;
    for y in (440..560).step_by(4).rev() {
        click(&ui, 700.0, y as f32);
        if ui.get_settings_open() {
            opened = true;
            break;
        }
    }
    assert!(opened, "Listening history row did not open Playback");
    assert_eq!(ui.get_settings_page(), SettingsPage::Playback);
    // Your playlists closes the sheet and opens the library behind it.
    ui.set_account_open(true);
    draw(&window, 1140, 800, "settings-account-connected-again");
    for y in (260..380).step_by(4) {
        click(&ui, 700.0, y as f32);
        if !ui.get_account_open() {
            break;
        }
    }
    assert!(
        !ui.get_settings_open() && !ui.get_account_open() && ui.get_library_open(),
        "Your playlists row did not open the library"
    );
    ui.set_library_open(false);
    ui.set_home_open(true);
    ui.set_account_connected(false);
    ui.set_account_verified(false);
    ui.set_account_name("".into());
    ui.set_settings_open(false);
    draw(&window, 1140, 800, "settings-account-closed");

    // Narrow windows keep a compact icon rail; all six entries fit at the
    // minimum size without scrolling.
    ui.set_settings_page(SettingsPage::Audio);
    ui.set_settings_open(true);
    draw(&window, 650, 420, "settings-small-window");
    for (name, page) in [
        ("account", SettingsPage::Account),
        ("playback", SettingsPage::Playback),
        ("audio", SettingsPage::Audio),
        ("appearance", SettingsPage::Appearance),
        ("controls", SettingsPage::Controls),
        ("about", SettingsPage::About),
    ] {
        click(&ui, NARROW_X, narrow_y(page));
        assert_eq!(
            ui.get_settings_shown_page(),
            page,
            "narrow rail did not open {name}"
        );
        draw(&window, 650, 420, &format!("settings-small-{name}"));
    }
    click(&ui, NARROW_X, narrow_y(SettingsPage::Playback));
    assert!(ui.get_settings_open());
    assert_eq!(ui.get_settings_page(), SettingsPage::Playback);
    // The compact footer also opens About.
    click(&ui, NARROW_X, 395.0);
    assert_eq!(
        ui.get_settings_page(),
        SettingsPage::About,
        "compact footer"
    );
    ui.set_settings_page(SettingsPage::Playback);
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
    // Jam guests can't reach Account from Settings any more than from the header.
    ui.set_jam_guest(true);
    draw(&window, 650, 420, "settings-small-guest");
    click(&ui, NARROW_X, narrow_y(SettingsPage::Account));
    assert!(
        !ui.get_account_open(),
        "Jam guest opened Account from Settings"
    );
    ui.set_jam_guest(false);
    ui.set_settings_page(SettingsPage::Playback);
    ui.set_settings_open(true);
    // Collapsing to the mini player closes Settings and any shortcut capture.
    ui.set_settings_page(SettingsPage::Controls);
    draw(&window, 650, 420, "settings-before-collapse");
    ui.invoke_begin_shortcut(0);
    assert_eq!(ui.get_shortcut_recording_index(), 0);
    ui.set_collapsed(true);
    draw(&window, 650, 132, "settings-collapsed");
    assert!(!ui.get_settings_open() && !ui.get_account_open());
    assert_eq!(ui.get_shortcut_recording_index(), -1);
    ui.set_collapsed(false);
    ui.set_settings_open(true);
    draw(&window, 650, 420, "settings-after-collapse");
    ui.set_settings_scroll_y(-180.0);
    ui.set_settings_page(SettingsPage::Audio);
    draw(&window, 650, 420, "settings-audio-small");
    ui.set_settings_scroll_y(-180.0);
    ui.set_shortcut_toggle("Ctrl+Alt+Shift+Space".into());
    ui.set_settings_page(SettingsPage::Controls);
    draw(&window, 650, 420, "settings-controls-small-window");
    assert_eq!(
        ui.get_settings_scroll_y(),
        0.0,
        "A page inherited the previous scroll position"
    );
    // Mid-size windows keep the full rail.
    for (light, theme) in [(false, "dark"), (true, "light")] {
        ui.set_light_mode(light);
        ui.set_settings_page(SettingsPage::Audio);
        ui.set_settings_scroll_y(0.0);
        draw(&window, 780, 720, &format!("settings-audio-780-{theme}"));
        ui.set_settings_scroll_y(-160.0);
        draw(
            &window,
            780,
            720,
            &format!("settings-audio-780-scrolled-{theme}"),
        );
    }
}
