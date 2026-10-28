//! Local audio preferences apply equally to a host, listener or solo player.
use super::*;
fn options(s: &Settings) -> tide_playback::AudioOptions {
    tide_playback::AudioOptions {
        normalize: s.normalize_volume,
        device: s.audio_device.clone(),
    }
}
pub fn apply(s: &State, u: &AppWindow) -> bool {
    match s
        .engine
        .as_ref()
        .ok_or("Playback engine unavailable".to_string())
        .and_then(|e| e.configure_audio(options(&s.settings)))
    {
        Ok(()) => true,
        Err(_) => {
            u.set_audio_status(
                "Could not apply audio settings. Try again when playback is available.".into(),
            );
            false
        }
    }
}
fn outputs(s: &State, p: &tide_core::PlaybackSnapshot) -> Vec<tide_core::AudioDevice> {
    let mut devices = vec![tide_core::AudioDevice {
        name: "auto".into(),
        description: "Windows default".into(),
    }];
    devices.extend(p.audio_devices.iter().cloned());
    if !devices.iter().any(|d| d.name == s.settings.audio_device) {
        devices.push(tide_core::AudioDevice {
            name: s.settings.audio_device.clone(),
            description: "Saved output (unavailable)".into(),
        });
    }
    devices
}
pub fn refresh(s: &State, u: &AppWindow, p: &tide_core::PlaybackSnapshot) {
    let devices = outputs(s, p);
    u.set_normalize_volume(s.settings.normalize_volume);
    u.set_audio_device_index(
        devices
            .iter()
            .position(|d| d.name == s.settings.audio_device)
            .unwrap_or(0) as i32,
    );
    u.set_audio_device_names(update_model(
        u.get_audio_device_names(),
        devices
            .iter()
            .map(|d| d.description.clone().into())
            .collect(),
    ));
    u.set_audio_status(if p.audio_settings_error {
        "The output or audio effect could not be applied. Choose Windows default or turn Even volume off."
    } else if s.settings.audio_device != "auto" && !p.audio_devices.iter().any(|d| d.name == s.settings.audio_device) {
        "Saved output unavailable. Reconnect it or choose Windows default."
    } else if p.audio_devices.is_empty() {
        "Play a song to discover your headphones and speakers."
    } else { "Applies only to Tide. Windows default follows your system output." }.into());
}
pub fn install(state: &Rc<RefCell<State>>, ui: &AppWindow) {
    {
        let mut s = state.borrow_mut();
        if !options(&s.settings).valid() {
            s.settings.audio_device = "auto".into();
        }
        if s.engine.is_some() {
            apply(&s, ui);
        }
        refresh(
            &s,
            ui,
            &s.engine.as_ref().map(|e| e.snapshot()).unwrap_or_default(),
        );
    }
    let state_toggle = state.clone();
    let weak = ui.as_weak();
    ui.on_toggle_normalization(move || {
        if let Some(u) = weak.upgrade() {
            let mut s = state_toggle.borrow_mut();
            let before = s.settings.normalize_volume;
            s.settings.normalize_volume = !before;
            if apply(&s, &u) {
                u.set_normalize_volume(s.settings.normalize_volume);
                save(&s.settings);
            } else {
                s.settings.normalize_volume = before;
            }
        }
    });
    let state_output = state.clone();
    let weak = ui.as_weak();
    ui.on_choose_audio_device(move |index| {
        if let Some(u) = weak.upgrade() {
            let mut s = state_output.borrow_mut();
            let p = s.engine.as_ref().map(|e| e.snapshot()).unwrap_or_default();
            let devices = outputs(&s, &p);
            if let Some(device) = usize::try_from(index).ok().and_then(|i| devices.get(i)) {
                let before = s.settings.audio_device.clone();
                s.settings.audio_device = device.name.clone();
                if apply(&s, &u) {
                    save(&s.settings);
                    refresh(&s, &u, &p);
                } else {
                    s.settings.audio_device = before;
                }
            }
        }
    });
}
