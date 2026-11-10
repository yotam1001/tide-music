//! Grove-owned preferences. Spotify credentials remain in Credential Manager.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub light: bool,
    pub compact: bool,
    pub autoplay: bool,
    pub media_keys: bool,
    pub local_history: bool,
    pub volume: f32,
    pub hidden: HashSet<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            light: false,
            compact: false,
            autoplay: false,
            media_keys: true,
            local_history: false,
            volume: 35.0,
            hidden: HashSet::new(),
        }
    }
}
fn path() -> Result<PathBuf, String> {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or("Windows application-data directory is unavailable.")?;
    Ok(root.join("GroveMusic").join("preferences.json"))
}
impl Preferences {
    pub fn load() -> Result<Self, String> {
        let path = path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let size = std::fs::metadata(&path)
            .map_err(|_| "Could not inspect Grove preferences.")?
            .len();
        if size > 256 * 1024 {
            return Err("Grove preferences are too large. Defaults were used.".into());
        }
        let bytes = std::fs::read(path).map_err(|_| "Could not read Grove preferences.")?;
        let mut prefs: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Grove preferences are unreadable. Defaults were used.")?;
        prefs.volume = if prefs.volume.is_finite() {
            prefs.volume.clamp(0.0, 100.0)
        } else {
            35.0
        };
        prefs
            .hidden
            .retain(|uri| crate::spotify::spotify_uri(uri).ok().as_deref() == Some(uri));
        if prefs.hidden.len() > 2000 {
            prefs.hidden.clear();
        }
        Ok(prefs)
    }
    pub fn save(&self) -> Result<(), String> {
        let path = path()?;
        let parent = path.parent().ok_or("Invalid Grove preference path.")?;
        std::fs::create_dir_all(parent)
            .map_err(|_| "Could not create Grove preference directory.")?;
        let temporary = parent.join(format!("preferences-{}.tmp", crate::jam::id()));
        let bytes = serde_json::to_vec(self).map_err(|_| "Could not prepare Grove preferences.")?;
        std::fs::write(&temporary, bytes).map_err(|_| "Could not write Grove preferences.")?;
        if std::fs::rename(&temporary, &path).is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err(
                "Could not save Grove preferences. Your changes apply only for this session."
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_preferences_have_safe_defaults_and_no_account_secrets() {
        let prefs: Preferences = serde_json::from_str("{}").unwrap();
        assert!(!prefs.local_history);
        assert!(!prefs.autoplay);
        assert_eq!(prefs.volume, 35.0);
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(!json.contains("token"));
        assert!(!json.contains("credential"));
    }
}
