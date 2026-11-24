//! Linux implementations of the shared platform API.
mod media;
mod x11;

pub use media::{start, MediaSession};

#[cfg(test)]
mod tests {
    #[test]
    fn launcher_quotes_the_appimage_and_marks_ownership() {
        let entry = super::desktop_entry("/home/u/Apps/Tide x86_64.AppImage");
        assert!(entry.contains("Exec=\"/home/u/Apps/Tide x86_64.AppImage\"\n"));
        assert!(entry.contains("X-Tide-AppImage=true"));
        assert!(entry.contains("StartupWMClass=tide-music"));
    }
}

use std::ffi::c_void;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Once;
use std::time::Duration;

/// Start a desktop helper (xdg-open, a file manager) without inheriting
/// Tide's AppImage library paths, and reap it so no zombie is left behind.
fn spawn_detached(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in ["LD_LIBRARY_PATH", "LD_PRELOAD", "PYTHONHOME", "PYTHONPATH"] {
        match std::env::var_os(format!("TIDE_HOST_{name}")) {
            Some(value) if !value.is_empty() => command.env(name, value),
            _ => command.env_remove(name),
        };
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Open a web page with the desktop's default browser (`xdg-open`).
pub fn open_web_page(url: &str) -> Result<(), String> {
    if !url.starts_with("https://")
        || url.len() > 2048
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("Only HTTPS web pages can be opened".into());
    }
    spawn_detached("xdg-open", &[url.as_ref()]).map_err(|_| {
        "Could not open your default browser. Install xdg-utils or set a default browser.".into()
    })
}

/// Touchpad gesture routing is a Windows Settings feature.
pub fn open_touchpad_settings() -> Result<(), String> {
    Err("Touchpad settings are available on Windows".into())
}

/// Select the file in the desktop file manager, or open its folder.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let url = media::file_url(path);
    let shown = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build())
        .and_then(|conn| {
            conn.call_method(
                Some("org.freedesktop.FileManager1"),
                "/org/freedesktop/FileManager1",
                Some("org.freedesktop.FileManager1"),
                "ShowItems",
                &(vec![url.as_str()], ""),
            )
        });
    if shown.is_ok() {
        return Ok(());
    }
    let folder = path.parent().unwrap_or(path);
    spawn_detached("xdg-open", &[folder.as_os_str()])
}

/// Read one XDG desktop portal setting, accepting both the current `ReadOne`
/// and the older `Read` (which wraps the value in a second variant).
fn portal_setting(namespace: &str, key: &str) -> Option<zbus::zvariant::OwnedValue> {
    use zbus::zvariant::{OwnedValue, Value};
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.method_timeout(Duration::from_secs(1)).build())
        .ok()?;
    let call = |method: &str| {
        conn.call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            method,
            &(namespace, key),
        )
        .ok()
        .and_then(|reply| reply.body().deserialize::<OwnedValue>().ok())
    };
    let value = call("ReadOne").or_else(|| call("Read"))?;
    match &*value {
        Value::Value(inner) => OwnedValue::try_from(&**inner).ok(),
        _ => Some(value),
    }
}

fn read_color_scheme() -> u8 {
    // org.freedesktop.appearance color-scheme: 0 no preference, 1 dark, 2 light.
    match portal_setting("org.freedesktop.appearance", "color-scheme")
        .and_then(|value| u32::try_from(value).ok())
    {
        Some(1) => 1,
        Some(2) => 2,
        _ => 0,
    }
}

static COLOR_SCHEME: AtomicU8 = AtomicU8::new(0);

/// Desktop light/dark preference. The UI polls this, so a background thread
/// refreshes the cached answer instead of making D-Bus calls on the UI thread.
pub fn app_light_theme() -> Option<bool> {
    static WATCH: Once = Once::new();
    WATCH.call_once(|| {
        COLOR_SCHEME.store(read_color_scheme(), Ordering::Release);
        let _ = std::thread::Builder::new()
            .name("tide-appearance".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(2));
                COLOR_SCHEME.store(read_color_scheme(), Ordering::Release);
            });
    });
    match COLOR_SCHEME.load(Ordering::Acquire) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// The local wall-clock hour (0-23), used for the Home greeting.
pub fn local_hour() -> Option<u8> {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&now, &mut local) }.is_null() {
        return None;
    }
    u8::try_from(local.tm_hour).ok().filter(|hour| *hour < 24)
}

/// Honour the desktop's reduced-motion / animations preference at startup.
pub fn client_animations_enabled() -> bool {
    if let Some(reduced) = portal_setting("org.freedesktop.appearance", "reduced-motion")
        .and_then(|value| u32::try_from(value).ok())
    {
        return reduced == 0;
    }
    portal_setting("org.gnome.desktop.interface", "enable-animations")
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(true)
}

/// Current monitor work area, excluding panels. Defer fitting while minimized.
pub fn window_work_area(view: *mut c_void) -> Result<[i32; 4], String> {
    x11::shared()?.work_area(x11::window(view)?)
}

/// Fit this application's window after startup, a mode, monitor or DPI change.
/// Includes the window frame; never activates or raises the window.
pub fn fit_window_to_work_area(view: *mut c_void, work: [i32; 4]) -> Result<(), String> {
    x11::shared()?.fit(x11::window(view)?, work)
}

/// PipeWire and PulseAudio move existing streams to a new default output
/// themselves, so Tide needs no output-change recovery on Linux.
pub struct OutputWatcher;

impl OutputWatcher {
    pub fn new() -> Result<Self, String> {
        Err("The sound server follows the default output".into())
    }

    pub fn take_available(&self) -> Option<bool> {
        None
    }
}

/// AppImage desktop integration, the counterpart of the Windows Start menu
/// shortcut: a `tide-music.desktop` launcher and icon under `~/.local/share`
/// that start this AppImage. Refreshed when the AppImage moves; a launcher
/// the user (or a distribution package) wrote is left alone.
pub fn integrate_desktop_entry() -> Result<(), String> {
    let Some(image) = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    let image = image
        .to_str()
        .filter(|path| image.is_absolute() && !path.contains(['\n', '"', '`', '$', '\\']))
        .ok_or("The AppImage path cannot be used in a launcher")?
        .to_owned();
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .ok_or("No user data folder")?;
    let launcher = data.join("applications/tide-music.desktop");
    let entry = desktop_entry(&image);
    match std::fs::read_to_string(&launcher) {
        Ok(existing) if existing == entry => return Ok(()),
        Ok(existing) if !existing.contains("X-Tide-AppImage=true") => return Ok(()),
        _ => {}
    }
    let icon = data.join("icons/hicolor/256x256/apps/tide-music.png");
    write_atomic(&icon, media::ICON_PNG)?;
    write_atomic(&launcher, entry.as_bytes())?;
    // Ask menus to notice the new launcher; harmless if the tool is absent.
    let _ = spawn_detached(
        "update-desktop-database",
        &[launcher.parent().unwrap_or(&launcher).as_os_str()],
    );
    Ok(())
}

fn desktop_entry(image: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Tide Music\nGenericName=Music Player\n\
         Comment=A lightweight YouTube Music player\nExec=\"{image}\"\nIcon=tide-music\n\
         Terminal=false\nCategories=AudioVideo;Audio;Player;\nKeywords=music;youtube;player;\n\
         StartupWMClass=tide-music\nX-Tide-AppImage=true\n"
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let folder = path.parent().ok_or("Invalid path")?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let temporary = folder.join(format!(".tide-{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        e.to_string()
    })
}

pub mod mini {
    //! Native mini-player pinning. Never steals focus; full mode restores opacity.
    use std::ffi::c_void;

    #[derive(Default)]
    pub struct MiniWindow {
        translucent: bool,
    }

    impl MiniWindow {
        pub fn apply(
            &mut self,
            view: *mut c_void,
            pinned: bool,
            opacity: u8,
        ) -> Result<(), String> {
            let x = super::x11::shared()?;
            let window = super::x11::window(view)?;
            x.set_above(window, pinned)?;
            if opacity < 255 || self.translucent {
                x.set_opacity(
                    window,
                    if opacity == 255 {
                        255
                    } else {
                        opacity.max(153)
                    },
                )?;
                self.translucent = opacity < 255;
            }
            Ok(())
        }
    }
}
