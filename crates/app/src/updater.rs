//! Published stable releases only. No Google sessions or browser state enter this module.
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const API: &str = "https://api.github.com/repos/yotam1001/tide-music/releases/latest";
const MAX_INSTALLER: u64 = 96 * 1024 * 1024;
const CACHE_MARKER: &str = "Tide updater cache v1\n";

#[derive(Clone, Debug)]
pub(super) struct Release {
    version: Version,
    url: String,
    digest: String,
    size: u64,
}
#[derive(Deserialize)]
struct Metadata {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

fn parse_release(bytes: &[u8], current: &str) -> Result<Option<Release>, String> {
    let metadata: Metadata =
        serde_json::from_slice(bytes).map_err(|_| "Invalid release metadata")?;
    let tag = &metadata.tag_name;
    let version = Version::parse(tag.strip_prefix('v').ok_or("Invalid release version")?)
        .map_err(|_| "Invalid release version")?;
    if metadata.draft || metadata.prerelease || !version.pre.is_empty() {
        return Ok(None);
    }
    if !version.build.is_empty() || tag != &format!("v{version}") {
        return Err("Invalid stable release tag".into());
    }
    if version <= Version::parse(current).map_err(|_| "Invalid installed version")? {
        return Ok(None);
    }
    let mut assets = metadata
        .assets
        .into_iter()
        .filter(|a| a.name == "TideSetup.exe");
    let asset = assets
        .next()
        .ok_or("This release has no Windows installer")?;
    if assets.next().is_some()
        || asset.state != "uploaded"
        || !(1..=MAX_INSTALLER).contains(&asset.size)
    {
        return Err("Invalid installer asset".into());
    }
    let expected_url =
        format!("https://github.com/yotam1001/tide-music/releases/download/{tag}/TideSetup.exe");
    let digest = asset
        .digest
        .and_then(|s| s.strip_prefix("sha256:").map(str::to_owned))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("This release has no verified installer checksum")?;
    if asset.browser_download_url != expected_url {
        return Err("Unexpected installer source".into());
    }
    Ok(Some(Release {
        version,
        url: expected_url,
        digest: digest.to_lowercase(),
        size: asset.size,
    }))
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(timeout)))
        .max_idle_connections(0)
        .max_response_header_size(16 * 1024)
        .build()
        .into()
}

pub(super) fn check() -> Result<Option<Release>, String> {
    let mut response = agent(10)
        .get(API)
        .header("User-Agent", concat!("Tide/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|_| "Could not reach GitHub. Try again later.")?;
    match response.status().as_u16() {
        404 => return Ok(None),
        200 => {}
        403 | 429 => return Err("GitHub's update limit was reached. Try again later.".into()),
        _ => return Err("The update service is temporarily unavailable.".into()),
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read release information")?;
    if bytes.len() > 256 * 1024 {
        return Err("Release information is too large".into());
    }
    parse_release(&bytes, env!("CARGO_PKG_VERSION"))
}

fn allowed_download(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && u.fragment().is_none()
            && matches!(
                u.host_str(),
                Some(
                    "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            )
    })
}

fn plain(path: &Path) -> Result<(), String> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(metadata) => {
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let linked = metadata.file_type().is_symlink();
                if linked {
                    return Err("Updates cannot use a linked folder or file".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Could not inspect the update folder".into()),
        }
    }
    Ok(())
}

fn cache_directory(base: &Path) -> Result<PathBuf, String> {
    let directory = base.join("TideMusic/updates");
    if !directory.is_absolute() {
        return Err("No local update folder is available".into());
    }
    plain(&directory)?;
    fs::create_dir_all(&directory).map_err(|_| "Could not create the update folder")?;
    let marker = directory.join(".tide-updater");
    plain(&marker)?;
    if !marker.exists() {
        if fs::read_dir(&directory)
            .map_err(|_| "Could not inspect the update folder")?
            .next()
            .is_some()
        {
            return Err("The update folder contains unrecognized files".into());
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .and_then(|mut f| f.write_all(CACHE_MARKER.as_bytes()))
            .map_err(|_| "Could not mark the update folder")?;
    }
    let mut content = String::new();
    File::open(marker)
        .and_then(|f| f.take(64).read_to_string(&mut content))
        .map_err(|_| "Could not read the update folder marker")?;
    if content != CACHE_MARKER {
        return Err("The update folder is not owned by Tide".into());
    }
    Ok(directory)
}

fn copy_verified(
    input: &mut impl Read,
    output: &mut impl Write,
    release: &Release,
) -> Result<(), String> {
    let mut sha = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| "The installer download was interrupted")?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > release.size || size > MAX_INSTALLER {
            return Err("The installer is larger than expected".into());
        }
        sha.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|_| "Could not save the installer. Check free disk space.")?;
    }
    if size != release.size || format!("{:x}", sha.finalize()) != release.digest {
        return Err("Installer verification failed. Nothing was installed; try again.".into());
    }
    Ok(())
}

struct PartialDownload(PathBuf);
impl Drop for PartialDownload {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn download(release: &Release, base: &Path) -> Result<PathBuf, String> {
    let agent = agent(180);
    let mut url = release.url.clone();
    let mut response = None;
    for _ in 0..4 {
        if !allowed_download(&url) {
            return Err("Unexpected installer download destination".into());
        }
        let candidate = agent
            .get(&url)
            .header("User-Agent", "Tide-Updater")
            .call()
            .map_err(|_| "Could not download the installer. Check your connection.")?;
        if candidate.status().as_u16() == 200 {
            response = Some(candidate);
            break;
        }
        if !matches!(candidate.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            return Err("The installer download is unavailable".into());
        }
        url = candidate
            .headers()
            .get("location")
            .and_then(|h| h.to_str().ok())
            .ok_or("Invalid download redirect")?
            .to_owned();
    }
    let mut response = response.ok_or("Too many installer download redirects")?;
    stage_installer(release, base, &mut response.body_mut().as_reader())
}

fn stage_installer(
    release: &Release,
    base: &Path,
    reader: &mut impl Read,
) -> Result<PathBuf, String> {
    let directory = cache_directory(base)?;
    let partial = directory.join("TideSetup.part");
    let destination = directory.join("TideSetup.exe");
    let lock = directory.join(".update-lock");
    for path in [&partial, &destination, &lock] {
        plain(path)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let _lock = options
        .open(lock)
        .map_err(|_| "Another Tide update is already using the download folder")?;
    // Only two fixed files in an owned cache: current installer + partial, each <=96 MiB.
    let mut output =
        File::create(&partial).map_err(|_| "Could not create the installer download")?;
    let cleanup = PartialDownload(partial.clone());
    copy_verified(reader, &mut output, release)?;
    output
        .sync_all()
        .map_err(|_| "Could not finish saving the installer")?;
    drop(output);
    fs::rename(&partial, &destination)
        .map_err(|_| "Close the previous Tide installer and retry")?;
    drop(cleanup);
    Ok(destination)
}

fn installed_player(base: &Path, executable: &Path) -> bool {
    let root = base.join("Programs/Tide");
    let record_path = root.join(".tide-install.json");
    if plain(&record_path).is_err() || plain(executable).is_err() {
        return false;
    }
    let Ok(file) = File::open(record_path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take(64 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 64 * 1024 {
        return false;
    }
    let Ok(record) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let Some(release) = record["Release"].as_str() else {
        return false;
    };
    record["Product"] == "org.tidemusic.windows.preview"
        && release.len() == 32
        && release.bytes().all(|b| b.is_ascii_hexdigit())
        && executable == root.join("releases").join(release).join("tide-music.exe")
}

fn launch(path: &Path, release: &Release) -> Result<(), String> {
    plain(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Deny writes/deletion while hashing and starting the Windows image.
        options.share_mode(1);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Could not open the verified installer")?;
    copy_verified(&mut file, &mut std::io::sink(), release)?;
    std::process::Command::new(path)
        .arg("--update-from")
        .arg(std::process::id().to_string())
        .current_dir(path.parent().ok_or("Invalid installer path")?)
        .spawn()
        .map_err(|_| "Windows could not start setup. Your player is still running.")?;
    Ok(())
}

enum Completion {
    Checked(Result<Option<Release>, String>),
    Downloaded(Result<PathBuf, String>),
}
pub(super) struct Updater {
    timer: slint::Timer,
    receiver: Option<mpsc::Receiver<Completion>>,
    available: Option<Release>,
    last_check: Option<Instant>,
    base: Option<PathBuf>,
}
impl Updater {
    fn start(owner: &Rc<RefCell<Self>>, ui: &super::AppWindow, install: bool) {
        let mut state = owner.borrow_mut();
        if state.receiver.is_some() {
            return;
        }
        if !install
            && state
                .last_check
                .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let candidate = state.available.clone();
        let base = state.base.clone();
        if install && (candidate.is_none() || base.is_none()) {
            return;
        }
        let task = std::thread::Builder::new()
            .name("tide-update".into())
            .spawn(move || {
                let completion = if install {
                    Completion::Downloaded(download(&candidate.unwrap(), &base.unwrap()))
                } else {
                    Completion::Checked(check())
                };
                let _ = sender.send(completion);
            });
        if task.is_err() {
            ui.set_update_status("Could not start the update check".into());
            return;
        }
        if !install {
            state.last_check = Some(Instant::now());
        }
        state.receiver = Some(receiver);
        ui.set_update_busy(true);
        ui.set_update_status(
            if install {
                "Downloading and verifying update…"
            } else {
                "Checking GitHub for updates…"
            }
            .into(),
        );
        let weak = ui.as_weak();
        let owner = Rc::downgrade(owner);
        state.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(200),
            move || {
                if let (Some(owner), Some(ui)) = (owner.upgrade(), weak.upgrade()) {
                    owner.borrow_mut().poll(&ui);
                }
            },
        );
    }
    fn poll(&mut self, ui: &super::AppWindow) {
        let result = match self.receiver.as_ref().unwrap().try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Completion::Checked(Err("The update check stopped. Try again later.".into())),
        };
        self.receiver = None;
        self.timer.stop();
        ui.set_update_busy(false);
        match result {
            Completion::Checked(Ok(candidate)) => {
                let status = candidate
                    .as_ref()
                    .map(|r| format!("Tide {} is available", r.version))
                    .unwrap_or_else(|| "No newer stable release is available".into());
                self.available = candidate;
                ui.set_update_available(self.available.is_some());
                ui.set_update_status(status.into());
            }
            Completion::Checked(Err(error)) | Completion::Downloaded(Err(error)) => {
                ui.set_update_status(error.into())
            }
            Completion::Downloaded(Ok(path)) => {
                match launch(&path, self.available.as_ref().unwrap()) {
                    Ok(()) => {
                        let _ = slint::quit_event_loop();
                    }
                    Err(error) => ui.set_update_status(error.into()),
                }
            }
        }
    }
}

pub(super) fn wire(ui: &super::AppWindow, enabled: bool) -> Rc<RefCell<Updater>> {
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let installable = cfg!(windows)
        && base.as_ref().is_some_and(|base| {
            std::env::current_exe().is_ok_and(|exe| installed_player(base, &exe))
        });
    ui.set_update_installable(installable);
    let owner = Rc::new(RefCell::new(Updater {
        timer: slint::Timer::default(),
        receiver: None,
        available: None,
        last_check: None,
        base,
    }));
    let state = owner.clone();
    let weak = ui.as_weak();
    ui.on_check_updates(move || {
        if let Some(ui) = weak.upgrade() {
            Updater::start(&state, &ui, false);
        }
    });
    let state = owner.clone();
    let weak = ui.as_weak();
    ui.on_install_update(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_update_installable() {
                Updater::start(&state, &ui, true);
            }
        }
    });
    if enabled && installable && ui.get_auto_update_checks() {
        let state = Rc::downgrade(&owner);
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_secs(15), move || {
            if let (Some(state), Some(ui)) = (state.upgrade(), weak.upgrade()) {
                if ui.get_auto_update_checks() {
                    Updater::start(&state, &ui, false);
                }
            }
        });
    }
    owner
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata(tag: &str) -> serde_json::Value {
        json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": [{
            "name": "TideSetup.exe", "state": "uploaded", "size": 4,
            "digest": format!("sha256:{:x}", Sha256::digest(b"Tide")),
            "browser_download_url": format!("https://github.com/yotam1001/tide-music/releases/download/{tag}/TideSetup.exe")
        }]})
    }
    fn parse(value: &serde_json::Value) -> Result<Option<Release>, String> {
        parse_release(&serde_json::to_vec(value).unwrap(), "0.1.0")
    }
    #[test]
    fn only_strictly_newer_stable_versions_are_offered() {
        for tag in ["v0.1.0", "v0.0.9", "v0.2.0-beta.1"] {
            assert!(parse(&metadata(tag)).unwrap().is_none());
        }
        assert_eq!(
            parse(&metadata("v0.2.0"))
                .unwrap()
                .unwrap()
                .version
                .to_string(),
            "0.2.0"
        );
        for installed in ["0.1.0-beta.1", "0.1.0-preview.1", "0.1.0-preview.2"] {
            assert!(
                parse_release(&serde_json::to_vec(&metadata("v0.1.0")).unwrap(), installed)
                    .unwrap()
                    .is_some()
            );
        }
        for flag in ["draft", "prerelease"] {
            let mut value = metadata("v0.2.0");
            value[flag] = json!(true);
            assert!(parse(&value).unwrap().is_none());
        }
        for tag in ["0.2.0", "v0.2.0+build", "v01.2.0", "v0.2.0/../../evil"] {
            assert!(parse(&metadata(tag)).is_err());
        }
    }
    #[test]
    fn asset_identity_digest_size_and_uniqueness_are_required() {
        for (field, value) in [
            ("name", json!("OtherSetup.exe")),
            ("state", json!("new")),
            ("size", json!(0)),
            ("size", json!(MAX_INSTALLER + 1)),
            ("digest", json!(null)),
            ("digest", json!("sha256:unverified")),
            (
                "browser_download_url",
                json!("https://github.com/other/project/releases/download/v0.2.0/TideSetup.exe"),
            ),
        ] {
            let mut data = metadata("v0.2.0");
            data["assets"][0][field] = value;
            assert!(parse(&data).is_err(), "{field}");
        }
        let mut data = metadata("v0.2.0");
        let duplicate = data["assets"][0].clone();
        data["assets"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&data).is_err());
    }
    #[test]
    fn redirects_cannot_leak_requests_to_untrusted_hosts_or_cleartext() {
        for url in [
            "http://github.com/file",
            "https://github.com.evil.example/file",
            "https://evil.example/file",
            "https://user@github.com/file",
            "https://github.com:444/file",
            "file:///TideSetup.exe",
            "https://github.com/file#fragment",
        ] {
            assert!(!allowed_download(url), "{url}");
        }
        assert!(allowed_download(
            "https://release-assets.githubusercontent.com/asset?signature=fixture"
        ));
    }
    #[test]
    fn streaming_verification_rejects_truncation_extra_bytes_and_tampering() {
        let release = parse(&metadata("v0.2.0")).unwrap().unwrap();
        let mut output = Vec::new();
        copy_verified(&mut &b"Tide"[..], &mut output, &release).unwrap();
        assert_eq!(output, b"Tide");
        for bytes in [b"Tid".as_slice(), b"Tide!", b"Fake"] {
            let mut input = bytes;
            assert!(copy_verified(&mut input, &mut Vec::new(), &release).is_err());
        }
    }
    #[test]
    fn cache_requires_ownership_and_uses_fixed_names() {
        let root = tempfile::tempdir().unwrap();
        let directory = cache_directory(root.path()).unwrap();
        assert_eq!(directory, cache_directory(root.path()).unwrap());
        fs::write(directory.join(".tide-updater"), "foreign").unwrap();
        assert!(cache_directory(root.path()).is_err());
        let foreign = tempfile::tempdir().unwrap();
        fs::create_dir_all(foreign.path().join("TideMusic/updates")).unwrap();
        let sentinel = foreign.path().join("TideMusic/updates/keep.txt");
        fs::write(&sentinel, "keep").unwrap();
        assert!(cache_directory(foreign.path()).is_err());
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep");
    }
    #[test]
    fn failed_download_preserves_ready_installer_and_removes_its_partial_file() {
        let base = tempfile::tempdir().unwrap();
        let release = parse(&metadata("v0.2.0")).unwrap().unwrap();
        let setup = stage_installer(&release, base.path(), &mut &b"Tide"[..]).unwrap();
        assert!(stage_installer(&release, base.path(), &mut &b"Fake"[..]).is_err());
        assert_eq!(fs::read(&setup).unwrap(), b"Tide");
        assert!(!setup.with_file_name("TideSetup.part").exists());
        stage_installer(&release, base.path(), &mut &b"Tide"[..]).unwrap();
        assert_eq!(fs::read_dir(setup.parent().unwrap()).unwrap().count(), 3);
    }
    #[test]
    fn automatic_installation_is_confined_to_the_active_owned_install() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("Programs/Tide");
        fs::create_dir_all(&root).unwrap();
        let id = "abcdef0123456789abcdef0123456789";
        fs::write(
            root.join(".tide-install.json"),
            json!({"Product":"org.tidemusic.windows.preview", "Release":id}).to_string(),
        )
        .unwrap();
        let exe = root.join("releases").join(id).join("tide-music.exe");
        assert!(installed_player(base.path(), &exe));
        assert!(!installed_player(
            base.path(),
            &base.path().join("development/tide-music.exe")
        ));
        fs::write(
            root.join(".tide-install.json"),
            json!({"Product":"org.tidemusic.windows.preview", "Release":"../../foreign"})
                .to_string(),
        )
        .unwrap();
        assert!(!installed_player(base.path(), &exe));
    }

    #[cfg(windows)]
    #[test]
    fn verified_handoff_starts_only_the_expected_bytes_with_the_parent_pid() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("fixture.cs");
        let exe = root.path().join("TideSetup.exe");
        let receipt = root.path().join("TideSetup.exe.receipt");
        // Publish only a complete receipt so the reader cannot observe
        // File.WriteAllText's newly created, but still empty, output file.
        fs::write(
            &source,
            r#"class Fixture { static void Main(string[] args) {
            var receipt = System.Reflection.Assembly.GetExecutingAssembly().Location + ".receipt";
            System.IO.File.WriteAllText(receipt + ".tmp", System.String.Join("|", args));
            System.IO.File.Move(receipt + ".tmp", receipt);
        } }"#,
        )
        .unwrap();
        let compiler = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
        let result = std::process::Command::new(compiler)
            .arg("/nologo")
            .arg("/target:winexe")
            .arg(format!("/out:{}", exe.display()))
            .arg(source)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        let bytes = fs::read(&exe).unwrap();
        let release = Release {
            version: Version::parse("0.2.0").unwrap(),
            url: String::new(),
            digest: format!("{:x}", Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        };
        let mut tampered = bytes.clone();
        *tampered.last_mut().unwrap() ^= 1;
        fs::write(&exe, tampered).unwrap();
        assert!(launch(&exe, &release).is_err());
        assert!(!receipt.exists());
        fs::write(&exe, bytes).unwrap();
        launch(&exe, &release).unwrap();
        let started = Instant::now();
        let actual = loop {
            match fs::read_to_string(&receipt) {
                Ok(actual) => break actual,
                Err(error) => {
                    assert!(
                        started.elapsed() < Duration::from_secs(15)
                            && matches!(
                                error.kind(),
                                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                            ),
                        "Could not read updater test receipt: {error}"
                    );
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        assert_eq!(actual, format!("--update-from|{}", std::process::id()));
    }
}
