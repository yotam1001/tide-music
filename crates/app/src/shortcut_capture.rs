//! Focused shortcut recording only: no keyboard hook, typing log or background capture.
#[cfg(any(windows, test))]
fn encode(
    text: &str,
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> Result<Option<String>, &'static str> {
    use slint::platform::Key;
    let Some(key) = text.chars().next().filter(|_| text.chars().count() == 1) else {
        return Ok(None);
    };
    if [
        Key::Control,
        Key::ControlR,
        Key::Alt,
        Key::AltGr,
        Key::Shift,
        Key::ShiftR,
        Key::Meta,
        Key::MetaR,
    ]
    .into_iter()
    .any(|k| char::from(k) == key)
    {
        return Ok(None);
    }
    if meta || !(control || alt) {
        return Err("Hold Ctrl or Alt, then press a letter, number, Space or function key.");
    }
    let name = if key.is_ascii_alphanumeric() {
        key.to_ascii_uppercase().to_string()
    } else if key == ' ' {
        "Space".into()
    } else if ('\u{f704}'..='\u{f71b}').contains(&key) {
        format!("F{}", key as u32 - 0xf704 + 1)
    } else {
        return Err("Choose a letter, number, Space or function key with Ctrl or Alt.");
    };
    let mut value = String::new();
    if control {
        value.push_str("Ctrl+");
    }
    if alt {
        value.push_str("Alt+");
    }
    if shift {
        value.push_str("Shift+");
    }
    value.push_str(&name);
    tide_windows::shortcuts::parse_bindings(&[value.clone(), String::new(), String::new()])
        .map_err(|_| "That combination is reserved. Choose another shortcut.")?;
    Ok(Some(value))
}

#[cfg(windows)]
mod recording {
    use super::super::{save, AppWindow, State};
    use slint::ComponentHandle;
    use std::{cell::RefCell, rc::Rc};
    use tide_windows::shortcuts::{parse_bindings, GlobalShortcuts};
    type Manager = Rc<RefCell<Option<GlobalShortcuts>>>;
    fn apply(manager: &Manager, values: &[String; 3]) -> Result<(), String> {
        let bindings = parse_bindings(values)?;
        manager
            .borrow_mut()
            .as_mut()
            .ok_or("Global shortcuts are unavailable in this mode.")?
            .apply(bindings)
    }
    fn cancel(ui: &AppWindow, state: &Rc<RefCell<State>>, manager: &Manager) {
        if ui.get_shortcut_recording_index() < 0 {
            return;
        }
        ui.set_shortcut_recording_index(-1);
        let old = state.borrow().settings.shortcuts.clone();
        ui.set_shortcut_status(apply(manager, &old).err().unwrap_or_default().into());
    }
    fn commit(
        ui: &AppWindow,
        state: &Rc<RefCell<State>>,
        manager: &Manager,
        index: usize,
        value: String,
    ) {
        let mut values = state.borrow().settings.shortcuts.clone();
        values[index] = value;
        // Validation does not disturb the saved values or registrations.
        if let Err(error) = parse_bindings(&values) {
            ui.set_shortcut_status(error.into());
            return;
        }
        match apply(manager, &values) {
            Ok(()) => {
                ui.set_shortcut_recording_index(-1);
                ui.set_shortcut_toggle(values[0].clone().into());
                ui.set_shortcut_next(values[1].clone().into());
                ui.set_shortcut_previous(values[2].clone().into());
                let mut s = state.borrow_mut();
                s.settings.shortcuts = values;
                save(&s.settings);
                ui.set_shortcut_status("Saved".into());
            }
            Err(error) => {
                cancel(ui, state, manager);
                ui.set_shortcut_status(error.into());
            }
        }
    }
    pub(crate) fn wire(ui: &AppWindow, state: Rc<RefCell<State>>, manager: Manager) {
        let (s, m, w) = (state.clone(), manager.clone(), ui.as_weak());
        ui.on_begin_shortcut(move |index| {
            let Some(u) = w.upgrade() else { return };
            if !(0..3).contains(&index) {
                return;
            }
            cancel(&u, &s, &m);
            match apply(&m, &Default::default()) {
                Ok(()) => {
                    u.set_shortcut_recording_index(index);
                    u.set_shortcut_status("Press your shortcut. Esc cancels.".into());
                }
                Err(error) => u.set_shortcut_status(error.into()),
            }
        });
        let (s, m, w) = (state.clone(), manager.clone(), ui.as_weak());
        ui.on_cancel_shortcut(move || {
            if let Some(u) = w.upgrade() {
                cancel(&u, &s, &m)
            }
        });
        let (s, m, w) = (state.clone(), manager.clone(), ui.as_weak());
        ui.on_clear_shortcut(move |index| {
            let Some(u) = w.upgrade() else { return };
            if !(0..3).contains(&index) {
                return;
            }
            cancel(&u, &s, &m);
            commit(&u, &s, &m, index as usize, String::new());
        });
        let w = ui.as_weak();
        ui.on_record_shortcut(move |text, control, alt, shift, meta| {
            let Some(u) = w.upgrade() else { return };
            let index = u.get_shortcut_recording_index();
            if !(0..3).contains(&index) {
                return;
            }
            match super::encode(&text, control, alt, shift, meta) {
                Ok(Some(value)) => commit(&u, &state, &manager, index as usize, value),
                Ok(None) => {}
                Err(error) => u.set_shortcut_status(error.into()),
            }
        });
    }
}
#[cfg(windows)]
pub(super) use recording::wire;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_chords_match_supported_global_bindings() {
        assert_eq!(
            encode("p", true, true, false, false).unwrap().as_deref(),
            Some("Ctrl+Alt+P")
        );
        assert_eq!(
            encode(" ", false, true, true, false).unwrap().as_deref(),
            Some("Alt+Shift+Space")
        );
        assert_eq!(
            encode("\u{f704}", true, false, false, false)
                .unwrap()
                .as_deref(),
            Some("Ctrl+F1")
        );
        assert!(encode("\u{f70f}", true, false, false, false).is_err());
        assert!(encode("p", false, false, false, false).is_err());
        assert!(encode("p", true, false, false, true).is_err());
        assert!(encode("\u{0011}", true, false, false, false)
            .unwrap()
            .is_none());
        assert!(encode("password text", true, false, false, false)
            .unwrap()
            .is_none());
    }
}
