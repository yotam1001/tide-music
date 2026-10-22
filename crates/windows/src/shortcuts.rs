//! Opt-in registered chords, never keyboard hooks or hardware media-key grabs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    modifiers: u32,
    key: u32,
}

/// Resolve shifted symbols and non-Latin letters using the focused thread's layout.
/// Only alphanumeric virtual keys accepted by the existing global parser are returned.
pub fn layout_key_name(character: char) -> Option<String> {
    #[cfg(windows)]
    {
        let unit = u16::try_from(character as u32).ok()?;
        let mapped = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::VkKeyScanW(unit) };
        if mapped == -1 {
            return None;
        }
        let key = (mapped as u16 & 0xff) as u8;
        return key
            .is_ascii_alphanumeric()
            .then(|| (key as char).to_string());
    }
    #[cfg(not(windows))]
    {
        let _ = character;
        None
    }
}

pub fn parse_bindings(values: &[String; 3]) -> Result<[Option<Chord>; 3], String> {
    let mut result = [None; 3];
    for (index, text) in values.iter().enumerate() {
        result[index] = parse(text)?;
        if result[index].is_some() && result[..index].contains(&result[index]) {
            return Err("Use a different shortcut for each action.".into());
        }
    }
    Ok(result)
}

fn parse(text: &str) -> Result<Option<Chord>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let invalid = || {
        "Use Ctrl or Alt plus a letter, digit, Space or F1–F24 (except reserved F12), for example Ctrl+Alt+P.".to_string()
    };
    if text.len() > 64 || !text.is_ascii() {
        return Err(invalid());
    }
    let mut modifiers = 0;
    let mut key = None;
    for part in text.split('+').map(|part| part.trim().to_ascii_uppercase()) {
        let modifier = match part.as_str() {
            "CTRL" | "CONTROL" => 2,
            "ALT" => 1,
            "SHIFT" => 4,
            _ => 0,
        };
        if modifier != 0 {
            if modifiers & modifier != 0 || key.is_some() {
                return Err(invalid());
            }
            modifiers |= modifier;
        } else {
            if key.is_some() {
                return Err(invalid());
            }
            key = Some(
                if part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric() {
                    part.as_bytes()[0] as u32
                } else if part == "SPACE" {
                    0x20
                } else if let Some(number) =
                    part.strip_prefix('F').and_then(|s| s.parse::<u32>().ok())
                {
                    if !(1..=24).contains(&number) || number == 12 {
                        return Err(invalid());
                    }
                    0x70 + number - 1
                } else {
                    return Err(invalid());
                },
            );
        }
    }
    if modifiers & 3 == 0 {
        return Err(invalid());
    }
    Ok(Some(Chord {
        modifiers,
        key: key.ok_or_else(invalid)?,
    }))
}

#[cfg(windows)]
mod registered {
    use super::Chord;
    use crate::MediaAction;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::thread::{self, JoinHandle};
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{
                RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
            },
            WindowsAndMessaging::{
                GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, PM_REMOVE, WM_APP,
                WM_HOTKEY, WM_QUIT,
            },
        },
    };

    const CONFIGURE: u32 = WM_APP + 71;
    const FIRST_ID: i32 = 0x4100;
    type Configuration = ([Option<Chord>; 3], mpsc::SyncSender<Result<(), String>>);
    type Binding = (i32, Chord, MediaAction);

    pub struct GlobalShortcuts {
        thread_id: u32,
        requests: mpsc::Sender<Configuration>,
        stopped: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl GlobalShortcuts {
        pub fn new(callback: impl Fn(MediaAction) + Send + 'static) -> Result<Self, String> {
            let (requests, receiver) = mpsc::channel::<Configuration>();
            let (ready, started) = mpsc::sync_channel(1);
            let stopped = Arc::new(AtomicBool::new(false));
            let stop = stopped.clone();
            let worker = thread::Builder::new()
                .name("tide-shortcuts".into())
                .spawn(move || {
                    // This thread owns its message queue and registrations; it creates
                    // no window, changes no foreground window and captures no typing.
                    let mut message = MSG::default();
                    unsafe {
                        let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                    }
                    let id = unsafe { GetCurrentThreadId() };
                    if ready.send(id).is_err() {
                        return;
                    }
                    let mut active = Vec::<Binding>::new();
                    while !stop.load(Ordering::Acquire) {
                        if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
                            break;
                        }
                        // Drain requests on any wake: even a full Windows message
                        // queue cannot strand an enqueued configuration operation.
                        let mut configured = false;
                        while let Ok((chords, reply)) = receiver.try_recv() {
                            configured = true;
                            let _ = reply.send(replace(&mut active, chords));
                        }
                        if !configured
                            && message.message == WM_HOTKEY
                            && !stop.load(Ordering::Acquire)
                        {
                            if let Some((_, _, action)) = active
                                .iter()
                                .find(|(id, _, _)| *id as usize == message.wParam.0)
                            {
                                let _ =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        callback(*action)
                                    }));
                            }
                        }
                    }
                    for (id, _, _) in active {
                        unsafe {
                            let _ = UnregisterHotKey(None, id);
                        }
                    }
                })
                .map_err(|_| "Could not start global shortcuts.".to_string())?;
            let thread_id = started
                .recv()
                .map_err(|_| "Global shortcuts did not start.".to_string())?;
            Ok(Self {
                thread_id,
                requests,
                stopped,
                worker: Some(worker),
            })
        }

        pub fn apply(&mut self, chords: [Option<Chord>; 3]) -> Result<(), String> {
            for (index, chord) in chords.iter().enumerate() {
                if chord.is_some() && chords[..index].contains(chord) {
                    return Err("Use a different shortcut for each action.".into());
                }
            }
            let (reply, response) = mpsc::sync_channel(1);
            self.requests
                .send((chords, reply))
                .map_err(|_| "Global shortcuts stopped.".to_string())?;
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, CONFIGURE, WPARAM(0), LPARAM(0));
            }
            response
                .recv()
                .map_err(|_| "Global shortcuts stopped.".to_string())?
        }
    }

    // Acquire only new chords before touching the old ones. A conflict leaves
    // every existing registration intact, including when the user swaps actions.
    fn replace(active: &mut Vec<Binding>, chords: [Option<Chord>; 3]) -> Result<(), String> {
        let actions = [
            MediaAction::Toggle,
            MediaAction::Next,
            MediaAction::Previous,
        ];
        let mut next = Vec::<Binding>::new();
        let mut acquired = Vec::new();
        for (index, chord) in chords.into_iter().enumerate() {
            let Some(chord) = chord else {
                continue;
            };
            let id = if let Some((id, _, _)) = active.iter().find(|(_, key, _)| *key == chord) {
                *id
            } else {
                let id = (FIRST_ID..FIRST_ID + 6)
                    .find(|candidate| {
                        !active.iter().any(|(id, _, _)| id == candidate)
                            && !acquired.contains(candidate)
                    })
                    .expect("at most six owned registrations");
                if unsafe {
                    RegisterHotKey(
                        None,
                        id,
                        HOT_KEY_MODIFIERS(chord.modifiers) | MOD_NOREPEAT,
                        chord.key,
                    )
                }
                .is_err()
                {
                    for id in acquired {
                        unsafe {
                            let _ = UnregisterHotKey(None, id);
                        }
                    }
                    return Err("A shortcut is already in use by Windows or another app. Previous shortcuts are still active; choose another combination.".into());
                }
                acquired.push(id);
                id
            };
            next.push((id, chord, actions[index]));
        }
        for (id, _, _) in active.iter() {
            if !next.iter().any(|(next, _, _)| next == id) {
                unsafe {
                    let _ = UnregisterHotKey(None, *id);
                }
            }
        }
        // Discard transport presses queued during an explicit reconfiguration.
        // They must not be interpreted under a newly swapped action mapping.
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }.as_bool()
        {
        }
        *active = next;
        Ok(())
    }

    impl Drop for GlobalShortcuts {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::Release);
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn owned_chords_dispatch_swap_reject_conflicts_and_unregister_on_drop() {
            let (tx, rx) = mpsc::sync_channel(8);
            let mut owner = GlobalShortcuts::new(move |action| {
                let _ = tx.try_send(action);
            })
            .unwrap();
            let chords = super::super::parse_bindings(&[
                "Ctrl+Alt+Shift+F24".into(),
                "Ctrl+Alt+Shift+F23".into(),
                "".into(),
            ])
            .unwrap();
            owner.apply(chords).unwrap();
            let press = |id| unsafe {
                PostThreadMessageW(owner.thread_id, WM_HOTKEY, WPARAM(id), LPARAM(0)).unwrap();
            };
            press(FIRST_ID as usize);
            assert_eq!(
                rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(),
                MediaAction::Toggle
            );
            let mut other = GlobalShortcuts::new(|_| {}).unwrap();
            assert!(other.apply(chords).is_err());
            let swapped = [chords[1], chords[0], None];
            owner.apply(swapped).unwrap();
            unsafe {
                PostThreadMessageW(
                    owner.thread_id,
                    WM_HOTKEY,
                    WPARAM(FIRST_ID as usize),
                    LPARAM(0),
                )
                .unwrap();
            }
            assert_eq!(
                rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(),
                MediaAction::Next
            );
            // A partial acquisition must roll back the newly registered chord
            // while preserving this owner's previous registrations.
            let private =
                super::super::parse_bindings(&["Ctrl+Alt+Shift+F22".into(), "".into(), "".into()])
                    .unwrap();
            other.apply(private).unwrap();
            let mut partial =
                super::super::parse_bindings(&["Ctrl+Alt+Shift+F21".into(), "".into(), "".into()])
                    .unwrap();
            partial[1] = chords[0];
            assert!(other.apply(partial).is_err());
            let mut probe = GlobalShortcuts::new(|_| {}).unwrap();
            assert!(
                probe.apply(private).is_err(),
                "previous chord remains owned after a conflict"
            );
            partial[1] = None;
            probe.apply(partial).unwrap();
            drop(probe);
            assert!(owner.apply([chords[0], chords[0], None]).is_err());
            let conflicting = [chords[0], None, None];
            assert!(other.apply(conflicting).is_err());
            drop(owner);
            other.apply(chords).unwrap();
            other.apply([None; 3]).unwrap();
        }
    }
}

#[cfg(windows)]
pub use registered::GlobalShortcuts;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_are_explicit_modified_chords_with_distinct_actions() {
        assert_eq!(
            parse(" ctrl + alt + p ").unwrap(),
            parse("ALT+CONTROL+P").unwrap()
        );
        assert!(parse(" ").unwrap().is_none());
        for text in [
            "P",
            "Space",
            "Shift+P",
            "Ctrl",
            "Ctrl++P",
            "Win+P",
            "Ctrl+F12",
            "Ctrl+F25",
            "Ctrl+Ctrl+P",
            "P+Ctrl",
            "Ctrl+MediaPlayPause",
        ] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
        for text in ["Ctrl+Alt+P", "Alt+Shift+Space", "Ctrl+F24", "Alt+9"] {
            assert!(parse(text).unwrap().is_some());
        }
        assert!(parse_bindings(&["Ctrl+Alt+P".into(), "Alt+Ctrl+P".into(), "".into()]).is_err());
    }
}
