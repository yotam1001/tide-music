//! Local audio preferences apply equally to a host, listener or solo player.
use super::*;
/// The engine's options at a crossfade `gain_db` (0 outside a crossfade).
/// A crossfade shapes both ends of a song, so fades are off while it is on.
pub(super) fn options(s: &Settings, gain_db: f64) -> tide_playback::AudioOptions {
    tide_playback::AudioOptions {
        normalize: s.normalize_volume,
        device: s.audio_device.clone(),
        equalizer: s.equalizer,
        fade: s.fade_transitions && s.crossfade_seconds == 0,
        gain_db,
        bit_perfect: s.bit_perfect,
    }
}
pub fn apply(s: &State, u: &AppWindow) -> bool {
    match s
        .engine
        .as_ref()
        .ok_or("Playback engine unavailable".to_string())
        .and_then(|e| e.configure_audio(options(&s.settings, s.crossfade.primary_gain_db())))
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
    u.set_equalizer(i32::from(s.settings.equalizer));
    u.set_fade_transitions(s.settings.fade_transitions);
    u.set_crossfade_seconds(i32::from(s.settings.crossfade_seconds));
    u.set_bit_perfect(s.settings.bit_perfect);
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
        if !options(&s.settings, 0.0).valid() {
            s.settings.audio_device = "auto".into();
            s.settings.equalizer = 0;
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
    let state_fade = state.clone();
    let weak = ui.as_weak();
    ui.on_toggle_fade_transitions(move || {
        if let Some(u) = weak.upgrade() {
            let mut s = state_fade.borrow_mut();
            let before = s.settings.fade_transitions;
            s.settings.fade_transitions = !before;
            if apply(&s, &u) {
                u.set_fade_transitions(s.settings.fade_transitions);
                save(&s.settings);
            } else {
                s.settings.fade_transitions = before;
            }
        }
    });
    let state_bit_perfect = state.clone();
    let weak = ui.as_weak();
    ui.on_toggle_bit_perfect(move || {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let mut s = state_bit_perfect.borrow_mut();
        crate::crossfade::Crossfade::cancel(&mut s);
        let before = s.settings.bit_perfect;
        s.settings.bit_perfect = !before;
        if apply(&s, &u) {
            u.set_bit_perfect(s.settings.bit_perfect);
            // Full volume when turned on; the chosen volume returns when off.
            let volume = f64::from(u.get_volume());
            crate::send(&mut s, &u, tide_core::PlaybackCommand::SetVolume(volume));
            save(&s.settings);
        } else {
            s.settings.bit_perfect = before;
        }
    });
    let state_crossfade = state.clone();
    let weak = ui.as_weak();
    ui.on_choose_crossfade(move |seconds| {
        let (Some(u), Ok(seconds)) = (weak.upgrade(), u8::try_from(seconds)) else {
            return;
        };
        if !crate::crossfade::LENGTHS.contains(&seconds) {
            return;
        }
        let mut s = state_crossfade.borrow_mut();
        crate::crossfade::Crossfade::cancel(&mut s);
        let before = s.settings.crossfade_seconds;
        s.settings.crossfade_seconds = seconds;
        if apply(&s, &u) {
            u.set_crossfade_seconds(i32::from(seconds));
            save(&s.settings);
        } else {
            s.settings.crossfade_seconds = before;
        }
    });
    let state_equalizer = state.clone();
    let weak = ui.as_weak();
    ui.on_choose_equalizer(move |index| {
        let (Some(u), Ok(index)) = (weak.upgrade(), u8::try_from(index)) else {
            return;
        };
        if usize::from(index) >= tide_playback::EQUALIZER_PRESETS.len() {
            return;
        }
        let mut s = state_equalizer.borrow_mut();
        let before = s.settings.equalizer;
        s.settings.equalizer = index;
        if apply(&s, &u) {
            u.set_equalizer(i32::from(index));
            save(&s.settings);
        } else {
            s.settings.equalizer = before;
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
