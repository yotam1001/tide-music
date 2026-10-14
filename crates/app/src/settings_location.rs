use std::path::PathBuf;

/// Keep the existing folder name so upgrades find the user's saved queue.
pub(super) fn settings_path(mut env: impl FnMut(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let mut absolute = |key| env(key).filter(|path| path.is_absolute());
    let base = if cfg!(windows) {
        absolute("APPDATA")
            .or_else(|| absolute("LOCALAPPDATA"))
            .or_else(|| absolute("USERPROFILE").map(|home| home.join("AppData/Roaming")))
    } else if cfg!(target_os = "macos") {
        absolute("HOME").map(|home| home.join("Library/Application Support"))
    } else {
        absolute("XDG_CONFIG_HOME").or_else(|| absolute("HOME").map(|home| home.join(".config")))
    };
    // No temp fallback: absent user directories must not silently lose settings.
    base.map(|base| base.join("TideMusic/settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_relative_environment_never_uses_temp() {
        assert_eq!(settings_path(|_| None), None);
        assert_eq!(settings_path(|_| Some("relative".into())), None);
    }
    #[cfg(windows)]
    #[test]
    fn windows_preserves_location_and_has_durable_fallbacks() {
        let resolve = |entries: &[(&str, &str)]| {
            settings_path(|key| {
                entries
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| PathBuf::from(value))
            })
        };
        assert_eq!(
            resolve(&[("APPDATA", "C:/roaming"), ("LOCALAPPDATA", "C:/local")]),
            Some("C:/roaming/TideMusic/settings.json".into())
        );
        assert_eq!(
            resolve(&[("APPDATA", ""), ("LOCALAPPDATA", "C:/local")]),
            Some("C:/local/TideMusic/settings.json".into())
        );
        assert_eq!(
            resolve(&[("USERPROFILE", "C:/user")]),
            Some("C:/user/AppData/Roaming/TideMusic/settings.json".into())
        );
    }
}
