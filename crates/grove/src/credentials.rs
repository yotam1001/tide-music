//! Grove owns persistence; librespot never gets a plaintext credentials cache.
use serde::{de::DeserializeOwned, Serialize};
use zeroize::Zeroizing;

pub const LIBRARY: &str = "library-v1";
pub const PLAYBACK: &str = "playback-v1";

fn entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("GroveMusic", name)
        .map_err(|_| "Windows Credential Manager is unavailable.".into())
}

pub fn load<T: DeserializeOwned>(name: &str) -> Result<Option<T>, String> {
    match entry(name)?.get_password() {
        Ok(value) => serde_json::from_str(&Zeroizing::new(value))
            .map(Some)
            .map_err(|_| {
                "Saved Spotify approval is unreadable. Sign out, then sign in again.".into()
            }),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Could not read Grove’s protected Spotify approval.".into()),
    }
}

pub fn save<T: Serialize>(name: &str, value: &T) -> Result<(), String> {
    let value = Zeroizing::new(
        serde_json::to_string(value)
            .map_err(|_| "Could not prepare protected storage.".to_string())?,
    );
    entry(name)?.set_password(&value).map_err(|_| "Could not save Spotify approval in Windows Credential Manager. This session works until Grove closes.".into())
}

pub fn delete(name: &str) -> Result<(), String> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => {
            Err("Could not remove a saved Spotify approval. Sign-out is incomplete; retry.".into())
        }
    }
}
