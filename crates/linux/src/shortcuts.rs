//! Opt-in registered chords, never keyboard hooks or hardware media-key grabs.
//!
//! The text format and rules match Windows exactly (Ctrl or Alt, optional
//! Shift, plus a letter, digit, Space or F1–F24 except F12), so a settings file
//! moves between the two systems unchanged.
//!
//! On Wayland desktops chords go through the XDG GlobalShortcuts portal, where
//! the desktop asks the user to confirm them once. On X11 they are registered
//! with `XGrabKey` on the root window.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    /// 1 = Alt, 2 = Ctrl, 4 = Shift (the Windows `MOD_*` values).
    modifiers: u32,
    /// Windows virtual-key code: `A`–`Z`, `0`–`9`, Space, F1–F24.
    key: u32,
}

impl Chord {
    /// X11 keysym of the key: lower-case Latin letters, digits, space, F-keys.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn keysym(&self) -> u32 {
        match self.key {
            0x41..=0x5a => self.key + 0x20,
            0x70..=0x87 => 0xffbe + (self.key - 0x70),
            key => key,
        }
    }

    /// X11 modifier mask: ShiftMask 1, ControlMask 4, Mod1Mask (Alt) 8.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn x11_mask(&self) -> u16 {
        let mut mask = 0;
        if self.modifiers & 4 != 0 {
            mask |= 1;
        }
        if self.modifiers & 2 != 0 {
            mask |= 4;
        }
        if self.modifiers & 1 != 0 {
            mask |= 8;
        }
        mask
    }

    /// XDG shortcut trigger, for example `CTRL+ALT+p` or `CTRL+F5`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn portal_trigger(&self) -> String {
        let mut text = String::new();
        if self.modifiers & 2 != 0 {
            text.push_str("CTRL+");
        }
        if self.modifiers & 1 != 0 {
            text.push_str("ALT+");
        }
        if self.modifiers & 4 != 0 {
            text.push_str("SHIFT+");
        }
        match self.key {
            0x20 => text.push_str("space"),
            0x70..=0x87 => text.push_str(&format!("F{}", self.key - 0x70 + 1)),
            key => text.push((key as u8 as char).to_ascii_lowercase()),
        }
        text
    }
}

/// Return the portable label for supported alphanumeric keys.
pub fn layout_key_name(character: char) -> Option<String> {
    character
        .is_ascii_alphanumeric()
        .then(|| character.to_ascii_uppercase().to_string())
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
    let key = key.ok_or_else(invalid)?;
    // Ctrl+Alt+F1–F12 switch virtual consoles on Linux: the X server and
    // Wayland compositors act on them before any app sees the key.
    if modifiers & 3 == 3 && (0x70..0x7c).contains(&key) {
        return Err(
            "Ctrl+Alt+F1–F12 switch consoles on Linux. Choose a letter, number, Space or another key."
                .into(),
        );
    }
    Ok(Some(Chord { modifiers, key }))
}

#[cfg(target_os = "linux")]
type Callback = std::sync::Arc<std::sync::Mutex<dyn Fn(crate::MediaAction) + Send>>;

/// Run the app callback for one settings slot, isolating a panicking handler.
#[cfg(target_os = "linux")]
fn fire(callback: &Callback, slot: usize) {
    let Some(action) = ACTIONS.get(slot).copied() else {
        return;
    };
    if let Ok(callback) = callback.lock() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(action)));
    }
}

#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod x11;

#[cfg(target_os = "linux")]
enum Backend {
    Portal(portal::PortalShortcuts),
    X11(x11::GrabbedShortcuts),
}

#[cfg(target_os = "linux")]
pub struct GlobalShortcuts {
    backend: Backend,
}

#[cfg(target_os = "linux")]
impl GlobalShortcuts {
    pub fn new(callback: impl Fn(crate::MediaAction) + Send + 'static) -> Result<Self, String> {
        let callback: Callback = std::sync::Arc::new(std::sync::Mutex::new(callback));
        // XGrabKey through XWayland only fires while a Tide window has focus,
        // so Wayland sessions need the portal for truly global chords.
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        if wayland {
            match portal::PortalShortcuts::new(callback.clone()) {
                Ok(portal) => {
                    return Ok(Self {
                        backend: Backend::Portal(portal),
                    })
                }
                Err(error) if std::env::var_os("DISPLAY").is_none() => return Err(error),
                Err(_) => {}
            }
        }
        x11::GrabbedShortcuts::new(callback).map(|x11| Self {
            backend: Backend::X11(x11),
        })
    }

    pub fn apply(&mut self, chords: [Option<Chord>; 3]) -> Result<(), String> {
        match &mut self.backend {
            Backend::Portal(portal) => portal.apply(chords),
            Backend::X11(x11) => x11.apply(chords),
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub struct GlobalShortcuts;

#[cfg(not(target_os = "linux"))]
impl GlobalShortcuts {
    pub fn new(_callback: impl Fn(crate::MediaAction) + Send + 'static) -> Result<Self, String> {
        Err(crate::UNSUPPORTED.into())
    }
    pub fn apply(&mut self, _chords: [Option<Chord>; 3]) -> Result<(), String> {
        Err(crate::UNSUPPORTED.into())
    }
}

/// The action bound to each of the three settings slots.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const ACTIONS: [crate::MediaAction; 3] = [
    crate::MediaAction::Toggle,
    crate::MediaAction::Next,
    crate::MediaAction::Previous,
];

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
        // Console switching owns Ctrl+Alt+F1–F12 on Linux; other F-keys work.
        for text in ["Ctrl+Alt+F1", "Ctrl+Alt+F9", "Ctrl+Alt+Shift+F11"] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
        for text in ["Ctrl+Alt+F13", "Ctrl+F9", "Alt+F9"] {
            assert!(parse(text).unwrap().is_some(), "rejected {text}");
        }
        for text in ["Ctrl+Alt+P", "Alt+Shift+Space", "Ctrl+F24", "Alt+9"] {
            assert!(parse(text).unwrap().is_some());
        }
        assert!(parse_bindings(&["Ctrl+Alt+P".into(), "Alt+Ctrl+P".into(), "".into()]).is_err());
    }

    #[test]
    fn chords_map_to_x11_and_portal_names() {
        let chord = parse("Ctrl+Alt+P").unwrap().unwrap();
        assert_eq!(chord.keysym(), 0x70);
        assert_eq!(chord.x11_mask(), 4 | 8);
        assert_eq!(chord.portal_trigger(), "CTRL+ALT+p");
        let chord = parse("Alt+Shift+F5").unwrap().unwrap();
        assert_eq!(chord.keysym(), 0xffc2);
        assert_eq!(chord.x11_mask(), 1 | 8);
        assert_eq!(chord.portal_trigger(), "ALT+SHIFT+F5");
        assert_eq!(
            parse("Ctrl+Space").unwrap().unwrap().portal_trigger(),
            "CTRL+space"
        );
        assert_eq!(parse("Ctrl+7").unwrap().unwrap().keysym(), 0x37);
    }
}
