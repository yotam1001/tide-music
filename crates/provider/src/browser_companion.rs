//! macOS and Linux Browser Companion native-messaging host registration and status.
//! Keeps all mutable registration and policy state outside the signed Tide.app
//! bundle and the read-only Linux AppImage.
use std::path::{Path, PathBuf};

pub const HOST_NAME: &str = "org.tidemusic.session";
pub const FIREFOX_EXTENSION_ID: &str = "tide-session@tide-music";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacBrowser {
    Chrome,
    Edge,
    Brave,
    Arc,
    Firefox,
}

impl MacBrowser {
    pub const ALL: [Self; 5] = [
        Self::Chrome,
        Self::Edge,
        Self::Brave,
        Self::Arc,
        Self::Firefox,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Brave => "brave",
            Self::Arc => "arc",
            Self::Firefox => "firefox",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
            // Arc has no Linux build; that slot registers Chromium there.
            Self::Arc if cfg!(target_os = "linux") => "Chromium",
            Self::Arc => "Arc",
            Self::Firefox => "Firefox / Zen",
        }
    }

    pub fn is_chromium(self) -> bool {
        !matches!(self, Self::Firefox)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "chrome" => Some(Self::Chrome),
            "edge" => Some(Self::Edge),
            "brave" => Some(Self::Brave),
            "arc" | "chromium" => Some(Self::Arc),
            "firefox" | "zen" => Some(Self::Firefox),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompanionHostStatus {
    pub host_path: Option<PathBuf>,
    pub extension_dir: Option<PathBuf>,
    pub registered_browsers: Vec<MacBrowser>,
    pub allowed_extension_ids: Vec<String>,
}

pub fn validate_chromium_extension_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    let stripped = trimmed
        .strip_prefix("chrome-extension://")
        .unwrap_or(&trimmed);
    let id = stripped.strip_suffix('/').unwrap_or(stripped);
    if id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
        Ok(id.to_owned())
    } else {
        Err("Enter the 32-letter extension ID from chrome://extensions (letters a–p).".into())
    }
}

pub fn host_executable_beside(app_exe: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "tide-session-host.exe"
    } else {
        "tide-session-host"
    };
    let candidate = app_exe.parent()?.join(name);
    let meta = std::fs::symlink_metadata(&candidate).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(candidate)
}

pub fn bundled_extension_dir(app_exe: &Path) -> Option<PathBuf> {
    let exe_dir = app_exe.parent()?;
    // Tide.app/Contents/MacOS -> ../Resources/browser-companion, and the
    // AppImage's usr/bin -> ../share/tide-music/browser-companion.
    [
        exe_dir.parent()?.join("Resources/browser-companion"),
        exe_dir.parent()?.join("share/tide-music/browser-companion"),
    ]
    .into_iter()
    .find(|dir| is_valid_extension_root(dir))
}

fn is_valid_extension_root(dir: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return false;
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return false;
    }
    dir.join("chromium/manifest.json").is_file() || dir.join("firefox/manifest.json").is_file()
}

#[cfg(not(unix))]
pub fn status(app_exe: &Path) -> CompanionHostStatus {
    CompanionHostStatus {
        host_path: host_executable_beside(app_exe),
        extension_dir: bundled_extension_dir(app_exe),
        registered_browsers: Vec::new(),
        allowed_extension_ids: Vec::new(),
    }
}

#[cfg(not(unix))]
pub fn register_macos_browser(
    _app_exe: &Path,
    _browser: MacBrowser,
    _chromium_extension_id: Option<&str>,
) -> Result<String, String> {
    Err("In-app browser companion registration is available on macOS and Linux.".into())
}

#[cfg(not(unix))]
pub fn unregister_macos_browser(_browser: MacBrowser) -> Result<bool, String> {
    Err("In-app browser companion removal is available on macOS and Linux.".into())
}

#[cfg(unix)]
pub use macos_impl::{register_macos_browser, status, unregister_macos_browser};

/// Shared by macOS and Linux; only the folders differ.
#[cfg(unix)]
mod macos_impl {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::{
        fs,
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    const POLICY_FILE: &str = "tide-session-host-policy.json";
    const MAX_ALLOWED_ORIGINS: usize = 16;
    const MAX_POLICY_BYTES: usize = 2048;

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct HostPolicy {
        allowed_origins: Vec<String>,
    }

    #[derive(Serialize)]
    struct ChromiumManifest {
        name: &'static str,
        description: &'static str,
        path: String,
        #[serde(rename = "type")]
        kind: &'static str,
        allowed_origins: Vec<String>,
    }

    #[derive(Serialize)]
    struct FirefoxManifest {
        name: &'static str,
        description: &'static str,
        path: String,
        #[serde(rename = "type")]
        kind: &'static str,
        allowed_extensions: [&'static str; 1],
    }

    fn home_dir() -> Result<PathBuf, String> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| "Could not determine your home folder.".into())
    }

    /// Per-user Tide folder that holds the host policy (and on Linux a stable
    /// copy of the host, since the AppImage mount path changes every launch).
    fn user_dir(home: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/TideMusic")
        } else {
            home.join(".local/share/TideMusic")
        }
    }

    fn policy_path(home: &Path) -> PathBuf {
        user_dir(home).join(POLICY_FILE)
    }

    fn manifest_path(home: &Path, browser: MacBrowser) -> PathBuf {
        let folder = if cfg!(target_os = "macos") {
            match browser {
                MacBrowser::Chrome => {
                    "Library/Application Support/Google/Chrome/NativeMessagingHosts"
                }
                MacBrowser::Edge => {
                    "Library/Application Support/Microsoft Edge/NativeMessagingHosts"
                }
                MacBrowser::Brave => {
                    "Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"
                }
                MacBrowser::Arc => "Library/Application Support/Arc/User Data/NativeMessagingHosts",
                MacBrowser::Firefox => "Library/Application Support/Mozilla/NativeMessagingHosts",
            }
        } else {
            match browser {
                MacBrowser::Chrome => ".config/google-chrome/NativeMessagingHosts",
                MacBrowser::Edge => ".config/microsoft-edge/NativeMessagingHosts",
                MacBrowser::Brave => ".config/BraveSoftware/Brave-Browser/NativeMessagingHosts",
                MacBrowser::Arc => ".config/chromium/NativeMessagingHosts",
                MacBrowser::Firefox => ".mozilla/native-messaging-hosts",
            }
        };
        home.join(folder).join(format!("{HOST_NAME}.json"))
    }

    /// Linux: copy the host out of the AppImage to a stable per-user path that
    /// browser manifests can keep pointing at across launches and updates.
    #[cfg(not(target_os = "macos"))]
    fn stable_host(home: &Path, bundled: &Path) -> Result<PathBuf, String> {
        let bin = user_dir(home).join("bin");
        reject_symlink_in_ancestors(&bin, home)?;
        fs::create_dir_all(&bin).map_err(|_| "Could not create Tide's helper folder.")?;
        let target = bin.join("tide-session-host");
        let bytes = fs::read(bundled).map_err(|_| "Could not read tide-session-host.")?;
        if fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
            atomic_write(&target, &bytes, 0o755)?;
        }
        Ok(target)
    }

    fn reject_symlink_in_ancestors(target: &Path, stop_at: &Path) -> Result<(), String> {
        let mut current = target;
        loop {
            match fs::symlink_metadata(current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err("Refusing to write through a symlinked configuration path.".into());
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err("Could not inspect macOS browser configuration folder.".into());
                }
                _ => {}
            }
            if current == stop_at {
                break;
            }
            let Some(parent) = current.parent() else {
                break;
            };
            current = parent;
        }
        Ok(())
    }

    fn read_existing_origins(path: &Path) -> Result<Vec<String>, String> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err("Could not inspect existing browser companion policy.".into()),
        };
        if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_POLICY_BYTES as u64
        {
            return Err("Existing browser companion policy file is invalid.".into());
        }
        let data =
            fs::read(path).map_err(|_| "Could not read existing browser companion policy.")?;
        if data.len() > MAX_POLICY_BYTES {
            return Err("Existing browser companion policy exceeds size limit.".into());
        }
        let parsed: HostPolicy = serde_json::from_slice(&data)
            .map_err(|_| "Existing browser companion policy could not be parsed.")?;
        if parsed.allowed_origins.len() > MAX_ALLOWED_ORIGINS {
            return Err("Existing browser companion policy has too many origins.".into());
        }
        let mut origins = Vec::new();
        for origin in parsed.allowed_origins {
            let id = validate_chromium_extension_id(&origin)
                .map_err(|_| "Existing browser companion policy contains an invalid origin.")?;
            let normalized = format!("chrome-extension://{id}/");
            if !origins.contains(&normalized) {
                origins.push(normalized);
            }
        }
        Ok(origins)
    }

    fn ensure_tide_owned_manifest(path: &Path) -> Result<(), String> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Could not inspect existing native-messaging manifest.".into()),
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("Refusing to modify an unsafe native-messaging manifest.".into());
        }
        let data =
            fs::read(path).map_err(|_| "Could not read existing native-messaging manifest.")?;
        let parsed: serde_json::Value = serde_json::from_slice(&data)
            .map_err(|_| "Existing native-messaging manifest is unreadable.")?;
        if parsed.get("name").and_then(|v| v.as_str()) != Some(HOST_NAME) {
            return Err("Refusing to overwrite a manifest not owned by Tide.".into());
        }
        Ok(())
    }

    fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "Invalid native-messaging target path.".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|_| "Could not create browser native-messaging folder.")?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp = parent.join(format!(".tide-nmh-{}-{stamp}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temp)
            .map_err(|_| "Could not create temporary browser configuration file.")?;
        let write_result = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::set_permissions(&temp, fs::Permissions::from_mode(mode))?;
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err("Could not save browser native-messaging configuration.".into());
        }
        Ok(())
    }

    pub(super) fn status_in(home: &Path, app_exe: &Path) -> CompanionHostStatus {
        let host_path = host_executable_beside(app_exe);
        let extension_dir = bundled_extension_dir(app_exe);
        let allowed_extension_ids = read_existing_origins(&policy_path(home))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|origin| validate_chromium_extension_id(&origin).ok())
            .collect();
        let mut registered_browsers = Vec::new();
        for browser in MacBrowser::ALL {
            let path = manifest_path(home, browser);
            if ensure_tide_owned_manifest(&path).is_ok() && path.is_file() {
                registered_browsers.push(browser);
            }
        }
        // Keep the stable Linux host copy current after Tide updates.
        #[cfg(not(target_os = "macos"))]
        if !registered_browsers.is_empty() {
            if let Some(bundled) = &host_path {
                let _ = stable_host(home, bundled);
            }
        }
        CompanionHostStatus {
            host_path,
            extension_dir,
            registered_browsers,
            allowed_extension_ids,
        }
    }

    pub(super) fn register_in(
        home: &Path,
        app_exe: &Path,
        browser: MacBrowser,
        chromium_extension_id: Option<&str>,
    ) -> Result<String, String> {
        let host = host_executable_beside(app_exe).ok_or_else(|| {
            "Browser companion host (tide-session-host) was not found beside Tide.".to_owned()
        })?;
        #[cfg(not(target_os = "macos"))]
        let host = stable_host(home, &host)?;
        let host = host
            .canonicalize()
            .map_err(|_| "Could not resolve tide-session-host executable path.")?;
        let host_str = host
            .to_str()
            .ok_or_else(|| "Host executable path must be valid UTF-8.".to_owned())?
            .to_owned();

        let manifest_file = manifest_path(home, browser);
        reject_symlink_in_ancestors(&manifest_file, home)?;
        ensure_tide_owned_manifest(&manifest_file)?;

        if browser.is_chromium() {
            let raw_id = chromium_extension_id.ok_or_else(|| {
                format!(
                    "Enter your {} extension ID first (from chrome://extensions).",
                    browser.label()
                )
            })?;
            let id = validate_chromium_extension_id(raw_id)?;
            let origin = format!("chrome-extension://{id}/");
            let policy = policy_path(home);
            reject_symlink_in_ancestors(&policy, home)?;
            let mut origins = read_existing_origins(&policy)?;
            if !origins.contains(&origin) {
                if origins.len() >= MAX_ALLOWED_ORIGINS {
                    return Err(
                        "Maximum number of registered browser extension IDs reached.".into(),
                    );
                }
                origins.push(origin);
            }
            let mut policy_bytes = serde_json::to_vec_pretty(&HostPolicy {
                allowed_origins: origins.clone(),
            })
            .map_err(|_| "Could not encode browser companion policy.")?;
            policy_bytes.push(b'\n');
            if policy_bytes.len() > MAX_POLICY_BYTES {
                return Err("Browser companion policy exceeds size limit.".into());
            }
            atomic_write(&policy, &policy_bytes, 0o600)?;

            let mut manifest_bytes = serde_json::to_vec_pretty(&ChromiumManifest {
                name: HOST_NAME,
                description: "Tide YouTube Music session receiver",
                path: host_str,
                kind: "stdio",
                allowed_origins: origins,
            })
            .map_err(|_| "Could not encode browser manifest.")?;
            manifest_bytes.push(b'\n');
            atomic_write(&manifest_file, &manifest_bytes, 0o644)?;
            Ok(format!(
                "{} connected. Open music.youtube.com in {}, click Tide's extension, choose Import selected tab, then open Library.",
                browser.label(),
                browser.label()
            ))
        } else {
            let mut manifest_bytes = serde_json::to_vec_pretty(&FirefoxManifest {
                name: HOST_NAME,
                description: "Tide YouTube Music session receiver",
                path: host_str,
                kind: "stdio",
                allowed_extensions: [FIREFOX_EXTENSION_ID],
            })
            .map_err(|_| "Could not encode Firefox manifest.")?;
            manifest_bytes.push(b'\n');
            atomic_write(&manifest_file, &manifest_bytes, 0o644)?;
            Ok("Firefox / Zen connected. Open music.youtube.com, click Tide's extension, choose Import selected tab, then open Library.".into())
        }
    }

    pub(super) fn unregister_in(home: &Path, browser: MacBrowser) -> Result<bool, String> {
        let manifest_file = manifest_path(home, browser);
        reject_symlink_in_ancestors(&manifest_file, home)?;
        if !manifest_file.exists() {
            return Ok(false);
        }
        ensure_tide_owned_manifest(&manifest_file)?;
        fs::remove_file(&manifest_file)
            .map_err(|_| "Could not remove browser native-messaging manifest.")?;
        Ok(true)
    }

    pub fn status(app_exe: &Path) -> CompanionHostStatus {
        let Ok(home) = home_dir() else {
            return CompanionHostStatus {
                host_path: host_executable_beside(app_exe),
                extension_dir: bundled_extension_dir(app_exe),
                registered_browsers: Vec::new(),
                allowed_extension_ids: Vec::new(),
            };
        };
        status_in(&home, app_exe)
    }

    pub fn register_macos_browser(
        app_exe: &Path,
        browser: MacBrowser,
        chromium_extension_id: Option<&str>,
    ) -> Result<String, String> {
        let home = home_dir()?;
        register_in(&home, app_exe, browser, chromium_extension_id)
    }

    pub fn unregister_macos_browser(browser: MacBrowser) -> Result<bool, String> {
        let home = home_dir()?;
        unregister_in(&home, browser)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::symlink;

        fn temp_workspace(tag: &str) -> PathBuf {
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "tide-companion-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            root
        }

        #[test]
        fn registers_merges_and_unregisters_in_isolated_home() {
            let root = temp_workspace("reg");
            let home = root.join("home");
            let app_macos = root.join("Tide.app/Contents/MacOS");
            let ext_dir = root.join("Tide.app/Contents/Resources/browser-companion/chromium");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&app_macos).unwrap();
            fs::create_dir_all(&ext_dir).unwrap();
            let app_exe = app_macos.join("tide-music");
            let host_exe = app_macos.join("tide-session-host");
            fs::write(&app_exe, b"#!/bin/sh\n").unwrap();
            fs::write(&host_exe, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&host_exe, fs::Permissions::from_mode(0o755)).unwrap();
            fs::write(ext_dir.join("manifest.json"), b"{}").unwrap();

            let id1 = "a".repeat(32);
            let id2 = "b".repeat(32);
            assert!(register_in(&home, &app_exe, MacBrowser::Chrome, None).is_err());
            register_in(&home, &app_exe, MacBrowser::Chrome, Some(&id1)).unwrap();
            register_in(
                &home,
                &app_exe,
                MacBrowser::Edge,
                Some(&format!("chrome-extension://{}/", id2.to_uppercase())),
            )
            .unwrap();
            register_in(&home, &app_exe, MacBrowser::Firefox, None).unwrap();

            let st = status_in(&home, &app_exe);
            assert_eq!(st.host_path, Some(host_exe));
            assert!(st.extension_dir.is_some());
            assert_eq!(st.allowed_extension_ids, vec![id1, id2]);
            assert_eq!(
                st.registered_browsers,
                vec![MacBrowser::Chrome, MacBrowser::Edge, MacBrowser::Firefox]
            );

            assert!(unregister_in(&home, MacBrowser::Chrome).unwrap());
            assert!(!unregister_in(&home, MacBrowser::Chrome).unwrap());
            let st_after = status_in(&home, &app_exe);
            assert_eq!(
                st_after.registered_browsers,
                vec![MacBrowser::Edge, MacBrowser::Firefox]
            );
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn refuses_symlinks_and_foreign_manifests() {
            let root = temp_workspace("sec");
            let home = root.join("home");
            let app_macos = root.join("Tide.app/Contents/MacOS");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&app_macos).unwrap();
            let app_exe = app_macos.join("tide-music");
            let host_exe = app_macos.join("tide-session-host");
            fs::write(&app_exe, b"#!/bin/sh\n").unwrap();
            fs::write(&host_exe, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&host_exe, fs::Permissions::from_mode(0o755)).unwrap();

            let ff_manifest = manifest_path(&home, MacBrowser::Firefox);
            fs::create_dir_all(ff_manifest.parent().unwrap()).unwrap();
            fs::write(&ff_manifest, br#"{"name":"other.host"}"#).unwrap();
            assert!(register_in(&home, &app_exe, MacBrowser::Firefox, None).is_err());
            assert!(unregister_in(&home, MacBrowser::Firefox).is_err());

            let outside = root.join("outside.json");
            fs::write(&outside, b"{}").unwrap();
            let policy = policy_path(&home);
            fs::create_dir_all(policy.parent().unwrap()).unwrap();
            symlink(&outside, &policy).unwrap();
            let id = "a".repeat(32);
            assert!(register_in(&home, &app_exe, MacBrowser::Chrome, Some(&id)).is_err());
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(test)]
mod common_tests {
    use super::*;

    #[test]
    fn validates_chromium_extension_ids() {
        let valid = "abcdefghijklmnopabcdefghijklmnop";
        assert_eq!(validate_chromium_extension_id(valid).unwrap(), valid);
        assert_eq!(
            validate_chromium_extension_id(&format!("  CHROME-EXTENSION://{valid}/ ")).unwrap(),
            valid
        );
        for bad in ["", "short", &"q".repeat(32), "chrome-extension://*/"] {
            assert!(validate_chromium_extension_id(bad).is_err());
        }
    }
}
