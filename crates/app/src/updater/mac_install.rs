//! In-place Mac updates: the verified disk image is mounted read-only, its
//! Tide.app is copied into Tide's own update cache and checked, and after Tide
//! quits a detached shell swaps the bundle and reopens it.
//!
//! The image is downloaded by Tide itself rather than a browser, so the copy
//! carries no quarantine flag and an approved Tide is not blocked again by
//! Gatekeeper after every update. Integrity comes from the version-pinned
//! GitHub release URL, its published SHA-256 and macOS code-signature
//! validation of the staged bundle.
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(super) const BUNDLE_ID: &str = "com.tidemusic.Tide";
const STAGED: &str = "staged";

/// The running bundle when it can be replaced in place: a `*.app` outside a
/// mounted disk image, inside a folder this user can write.
pub(super) fn current_bundle(executable: &Path) -> Option<PathBuf> {
    let bundle = executable.parent()?.parent()?.parent()?;
    if bundle.extension()? != "app"
        || executable.parent()?.file_name()? != "MacOS"
        || bundle.starts_with("/Volumes")
        || super::plain(bundle).is_err()
    {
        return None;
    }
    let parent = bundle.parent()?;
    let probe = parent.join(format!(".tide-update-probe-{}", std::process::id()));
    let writable = std::fs::create_dir(&probe).is_ok();
    let _ = std::fs::remove_dir(&probe);
    writable.then(|| bundle.to_owned())
}

fn run(command: &mut Command, failure: &str) -> Result<String, String> {
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| failure.to_string())?;
    if !output.status.success() {
        return Err(failure.into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn plist_value(app: &Path, key: &str) -> Option<String> {
    run(
        Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist")),
        "plist",
    )
    .ok()
}

/// Bundle identity and signature checks shared by staging and the restart.
pub(super) fn check_bundle(app: &Path, version: &str) -> Result<(), String> {
    super::plain(app)?;
    if plist_value(app, "CFBundleIdentifier").as_deref() != Some(BUNDLE_ID)
        || plist_value(app, "CFBundleShortVersionString").as_deref() != Some(version)
    {
        return Err(
            "The downloaded Tide is not the offered release. Nothing was installed.".into(),
        );
    }
    run(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app),
        "The downloaded Tide failed its code-signature check. Nothing was installed.",
    )?;
    Ok(())
}

struct Mounted(PathBuf);
impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet", "-force"])
            .arg(&self.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// Mount the verified image read-only and copy its Tide.app into the update
/// cache. Returns the staged bundle.
pub(super) fn stage(image: &Path, version: &str) -> Result<PathBuf, String> {
    let directory = image.parent().ok_or("Invalid update folder")?;
    let mount = directory.join("mount");
    let staged = directory.join(STAGED);
    for path in [&mount, &staged] {
        super::plain(path)?;
        if path.exists() {
            if path == &mount {
                let _ = Command::new("/usr/bin/hdiutil")
                    .args(["detach", "-quiet", "-force"])
                    .arg(path)
                    .status();
                let _ = std::fs::remove_dir(path);
            } else {
                std::fs::remove_dir_all(path)
                    .map_err(|_| "Could not clear the previous staged update")?;
            }
        }
    }
    std::fs::create_dir(&mount).map_err(|_| "Could not prepare the update")?;
    run(
        Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noautoopen",
                "-quiet",
                "-mountpoint",
            ])
            .arg(&mount)
            .arg(image),
        "macOS could not open the downloaded disk image. Try the download again.",
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_dir(&mount);
    })?;
    let mounted = Mounted(mount);
    let source = mounted.0.join("Tide.app");
    std::fs::create_dir(&staged).map_err(|_| "Could not prepare the update")?;
    let target = staged.join("Tide.app");
    run(
        Command::new("/usr/bin/ditto")
            .arg("--noqtn")
            .arg(&source)
            .arg(&target),
        "Could not copy the update. Check free disk space.",
    )?;
    drop(mounted);
    check_bundle(&target, version)?;
    Ok(target)
}

/// After Tide exits, replace `bundle` with `staged`, keep the old copy until
/// the move succeeds, then open the new Tide. Arguments are passed
/// positionally; nothing is interpolated into the script.
pub(super) fn apply(staged: &Path, bundle: &Path, version: &str) -> Result<(), String> {
    check_bundle(staged, version)?;
    let backup = staged
        .parent()
        .and_then(Path::parent)
        .ok_or("Invalid update folder")?
        .join("previous.app");
    super::plain(&backup)?;
    const SCRIPT: &str = r#"pid="$1"; staged="$2"; target="$3"; backup="$4"
i=0
while kill -0 "$pid" 2>/dev/null; do
  i=$((i + 1)); [ "$i" -gt 600 ] && exit 1; sleep 0.1
done
rm -rf "$backup"
mv "$target" "$backup" || { open "$target"; exit 1; }
if mv "$staged" "$target"; then rm -rf "$backup"; else mv "$backup" "$target"; fi
open "$target""#;
    use std::os::unix::process::CommandExt;
    Command::new("/bin/sh")
        .args(["-c", SCRIPT, "tide-update"])
        .arg(std::process::id().to_string())
        .arg(staged)
        .arg(bundle)
        .arg(&backup)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|_| {
            "macOS could not start the update. Your music continues; retry Restart.".into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_writable_app_bundle_outside_disk_images_is_replaceable() {
        let root = std::env::temp_dir().join(format!("tide-mac-install-{}", std::process::id()));
        let executable = root.join("Tide.app/Contents/MacOS/tide-music");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"").unwrap();
        let canonical = std::fs::canonicalize(&root).unwrap();
        let executable = canonical.join("Tide.app/Contents/MacOS/tide-music");
        assert_eq!(
            current_bundle(&executable),
            Some(canonical.join("Tide.app"))
        );
        assert_eq!(current_bundle(&canonical.join("tide-music")), None);
        assert_eq!(
            current_bundle(Path::new(
                "/Volumes/Tide/Tide.app/Contents/MacOS/tide-music"
            )),
            None
        );
        let other = canonical.join("Tide/Contents/MacOS/tide-music");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        assert_eq!(current_bundle(&other), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Stages a real release image: TIDE_UPDATE_TEST_DMG=<Tide.dmg> and
    /// TIDE_UPDATE_TEST_VERSION=<its version>.
    #[test]
    #[ignore]
    fn real_release_image_stages_without_quarantine() {
        let image = std::path::PathBuf::from(std::env::var_os("TIDE_UPDATE_TEST_DMG").unwrap());
        let version = std::env::var("TIDE_UPDATE_TEST_VERSION").unwrap();
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("tide-mac-stage-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let copy = root.join("Tide.dmg");
        std::fs::copy(&image, &copy).unwrap();
        let staged = stage(&copy, &version).unwrap();
        assert_eq!(staged, root.join("staged/Tide.app"));
        assert!(!root.join("mount").exists(), "image must be detached");
        let quarantine = Command::new("/usr/bin/xattr")
            .args(["-r", "-p", "com.apple.quarantine"])
            .arg(&staged)
            .output()
            .unwrap();
        assert!(
            quarantine.stdout.is_empty(),
            "staged copy must not be quarantined"
        );
        assert!(check_bundle(&staged, "0.0.1").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn staged_bundle_must_be_the_offered_tide_release() {
        // The temporary folder is reached through /var -> /private/var.
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("tide-mac-check-{}", std::process::id()));
        let app = root.join("Tide.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        let plist = |id: &str, version: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"#
            )
        };
        std::fs::write(
            app.join("Contents/Info.plist"),
            plist("com.example.Other", "9.9.9"),
        )
        .unwrap();
        assert!(check_bundle(&app, "9.9.9")
            .unwrap_err()
            .contains("not the offered"));
        std::fs::write(app.join("Contents/Info.plist"), plist(BUNDLE_ID, "9.9.8")).unwrap();
        assert!(check_bundle(&app, "9.9.9")
            .unwrap_err()
            .contains("not the offered"));
        // Right identity, but an unsigned bundle never passes.
        std::fs::write(app.join("Contents/Info.plist"), plist(BUNDLE_ID, "9.9.9")).unwrap();
        assert!(check_bundle(&app, "9.9.9")
            .unwrap_err()
            .contains("code-signature"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
