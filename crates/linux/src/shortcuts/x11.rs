//! Root-window `XGrabKey` chords. Only the configured chords are grabbed;
//! no other key events ever reach Tide.
use super::{fire, Callback, Chord};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, GrabMode,
    ModMask, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

/// Caps Lock and Num Lock must not stop a chord from firing.
const LOCK_VARIANTS: [u16; 4] = [0, 2, 16, 2 | 16];
/// Shift, Control and Alt: the only modifiers a chord may use.
const CHORD_MASK: u16 = 1 | 4 | 8;

type Grab = (u8, u16, usize);

pub(super) struct GrabbedShortcuts {
    conn: Arc<RustConnection>,
    root: u32,
    wake: u32,
    grabs: Arc<Mutex<Vec<Grab>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GrabbedShortcuts {
    pub(super) fn new(callback: Callback) -> Result<Self, String> {
        let (conn, screen) = x11rb::connect(None)
            .map_err(|_| "Global shortcuts need an X11 or XWayland display.".to_string())?;
        let conn = Arc::new(conn);
        let root = conn.setup().roots[screen].root;
        let wake = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_window(
            0,
            wake,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        let grabs: Arc<Mutex<Vec<Grab>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (conn, grabs, stop) = (conn.clone(), grabs.clone(), stop.clone());
            std::thread::Builder::new()
                .name("tide-shortcuts".into())
                .spawn(move || loop {
                    let event = conn.wait_for_event();
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    match event {
                        Ok(Event::KeyPress(event)) => {
                            let mods = u16::from(event.state) & CHORD_MASK;
                            let slot = grabs.lock().ok().and_then(|grabs| {
                                grabs
                                    .iter()
                                    .find(|(code, mask, _)| *code == event.detail && *mask == mods)
                                    .map(|grab| grab.2)
                            });
                            if let Some(slot) = slot {
                                fire(&callback, slot);
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                })
                .map_err(|e| e.to_string())?
        };
        Ok(Self {
            conn,
            root,
            wake,
            grabs,
            stop,
            thread: Some(thread),
        })
    }

    fn keycode(&self, keysym: u32) -> Result<u8, String> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let mapping = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let per = usize::from(mapping.keysyms_per_keycode).max(1);
        mapping
            .keysyms
            .chunks(per)
            .position(|syms| syms.iter().take(2).any(|sym| *sym == keysym))
            .map(|index| min + index as u8)
            .ok_or_else(|| "That key is not on the current keyboard layout.".to_string())
    }

    fn ungrab(&self, grabs: &[Grab]) {
        for (code, mask, _) in grabs {
            for lock in LOCK_VARIANTS {
                let _ = self
                    .conn
                    .ungrab_key(*code, self.root, ModMask::from(mask | lock));
            }
        }
        let _ = self.conn.flush();
    }

    pub(super) fn apply(&mut self, chords: [Option<Chord>; 3]) -> Result<(), String> {
        let previous = std::mem::take(&mut *self.grabs.lock().map_err(|_| "Shortcut state")?);
        self.ungrab(&previous);
        let mut added: Vec<Grab> = Vec::new();
        for (slot, chord) in chords.iter().enumerate() {
            let Some(chord) = chord else {
                continue;
            };
            let grab = (self.keycode(chord.keysym())?, chord.x11_mask(), slot);
            for lock in LOCK_VARIANTS {
                let result = self
                    .conn
                    .grab_key(
                        true,
                        self.root,
                        ModMask::from(grab.1 | lock),
                        grab.0,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                    )
                    .map_err(|e| e.to_string())
                    .and_then(|cookie| cookie.check().map_err(|e| e.to_string()));
                if result.is_err() {
                    added.push(grab);
                    self.ungrab(&added);
                    return Err(
                        "Another app already uses one of these shortcuts. Choose a different one."
                            .into(),
                    );
                }
            }
            added.push(grab);
        }
        *self.grabs.lock().map_err(|_| "Shortcut state")? = added;
        Ok(())
    }
}

impl Drop for GrabbedShortcuts {
    fn drop(&mut self) {
        if let Ok(grabs) = self.grabs.lock().map(|grabs| grabs.clone()) {
            self.ungrab(&grabs);
        }
        self.stop.store(true, Ordering::Release);
        // A client message to our own InputOnly window wakes the event thread.
        let event = ClientMessageEvent::new(32, self.wake, AtomEnum::NONE, [0u32; 5]);
        let _ = self
            .conn
            .send_event(false, self.wake, EventMask::NO_EVENT, event);
        let _ = self.conn.destroy_window(self.wake);
        let _ = self.conn.flush();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
