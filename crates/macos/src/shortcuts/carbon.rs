use super::Chord;
use crate::MediaAction;
use std::{
    ffi::c_void,
    sync::{mpsc, Mutex},
};
#[repr(C)]
#[derive(Clone, Copy)]
struct HotKeyId {
    signature: u32,
    id: u32,
}
#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}
type Handler = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn RegisterEventHotKey(
        key: u32,
        modifiers: u32,
        id: HotKeyId,
        target: *mut c_void,
        options: u32,
        result: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(key: *mut c_void) -> i32;
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: u32,
        types: *const EventType,
        data: *mut c_void,
        result: *mut *mut c_void,
    ) -> i32;
    fn RemoveEventHandler(handler: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        kind: u32,
        actual_type: *mut u32,
        size: u32,
        actual_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}
const SIGNATURE: u32 = u32::from_be_bytes(*b"Tide");
struct State {
    bindings: Mutex<Vec<(u32, Chord, *mut c_void, MediaAction)>>,
    sender: mpsc::Sender<MediaAction>,
}
unsafe extern "C" fn handler(_next: *mut c_void, event: *mut c_void, data: *mut c_void) -> i32 {
    let mut id = HotKeyId {
        signature: 0,
        id: 0,
    };
    if GetEventParameter(
        event,
        u32::from_be_bytes(*b"----"),
        u32::from_be_bytes(*b"hkid"),
        std::ptr::null_mut(),
        8,
        std::ptr::null_mut(),
        (&mut id as *mut HotKeyId).cast(),
    ) != 0
        || id.signature != SIGNATURE
    {
        return -9874;
    }
    let state = &*(data as *const State);
    if let Ok(bindings) = state.bindings.lock() {
        if let Some((_, _, _, action)) = bindings.iter().find(|b| b.0 == id.id) {
            let _ = state.sender.send(*action);
            return 0;
        }
    }
    -9874
}
pub struct GlobalShortcuts {
    state: Box<State>,
    handler: *mut c_void,
}
impl GlobalShortcuts {
    pub fn new(callback: impl Fn(MediaAction) + Send + 'static) -> Result<Self, String> {
        if objc2::MainThreadMarker::new().is_none() {
            return Err("Global shortcuts require the main thread".into());
        }
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("tide-shortcuts".into())
            .spawn(move || {
                for action in rx {
                    let _ =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(action)));
                }
            })
            .map_err(|e| e.to_string())?;
        let mut state = Box::new(State {
            bindings: Mutex::new(Vec::new()),
            sender: tx,
        });
        let mut installed = std::ptr::null_mut();
        let event = EventType {
            class: u32::from_be_bytes(*b"keyb"),
            kind: 6,
        };
        let result = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                handler,
                1,
                &event,
                (&mut *state as *mut State).cast(),
                &mut installed,
            )
        };
        if result != 0 {
            return Err("Could not install ⌘/⌥ shortcuts".into());
        }
        Ok(Self {
            state,
            handler: installed,
        })
    }
    pub fn apply(&mut self, chords: [Option<Chord>; 3]) -> Result<(), String> {
        for (i, chord) in chords.iter().enumerate() {
            if chord.is_some() && chords[..i].contains(chord) {
                return Err("Use a different shortcut for each action.".into());
            }
        }
        let mut active = self
            .state
            .bindings
            .lock()
            .map_err(|_| "Shortcuts stopped")?;
        let mut next = Vec::new();
        let mut acquired = Vec::new();
        for (index, chord) in chords.into_iter().enumerate() {
            let Some(chord) = chord else {
                continue;
            };
            let (id, handle) = if let Some(b) = active.iter().find(|b| b.1 == chord) {
                (b.0, b.2)
            } else {
                let id = (1..=6)
                    .find(|id| {
                        !active.iter().any(|b| b.0 == *id)
                            && !next
                                .iter()
                                .any(|b: &(u32, Chord, *mut c_void, MediaAction)| b.0 == *id)
                    })
                    .unwrap();
                let mut handle = std::ptr::null_mut();
                if unsafe {
                    RegisterEventHotKey(
                        chord.key,
                        chord.modifiers,
                        HotKeyId {
                            signature: SIGNATURE,
                            id,
                        },
                        GetApplicationEventTarget(),
                        0,
                        &mut handle,
                    )
                } != 0
                {
                    for handle in acquired {
                        unsafe {
                            UnregisterEventHotKey(handle);
                        }
                    }
                    return Err("A ⌘/⌥ shortcut is already in use by macOS or another app. Previous shortcuts are still active; choose another combination.".into());
                }
                acquired.push(handle);
                (id, handle)
            };
            next.push((
                id,
                chord,
                handle,
                [
                    MediaAction::Toggle,
                    MediaAction::Next,
                    MediaAction::Previous,
                ][index],
            ));
        }
        for b in active.iter() {
            if !next.iter().any(|n| n.0 == b.0) {
                unsafe {
                    UnregisterEventHotKey(b.2);
                }
            }
        }
        *active = next;
        Ok(())
    }
}
impl Drop for GlobalShortcuts {
    fn drop(&mut self) {
        unsafe {
            RemoveEventHandler(self.handler);
        }
        if let Ok(active) = self.state.bindings.lock() {
            for b in active.iter() {
                unsafe {
                    UnregisterEventHotKey(b.2);
                }
            }
        }
    }
}
