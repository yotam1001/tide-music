//! Version-independent user settings with atomic replacement and one backup.
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 8 * 1024 * 1024;

fn backup(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn read(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_BYTES && valid(&bytes)).then_some(bytes)
}

fn valid(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes).is_ok_and(|value| value.is_object())
}

pub(super) fn load(path: &Path) -> Option<Value> {
    read(path)
        .or_else(|| read(&backup(path)))
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn save(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() as u64 > MAX_BYTES || !valid(bytes) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    fs::create_dir_all(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    // Both copies contain the latest completed choice, not the previous one.
    // Otherwise recovering after an opt-out could silently re-enable sharing.
    // Publish the backup first so a failed primary replacement leaves the live
    // primary intact, and never copy potentially corrupted old primary bytes.
    replace(&backup(path), bytes)?;
    replace(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENABLED: &[u8] = br#"{"history_feedback":true,"discord_presence":true}"#;
    const DISABLED: &[u8] = br#"{"history_feedback":false,"discord_presence":false}"#;

    #[test]
    fn abandoned_staging_and_corrupt_primary_keep_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("TideMusic/settings.json");
        save(&path, ENABLED).unwrap();
        // An interrupted write affects only the unpublished staging file.
        let mut staged = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
        staged.write_all(b"{partial").unwrap();
        assert_eq!(load(&path).unwrap()["discord_presence"], true);
        drop(staged);
        save(&path, DISABLED).unwrap();
        fs::write(&path, b"{interrupted old-version save").unwrap();
        let recovered = load(&path).unwrap();
        assert_eq!(recovered["history_feedback"], false);
        assert_eq!(recovered["discord_presence"], false);
        save(&path, ENABLED).unwrap();
        assert_eq!(read(&backup(&path)).unwrap(), ENABLED);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
    }

    #[test]
    fn malformed_or_oversized_writes_do_not_destroy_saved_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        save(&path, ENABLED).unwrap();
        for bytes in [
            b"null".as_slice(),
            b"{".as_slice(),
            &vec![b' '; MAX_BYTES as usize + 1],
        ] {
            assert!(save(&path, bytes).is_err());
            assert_eq!(fs::read(&path).unwrap(), ENABLED);
        }
    }

    #[cfg(windows)]
    #[test]
    fn failed_atomic_replacement_preserves_the_live_file() {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        save(&path, ENABLED).unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(save(&path, DISABLED).is_err());
        assert_eq!(load(&path).unwrap()["history_feedback"], true);
        drop(held);
        save(&path, DISABLED).unwrap();
        assert_eq!(load(&path).unwrap()["discord_presence"], false);
    }
}
