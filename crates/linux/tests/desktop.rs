//! Live desktop integration checks. They need a D-Bus session bus and an X11
//! display, so they are ignored by default. Run them with:
//!   xvfb-run -a dbus-run-session -- cargo test -p tide-linux --test desktop -- --ignored --test-threads=1
#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;
use tide_linux::MediaAction;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

fn player(conn: &Connection) -> Proxy<'_> {
    Proxy::new(
        conn,
        "org.mpris.MediaPlayer2.tide",
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .expect("MPRIS player proxy")
}

fn wait_for<T>(receiver: &mpsc::Receiver<T>) -> T {
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("callback within 5 s")
}

#[test]
#[ignore = "needs a D-Bus session bus"]
fn mpris_publishes_metadata_and_routes_controls() {
    let (send, receive) = mpsc::channel();
    let mut session = tide_linux::start(std::ptr::null_mut(), move |action| {
        let _ = send.send(action);
    })
    .expect("MPRIS session starts");
    session
        .update(
            "Tide Test Song",
            "Test Artist",
            "Test Album",
            true,
            12.0,
            180.0,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));

    let conn = Connection::session().unwrap();
    let player = player(&conn);
    let status: String = player.get_property("PlaybackStatus").unwrap();
    assert_eq!(status, "Playing");
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap();
    assert_eq!(
        String::try_from(metadata["xesam:title"].clone()).unwrap(),
        "Tide Test Song"
    );
    assert_eq!(
        i64::try_from(metadata["mpris:length"].clone()).unwrap(),
        180_000_000
    );
    let position: i64 = player.get_property("Position").unwrap();
    assert!((12_000_000..20_000_000).contains(&position), "{position}");

    for (method, expected) in [
        ("PlayPause", MediaAction::Toggle),
        ("Next", MediaAction::Next),
        ("Previous", MediaAction::Previous),
        ("Pause", MediaAction::Pause),
        ("Play", MediaAction::Play),
    ] {
        player.call_method(method, &()).unwrap();
        assert_eq!(wait_for(&receive), expected, "{method}");
    }
    let track: zbus::zvariant::OwnedObjectPath =
        metadata["mpris:trackid"].clone().try_into().unwrap();
    player
        .call_method("SetPosition", &(track.as_ref(), 60_000_000i64))
        .unwrap();
    assert_eq!(wait_for(&receive), MediaAction::Seek(60.0));

    session
        .update(
            "Tide Test Song",
            "Test Artist",
            "Test Album",
            false,
            60.0,
            180.0,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let status: String = player.get_property("PlaybackStatus").unwrap();
    assert_eq!(status, "Paused");
    drop(session);
    std::thread::sleep(Duration::from_millis(300));
    // Dropping the session releases the bus name.
    let names = zbus::blocking::fdo::DBusProxy::new(&conn)
        .unwrap()
        .list_names()
        .unwrap();
    assert!(!names
        .iter()
        .any(|name| name.as_str() == "org.mpris.MediaPlayer2.tide"));
}

/// A plain, mapped X11 window standing in for Tide's window.
fn test_window() -> (x11rb::rust_connection::RustConnection, u32) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    let (conn, screen) = x11rb::connect(None).expect("X11 display");
    let root = conn.setup().roots[screen].root;
    let window = conn.generate_id().unwrap();
    conn.create_window(
        0,
        window,
        root,
        10,
        10,
        320,
        96,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new(),
    )
    .unwrap();
    conn.map_window(window).unwrap();
    conn.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    (conn, window)
}

fn cardinal(conn: &x11rb::rust_connection::RustConnection, window: u32, name: &str) -> Vec<u32> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
    let atom = conn
        .intern_atom(false, name.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    conn.get_property(false, window, atom, AtomEnum::ANY, 0, 64)
        .unwrap()
        .reply()
        .unwrap()
        .value32()
        .map(|values| values.collect())
        .unwrap_or_default()
}

#[test]
#[ignore = "needs an X11 display"]
fn mini_window_opacity_is_applied_and_restored() {
    let (conn, window) = test_window();
    let view = window as usize as *mut std::ffi::c_void;
    let mut mini = tide_linux::mini::MiniWindow::default();
    mini.apply(view, true, 200).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let opacity = cardinal(&conn, window, "_NET_WM_WINDOW_OPACITY");
    assert_eq!(opacity, vec![(200u64 * u32::MAX as u64 / 255) as u32]);
    // Opacity never drops below 60 %, as on Windows.
    mini.apply(view, true, 10).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let opacity = cardinal(&conn, window, "_NET_WM_WINDOW_OPACITY");
    assert_eq!(opacity, vec![(153u64 * u32::MAX as u64 / 255) as u32]);
    mini.apply(view, false, 255).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(cardinal(&conn, window, "_NET_WM_WINDOW_OPACITY").is_empty());
    // The work area is a non-empty rectangle on the window's monitor.
    if let Ok([left, top, right, bottom]) = tide_linux::window_work_area(view) {
        assert!(right > left && bottom > top);
    }
}

/// Presses real keys through XTEST, like a physical keyboard. (xdotool remaps
/// keys onto spare keycodes, which never matches a registered grab.)
struct Keyboard {
    conn: x11rb::rust_connection::RustConnection,
    root: u32,
    min: u8,
    per: usize,
    keysyms: Vec<u32>,
}

impl Keyboard {
    fn new() -> Self {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as _;
        let (conn, screen) = x11rb::connect(None).expect("X11 display");
        let root = conn.setup().roots[screen].root;
        let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)
            .unwrap()
            .reply()
            .unwrap();
        Self {
            per: usize::from(mapping.keysyms_per_keycode),
            keysyms: mapping.keysyms,
            conn,
            root,
            min,
        }
    }

    fn code(&self, keysym: u32) -> u8 {
        let index = self
            .keysyms
            .chunks(self.per)
            .position(|syms| syms.first() == Some(&keysym))
            .unwrap_or_else(|| panic!("keysym {keysym:#x} is on the keyboard"));
        self.min + index as u8
    }

    /// Without a window manager (Xvfb) the focus starts as None, and the
    /// server then discards key events. Desktops always have a focus.
    fn focus_pointer_root(&self) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus};
        self.conn
            .set_input_focus(
                InputFocus::POINTER_ROOT,
                u32::from(InputFocus::POINTER_ROOT),
                x11rb::CURRENT_TIME,
            )
            .unwrap();
        self.conn.flush().unwrap();
    }

    /// Press `keys` in order, then release them in reverse.
    fn chord(&self, keysyms: &[u32]) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xtest::ConnectionExt as _;
        use x11rb::wrapper::ConnectionExt as _;
        let codes: Vec<u8> = keysyms.iter().map(|sym| self.code(*sym)).collect();
        for (kind, order) in [
            (2u8, codes.clone()),
            (3u8, codes.into_iter().rev().collect()),
        ] {
            for code in order {
                self.conn
                    .xtest_fake_input(kind, code, 0, self.root, 0, 0, 0)
                    .unwrap();
            }
        }
        self.conn.flush().unwrap();
        self.conn.sync().unwrap();
    }
}

const CONTROL_L: u32 = 0xffe3;
const ALT_L: u32 = 0xffe9;
const CAPS_LOCK: u32 = 0xffe5;
const KEY_P: u32 = 0x70;
const KEY_N: u32 = 0x6e;

#[test]
#[ignore = "needs an X11 display with XTEST"]
fn registered_chords_fire_and_unregister() {
    // Use the X11 grab path even inside a Wayland session's XWayland.
    std::env::remove_var("WAYLAND_DISPLAY");
    let (send, receive) = mpsc::channel();
    let mut shortcuts = tide_linux::shortcuts::GlobalShortcuts::new(move |action| {
        let _ = send.send(action);
    })
    .expect("shortcut manager");
    let bindings = tide_linux::shortcuts::parse_bindings(&[
        "Ctrl+Alt+P".into(),
        "Ctrl+Alt+N".into(),
        "".into(),
    ])
    .unwrap();
    shortcuts.apply(bindings).unwrap();
    let keyboard = Keyboard::new();
    keyboard.focus_pointer_root();
    keyboard.chord(&[CONTROL_L, ALT_L, KEY_P]);
    assert_eq!(wait_for(&receive), MediaAction::Toggle);
    keyboard.chord(&[CONTROL_L, ALT_L, KEY_N]);
    assert_eq!(wait_for(&receive), MediaAction::Next);
    // A different modifier set is not the registered chord.
    keyboard.chord(&[CONTROL_L, KEY_P]);
    assert!(receive.recv_timeout(Duration::from_millis(500)).is_err());
    // Caps Lock on must not stop a chord.
    keyboard.chord(&[CAPS_LOCK]);
    keyboard.chord(&[CONTROL_L, ALT_L, KEY_P]);
    keyboard.chord(&[CAPS_LOCK]);
    assert_eq!(wait_for(&receive), MediaAction::Toggle);
    // Clearing the bindings releases the grabs.
    shortcuts.apply([None, None, None]).unwrap();
    keyboard.chord(&[CONTROL_L, ALT_L, KEY_P]);
    assert!(receive.recv_timeout(Duration::from_millis(700)).is_err());
}
