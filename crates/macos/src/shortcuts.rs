//! Opt-in registered chords, never keyboard hooks or hardware media-key grabs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    modifiers: u32,
    key: u32,
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
        "Use \u{2318}/Command, \u{2325}/Option or Ctrl plus a letter, digit, Space or F1-F20, for example Cmd+Option+P.".to_string()
    };
    if text.len() > 64 {
        return Err(invalid());
    }
    let mut modifiers = 0;
    let mut key = None;
    for part in text.split('+').map(|part| part.trim().to_ascii_uppercase()) {
        let modifier = match part.as_str() {
            "CTRL" | "CONTROL" => 4096,
            "CMD" | "COMMAND" | "\u{2318}" => 256,
            "ALT" | "OPTION" | "\u{2325}" => 2048,
            "SHIFT" => 512,
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
                    if !(1..=20).contains(&number) {
                        return Err(invalid());
                    }
                    0x70 + number - 1
                } else {
                    return Err(invalid());
                },
            );
        }
    }
    if modifiers & (256 | 2048 | 4096) == 0 {
        return Err(invalid());
    }
    Ok(Some(Chord {
        modifiers,
        key: key_code(key.ok_or_else(invalid)?).ok_or_else(invalid)?,
    }))
}

fn key_code(key: u32) -> Option<u32> {
    const LETTERS: [u32; 26] = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6,
    ];
    const DIGITS: [u32; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    const FUNCTION: [u32; 20] = [
        122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
    ];
    match key {
        65..=90 => Some(LETTERS[(key - 65) as usize]),
        48..=57 => Some(DIGITS[(key - 48) as usize]),
        0x20 => Some(49),
        0x70..=0x83 => Some(FUNCTION[(key - 0x70) as usize]),
        _ => None,
    }
}
#[cfg(not(target_os = "macos"))]
pub struct GlobalShortcuts;
#[cfg(not(target_os = "macos"))]
impl GlobalShortcuts {
    pub fn new(_callback: impl Fn(crate::MediaAction) + Send + 'static) -> Result<Self, String> {
        Err(crate::UNSUPPORTED.into())
    }
    pub fn apply(&mut self, _chords: [Option<Chord>; 3]) -> Result<(), String> {
        Err(crate::UNSUPPORTED.into())
    }
}
#[cfg(target_os = "macos")]
mod carbon;
#[cfg(target_os = "macos")]
pub use carbon::GlobalShortcuts;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_modified_distinct_chords() {
        assert_eq!(
            parse(" cmd + option + p ").unwrap(),
            parse("\u{2325}+\u{2318}+P").unwrap()
        );
        assert!(parse(" ").unwrap().is_none());
        for text in [
            "P",
            "Space",
            "Shift+P",
            "Ctrl",
            "Ctrl++P",
            "Win+P",
            "Ctrl+F21",
            "Ctrl+Ctrl+P",
            "P+Ctrl",
            "Cmd+MediaPlayPause",
        ] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
        for text in [
            "Cmd+Alt+P",
            "Option+Shift+Space",
            "Control+F20",
            "Alt+9",
            "\u{2318}+F12",
        ] {
            assert!(parse(text).unwrap().is_some());
        }
        assert!(
            parse_bindings(&["Cmd+Alt+P".into(), "Option+Command+P".into(), "".into()]).is_err()
        );
    }
}
