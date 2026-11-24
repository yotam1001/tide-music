//! Linux counterpart of the setup handoff: replace the running AppImage with
//! the already verified download, then start it once this process has exited.
//!
//! The new image is written beside the old one and renamed over it, so the
//! swap is atomic and a failed copy leaves the working AppImage untouched.
//! The running copy keeps its mounted, open image until it quits.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

/// The AppImage this process runs from, when the user can replace it.
pub(super) fn replaceable_appimage() -> Option<PathBuf> {
    let image = PathBuf::from(std::env::var_os("APPIMAGE")?);
    let mount = PathBuf::from(std::env::var_os("APPDIR")?);
    let executable = std::env::current_exe().ok()?;
    replaceable(&image, &mount, &executable)
}

fn replaceable(image: &Path, mount: &Path, executable: &Path) -> Option<PathBuf> {
    // Only the real runtime sets both: the image path and its mounted tree
    // that this very executable runs from.
    if !image.is_absolute() || !mount.is_absolute() || !executable.starts_with(mount) {
        return None;
    }
    let name = image.file_name()?.to_str()?;
    if !name.to_ascii_lowercase().ends_with(".appimage") {
        return None;
    }
    super::plain(image).ok()?;
    let metadata = fs::symlink_metadata(image).ok()?;
    let uid = unsafe { libc::getuid() };
    if !metadata.is_file() || metadata.uid() != uid {
        return None;
    }
    let folder = image.parent()?;
    let folder_c = std::ffi::CString::new(folder.as_os_str().as_encoded_bytes()).ok()?;
    (unsafe { libc::access(folder_c.as_ptr(), libc::W_OK) } == 0).then(|| image.to_owned())
}

/// Copy `source` beside `target`, make it executable, flush it, and rename it
/// over `target`. Nothing changes at `target` unless every step succeeds.
fn install_file(source: &Path, target: &Path) -> Result<(), String> {
    let folder = target.parent().ok_or("Invalid AppImage location")?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid AppImage name")?;
    let temporary = folder.join(format!(".{name}.update-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    let result = (|| {
        let mut input = File::open(source).map_err(|_| "Could not open the verified update")?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&temporary)
            .map_err(|_| {
                "Could not write next to the Tide AppImage. Check the folder's permissions."
            })?;
        let mut buffer = [0; 64 * 1024];
        let mut copied = 0u64;
        loop {
            let count = input
                .read(&mut buffer)
                .map_err(|_| "Could not read the verified update")?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > super::MAX_INSTALLER {
                return Err("The update is larger than expected");
            }
            output
                .write_all(&buffer[..count])
                .map_err(|_| "Could not save the update. Check free disk space.")?;
        }
        output
            .set_permissions(fs::Permissions::from_mode(0o755))
            .map_err(|_| "Could not make the update executable")?;
        output
            .sync_all()
            .map_err(|_| "Could not finish saving the update")?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    if fs::rename(&temporary, target).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err("Could not replace the Tide AppImage".into());
    }
    if let Ok(folder) = File::open(folder) {
        let _ = folder.sync_all();
    }
    Ok(())
}

/// Start the new AppImage after this process exits, detached from it.
fn relaunch(target: &Path) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2""#)
        .arg("tide-relaunch")
        .arg(std::process::id().to_string())
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The runtime sets these again for the new image; stale values would point
    // at the old, soon unmounted tree.
    for name in [
        "APPIMAGE",
        "APPDIR",
        "ARGV0",
        "OWD",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
    ] {
        match std::env::var_os(format!("TIDE_HOST_{name}")) {
            Some(value) if !value.is_empty() => command.env(name, value),
            _ => command.env_remove(name),
        };
    }
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command
        .spawn()
        .map(drop)
        .map_err(|_| "Could not schedule Tide to restart. Start Tide again to finish.".into())
}

#[derive(Debug)]
pub(super) struct Pending {
    receiver: mpsc::Receiver<Result<(), String>>,
    deadline: Instant,
}

impl Pending {
    pub(super) fn start(path: &Path, _version: &str) -> Result<Self, String> {
        let target = replaceable_appimage().ok_or(
            "This copy of Tide cannot replace itself. Download the new AppImage from GitHub.",
        )?;
        let source = path.to_owned();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("tide-appimage-update".into())
            .spawn(move || {
                let result = install_file(&source, &target).and_then(|()| relaunch(&target));
                let _ = sender.send(result);
            })
            .map_err(|_| "Could not start installing the update. Retry Restart.")?;
        Ok(Self {
            receiver,
            deadline: Instant::now() + Duration::from_secs(120),
        })
    }

    pub(super) fn poll(&mut self) -> Option<Result<(), String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("The update stopped unexpectedly. Retry Restart.".into()))
            }
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= self.deadline => Some(Err(
                "Installing the update took too long. Your music continues; retry Restart.".into(),
            )),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_mounted_owned_appimage_in_a_writable_folder_is_replaceable() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("Tide-x86_64.AppImage");
        fs::write(&image, b"old").unwrap();
        let mount = Path::new("/tmp/.mount_TideXYZ");
        let exe = mount.join("usr/bin/tide-music");
        assert_eq!(replaceable(&image, mount, &exe), Some(image.clone()));
        // Not running from the image's mount.
        assert!(replaceable(&image, mount, Path::new("/usr/bin/tide-music")).is_none());
        // Relative or non-AppImage paths.
        assert!(replaceable(Path::new("Tide.AppImage"), mount, &exe).is_none());
        let other = dir.path().join("tide-music");
        fs::write(&other, b"x").unwrap();
        assert!(replaceable(&other, mount, &exe).is_none());
        // A linked image is never replaced through its link.
        let link = dir.path().join("Link.AppImage");
        std::os::unix::fs::symlink(&image, &link).unwrap();
        assert!(replaceable(&link, mount, &exe).is_none());
    }

    #[test]
    fn install_replaces_atomically_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Tide-x86_64.AppImage");
        let source = dir.path().join("staged");
        fs::write(&target, b"old image").unwrap();
        fs::write(&source, b"new image").unwrap();
        install_file(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new image");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".update-"))
            .collect();
        assert!(leftovers.is_empty());
        // A missing source leaves the working image untouched.
        assert!(install_file(&dir.path().join("missing"), &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new image");
    }
}
