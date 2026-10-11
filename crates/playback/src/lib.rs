use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tide_core::{PlaybackCommand, PlaybackSnapshot};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::sync::{mpsc, watch};
use url::Url;

mod prefetch;
#[cfg(windows)]
mod recovery;
use prefetch::{Preparation, PreparedStream};

#[cfg(windows)]
mod owned_process;
#[cfg(windows)]
pub use owned_process::OwnedProcess;

const COMMAND_CAPACITY: usize = 64;
const IPC_EVENT_CAPACITY: usize = 256;
const MAX_SOURCE_BYTES: usize = 8 * 1024;
const MAX_IPC_LINE_BYTES: usize = 256 * 1024;
const IPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_GRACE: Duration = Duration::from_millis(900);
const IPC_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
static PIPE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A cheap, thread-safe view of playback state.
///
/// The mpv process is started on the first `Load` command and is owned by the
/// worker. Dropping this handle asks the worker to quit and joins it, so an
/// owned mpv process cannot outlive the handle.
pub struct PlaybackHandle {
    command_tx: mpsc::Sender<WorkerCommand>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_requested: Arc<AtomicBool>,
    desired_generation: Arc<AtomicU64>,
    supersede_epoch: Arc<AtomicU64>,
    desired_volume: Arc<AtomicU64>,
    intent: Arc<Mutex<PlaybackIntent>>,
    snapshot: Arc<RwLock<PlaybackSnapshot>>,
    send_gate: Arc<Mutex<()>>,
    worker: Option<thread::JoinHandle<()>>,
    yt_dlp_available: bool,
    preparation: Arc<Preparation>,
}

/// Start the native playback worker. mpv is located at runtime but is not
/// launched until the first real track is loaded.
#[cfg(windows)]
pub fn start() -> Result<PlaybackHandle, String> {
    start_with_cookie_file(None)
}

/// Use a caller-owned, private cookie lease for signed-in YouTube playback.
/// The caller must keep it alive until this handle and its helpers are dropped.
#[cfg(windows)]
pub fn start_with_cookie_file(cookies: Option<PathBuf>) -> Result<PlaybackHandle, String> {
    start_with_account(cookies, 0)
}

#[cfg(windows)]
pub fn start_with_account(
    cookies: Option<PathBuf>,
    account_index: u32,
) -> Result<PlaybackHandle, String> {
    if account_index > 9 {
        return Err("Invalid signed-in account selection".into());
    }
    if cookies.as_ref().is_some_and(|path| !path.is_file()) {
        return Err("The signed-in session file is unavailable. Reconnect your account.".into());
    }
    let mut runtimes = RuntimePaths::discover()?;
    runtimes.cookies = cookies;
    runtimes.account_index = account_index;
    let yt_dlp_available = runtimes.yt_dlp.is_some();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|error| format!("Could not initialize the playback worker: {error}"))?;

    let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let snapshot = Arc::new(RwLock::new(PlaybackSnapshot {
        volume: 100.0,
        paused: true,
        ..PlaybackSnapshot::default()
    }));
    let desired_generation = Arc::new(AtomicU64::new(0));
    let supersede_epoch = Arc::new(AtomicU64::new(0));
    let desired_volume = Arc::new(AtomicU64::new(100.0_f64.to_bits()));
    let intent = Arc::new(Mutex::new(PlaybackIntent::default()));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let send_gate = Arc::new(Mutex::new(()));
    let (preparation, preparation_rx) = Preparation::new();

    let worker_snapshot = Arc::clone(&snapshot);
    let worker_generation = Arc::clone(&desired_generation);
    let worker_epoch = Arc::clone(&supersede_epoch);
    let worker_gate = Arc::clone(&send_gate);
    let worker_volume = Arc::clone(&desired_volume);
    let worker_intent = Arc::clone(&intent);
    let worker_preparation = Arc::clone(&preparation);
    let worker = thread::Builder::new()
        .name("tide-playback".into())
        .spawn(move || {
            runtime.block_on(run_worker(
                runtimes,
                command_rx,
                shutdown_rx,
                worker_snapshot,
                WorkerControls {
                    desired_generation: worker_generation,
                    supersede_epoch: worker_epoch,
                    send_gate: worker_gate,
                    desired_volume: worker_volume,
                    intent: worker_intent,
                    preparation: worker_preparation,
                    preparation_rx,
                },
            ));
        })
        .map_err(|error| format!("Could not start the playback worker: {error}"))?;

    Ok(PlaybackHandle {
        command_tx,
        shutdown_tx,
        shutdown_requested,
        desired_generation,
        supersede_epoch,
        desired_volume,
        intent,
        snapshot,
        send_gate,
        worker: Some(worker),
        yt_dlp_available,
        preparation,
    })
}

#[cfg(not(windows))]
pub fn start() -> Result<PlaybackHandle, String> {
    Err("The native mpv named-pipe backend is currently supported on Windows only.".into())
}

#[cfg(not(windows))]
pub fn start_with_cookie_file(_: Option<PathBuf>) -> Result<PlaybackHandle, String> {
    start()
}
#[cfg(not(windows))]
pub fn start_with_account(_: Option<PathBuf>, _: u32) -> Result<PlaybackHandle, String> {
    start()
}

impl PlaybackHandle {
    /// Queue a playback command without waiting for mpv or network I/O.
    pub fn send(&self, command: PlaybackCommand) -> Result<(), String> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err("The playback engine is shutting down.".into());
        }

        match command {
            PlaybackCommand::Load { source, generation } => {
                let validated = validate_source(&source)?;
                if validated.is_youtube && !self.yt_dlp_available {
                    return Err(
                        "YouTube playback needs yt-dlp.exe. Place it beside mpv in .tools/runtime or the app's tools directory, or add it to PATH.".into(),
                    );
                }

                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let previous_generation = self.desired_generation.load(Ordering::Acquire);
                if generation < previous_generation {
                    return Err(format!(
                        "Ignoring stale playback generation {generation}; generation {previous_generation} is already active."
                    ));
                }

                if self.shutdown_requested.load(Ordering::Acquire) {
                    return Err("The playback engine is shutting down.".into());
                }
                let epoch = next_epoch(self.supersede_epoch.load(Ordering::Acquire));
                self.command_tx
                    .try_send(WorkerCommand::Load(PendingLoad {
                        prepared: self.preparation.take_for(&validated.source),
                        source: validated.source,
                        generation,
                        epoch,
                    }))
                    .map_err(command_queue_error)?;

                self.supersede_epoch.store(epoch, Ordering::Release);
                self.desired_generation.store(generation, Ordering::Release);
                *self.intent.lock().unwrap_or_else(|p| p.into_inner()) = PlaybackIntent {
                    epoch,
                    paused: false,
                    ..PlaybackIntent::default()
                };
                let mut snapshot = self.snapshot_write();
                snapshot.generation = generation;
                snapshot.position = 0.0;
                snapshot.duration = 0.0;
                snapshot.paused = false;
                snapshot.loading = true;
                snapshot.ended = false;
                snapshot.error = None;
                snapshot.title = None;
                Ok(())
            }
            PlaybackCommand::Seek(seconds) => {
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err("Seek position must be a finite, non-negative number.".into());
                }
                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.shutdown_requested.load(Ordering::Acquire) {
                    return Err("The playback engine is shutting down.".into());
                }
                self.try_send(ActionCommand::Seek)?;
                let mut intent = self.intent.lock().unwrap_or_else(|p| p.into_inner());
                intent.seek = Some(seconds);
                intent.seek_revision = next_epoch(intent.seek_revision);
                Ok(())
            }
            PlaybackCommand::SetVolume(volume) => {
                if !volume.is_finite() || !(0.0..=100.0).contains(&volume) {
                    return Err("Volume must be a finite number from 0 to 100.".into());
                }
                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.shutdown_requested.load(Ordering::Acquire) {
                    return Err("The playback engine is shutting down.".into());
                }
                self.command_tx
                    .try_send(WorkerCommand::Action {
                        epoch: self.supersede_epoch.load(Ordering::Acquire),
                        command: ActionCommand::SetVolume,
                    })
                    .map_err(command_queue_error)?;
                self.desired_volume
                    .store(volume.to_bits(), Ordering::Release);
                self.snapshot_write().volume = volume;
                Ok(())
            }
            PlaybackCommand::Stop => {
                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.shutdown_requested.load(Ordering::Acquire) {
                    return Err("The playback engine is shutting down.".into());
                }
                let epoch = next_epoch(self.supersede_epoch.load(Ordering::Acquire));
                self.command_tx
                    .try_send(WorkerCommand::Stop { epoch })
                    .map_err(command_queue_error)?;
                self.supersede_epoch.store(epoch, Ordering::Release);
                self.preparation.clear();
                reset_snapshot(&mut self.snapshot_write());
                Ok(())
            }
            PlaybackCommand::SetPaused(paused) => {
                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.shutdown_requested.load(Ordering::Acquire) {
                    return Err("The playback engine is shutting down.".into());
                }
                self.try_send(ActionCommand::SetPaused)?;
                self.intent.lock().unwrap_or_else(|p| p.into_inner()).paused = paused;
                self.snapshot_write().paused = paused;
                Ok(())
            }
            PlaybackCommand::TogglePause => {
                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.shutdown_requested.load(Ordering::Acquire) {
                    return Err("The playback engine is shutting down.".into());
                }
                let paused = !self.intent.lock().unwrap_or_else(|p| p.into_inner()).paused;
                self.try_send(ActionCommand::SetPaused)?;
                self.intent.lock().unwrap_or_else(|p| p.into_inner()).paused = paused;
                self.snapshot_write().paused = paused;
                Ok(())
            }
            PlaybackCommand::Shutdown => {
                let _gate = self
                    .send_gate
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                self.shutdown_requested.store(true, Ordering::Release);
                self.preparation.close();
                self.shutdown_tx.send_replace(true);
                Ok(())
            }
        }
    }

    /// Return a copy of the most recently observed playback state.
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot_read().clone()
    }

    /// Resolve at most one upcoming YouTube track while the current track plays.
    /// Repeated calls with the same source are cheap. No media is downloaded;
    /// stream URLs stay in memory briefly and failures use ordinary loading.
    /// Pass `None` when the queue has no upcoming song or changes account.
    pub fn prepare_next(&self, source: Option<&str>) -> Result<(), String> {
        let _gate = self.send_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err("The playback engine is shutting down.".into());
        }
        let source = source.map(validate_source).transpose()?;
        self.preparation.set(
            source
                .filter(|s| s.is_youtube && self.yt_dlp_available)
                .map(|s| s.source),
        );
        Ok(())
    }

    /// Whether the requested upcoming track has a fresh, prepared stream.
    pub fn next_is_prepared(&self, source: &str) -> bool {
        validate_source(source).is_ok_and(|source| self.preparation.is_ready(&source.source))
    }

    fn try_send(&self, command: ActionCommand) -> Result<(), String> {
        self.command_tx
            .try_send(WorkerCommand::Action {
                epoch: self.supersede_epoch.load(Ordering::Acquire),
                command,
            })
            .map_err(command_queue_error)
    }

    fn snapshot_read(&self) -> std::sync::RwLockReadGuard<'_, PlaybackSnapshot> {
        self.snapshot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn snapshot_write(&self) -> std::sync::RwLockWriteGuard<'_, PlaybackSnapshot> {
        self.snapshot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        self.shutdown_requested.store(true, Ordering::Release);
        self.preparation.close();
        self.shutdown_tx.send_replace(true);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn command_queue_error<T>(error: mpsc::error::TrySendError<T>) -> String {
    match error {
        mpsc::error::TrySendError::Full(_) => {
            "The playback command queue is full; try again shortly.".into()
        }
        mpsc::error::TrySendError::Closed(_) => {
            "The playback worker is no longer accepting commands.".into()
        }
    }
}

fn next_epoch(current: u64) -> u64 {
    current.wrapping_add(1).max(1)
}

enum WorkerCommand {
    Load(PendingLoad),
    Stop {
        epoch: u64,
    },
    Action {
        epoch: u64,
        command: ActionCommand,
    },
    #[cfg(test)]
    ProbeTerminate(std::sync::mpsc::Sender<()>),
}

enum ActionCommand {
    SetPaused,
    Seek,
    SetVolume,
}

#[derive(Clone, Default)]
struct PlaybackIntent {
    epoch: u64,
    paused: bool,
    seek: Option<f64>,
    seek_revision: u64,
}

#[cfg(windows)]
struct WorkerControls {
    desired_generation: Arc<AtomicU64>,
    supersede_epoch: Arc<AtomicU64>,
    desired_volume: Arc<AtomicU64>,
    intent: Arc<Mutex<PlaybackIntent>>,
    send_gate: Arc<Mutex<()>>,
    preparation: Arc<Preparation>,
    preparation_rx: watch::Receiver<prefetch::Request>,
}

#[derive(Clone, Debug)]
struct RuntimePaths {
    cookies: Option<PathBuf>,
    account_index: u32,
    mpv: PathBuf,
    yt_dlp: Option<PathBuf>,
}

impl RuntimePaths {
    fn discover() -> Result<Self, String> {
        let mpv = find_runtime_binary("mpv.exe")
            .ok_or_else(|| {
                "mpv.exe was not found. Place it in the app's tools directory, .tools/runtime, or add it to PATH.".to_owned()
            })?;
        Ok(Self {
            cookies: None,
            account_index: 0,
            mpv,
            yt_dlp: Self::discover_yt_dlp(),
        })
    }

    fn discover_yt_dlp() -> Option<PathBuf> {
        find_runtime_binary("yt-dlp.exe")
    }
}

fn find_runtime_binary(name: &str) -> Option<PathBuf> {
    let mut directories = Vec::new();

    if let Some(runtime_dir) = env::var_os("TIDE_RUNTIME_DIR") {
        directories.push(PathBuf::from(runtime_dir));
    }

    if let Ok(executable) = env::current_exe() {
        if let Some(exe_dir) = executable.parent() {
            directories.push(exe_dir.join("tools"));
            directories.push(exe_dir.join(".tools").join("runtime"));
            directories.push(exe_dir.to_path_buf());
            add_runtime_ancestors(exe_dir, &mut directories);
        }
    }

    if let Ok(current_dir) = env::current_dir() {
        add_runtime_ancestors(&current_dir, &mut directories);
    }

    for directory in directories {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return fs::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }

    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return fs::canonicalize(&candidate).ok().or(Some(candidate));
            }
        }
    }
    None
}

fn add_runtime_ancestors(start: &Path, directories: &mut Vec<PathBuf>) {
    for ancestor in start.ancestors().take(8) {
        directories.push(ancestor.join(".tools").join("runtime"));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValidatedSource {
    source: String,
    is_youtube: bool,
}

/// Validate before adding a source to a persistent queue. This shares the
/// backend's scheme, credentials, control-character, and local-file checks.
pub fn validate_playback_source(source: &str) -> Result<String, String> {
    validate_source(source).map(|validated| validated.source)
}

fn validate_source(source: &str) -> Result<ValidatedSource, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("Choose a local file or an http/https URL.".into());
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err("The playback source is too long.".into());
    }
    if source.chars().any(char::is_control) {
        return Err("The playback source contains unsupported control characters.".into());
    }

    if source.contains("://") || source.starts_with("http:") || source.starts_with("https:") {
        let url = Url::parse(source).map_err(|_| "Enter a valid http/https URL.".to_owned())?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err("Only http and https playback URLs are supported.".into());
        }
        if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err(
                "The URL must have a host and must not contain embedded credentials.".into(),
            );
        }
        let is_youtube = url.host_str().is_some_and(is_youtube_host);
        return Ok(ValidatedSource {
            source: url.to_string(),
            is_youtube,
        });
    }

    #[cfg(windows)]
    if source.starts_with("\\\\") || source.starts_with("//") {
        return Err("Network shares are not accepted as local playback files.".into());
    }

    let path = PathBuf::from(source);
    if !path.is_file() {
        return Err("The local playback file does not exist or is not a regular file.".into());
    }
    let absolute_path = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map_err(|error| format!("Could not resolve the local file path: {error}"))?
            .join(path)
    };
    Ok(ValidatedSource {
        source: absolute_path.to_string_lossy().into_owned(),
        is_youtube: false,
    })
}

fn is_youtube_host(host: &str) -> bool {
    host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtu.be"
        || host.ends_with(".youtu.be")
}

#[cfg(windows)]
type PipeWriter = WriteHalf<tokio::net::windows::named_pipe::NamedPipeClient>;

#[cfg(windows)]
struct RunningPlayer {
    child: OwnedChild,
    writer: PipeWriter,
    reader_task: tokio::task::JoinHandle<()>,
    session_id: u64,
}

/// std::process::Child does not kill the child when dropped. This guard makes
/// even worker unwinding terminate and reap the owned mpv process.
#[cfg(windows)]
struct OwnedChild {
    process: Child,
    // Close after the child has been reaped; this also kills helper descendants.
    _job: OwnedJob,
}

#[cfg(windows)]
impl OwnedChild {
    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.process.try_wait()
    }

    fn kill(&mut self) -> io::Result<()> {
        self.process.kill()
    }

    fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.process.wait()
    }
}

#[cfg(windows)]
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.process.try_wait().ok().flatten().is_none() {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }
}

#[cfg(windows)]
struct OwnedJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl OwnedJob {
    fn attach(process: &Child) -> io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        // SAFETY: Null security attributes create a non-inheritable, unnamed
        // job. All handles and structure pointers below remain valid for each
        // synchronous Win32 call. OwnedJob closes its unique handle once.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(job.0, process.as_raw_handle()) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for OwnedJob {
    fn drop(&mut self) {
        // SAFETY: This is the unique handle returned by CreateJobObjectW.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[derive(Clone)]
struct PendingLoad {
    source: String,
    generation: u64,
    epoch: u64,
    prepared: Option<PreparedStream>,
}

#[cfg(windows)]
#[derive(Default)]
struct PlayerState {
    active_generation: Option<u64>,
    active_epoch: Option<u64>,
    active_entry_id: Option<u64>,
    awaiting_start: Option<u64>,
    next_request_id: i64,
    requests: HashMap<i64, RequestPurpose>,
    prepared_fallback: Option<PendingLoad>,
}

#[cfg(windows)]
enum RequestPurpose {
    Load { generation: u64, epoch: u64 },
}

#[cfg(windows)]
enum IpcEvent {
    Line { session_id: u64, line: String },
    Closed { session_id: u64 },
}

#[cfg(windows)]
async fn run_worker(
    runtimes: RuntimePaths,
    mut command_rx: mpsc::Receiver<WorkerCommand>,
    mut shutdown_rx: watch::Receiver<bool>,
    snapshot: Arc<RwLock<PlaybackSnapshot>>,
    controls: WorkerControls,
) {
    let WorkerControls {
        desired_generation,
        supersede_epoch,
        desired_volume,
        intent,
        send_gate,
        preparation,
        preparation_rx,
    } = controls;
    let (ipc_event_tx, mut ipc_event_rx) = mpsc::channel(IPC_EVENT_CAPACITY);
    let mut player: Option<RunningPlayer> = None;
    let mut state = PlayerState::default();
    let mut track: Option<recovery::Track> = None;
    let mut next_session_id = 1_u64;
    let mut watchdog = tokio::time::interval(Duration::from_millis(250));
    watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let preparation_task = tokio::spawn(prefetch::run(
        runtimes.clone(),
        Arc::clone(&preparation),
        preparation_rx,
    ));

    loop {
        let mut fault = false;
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() { break; }
            }
            command = command_rx.recv() => {
                let Some(command) = command else { break; };
                match command {
                    WorkerCommand::Load(load) => {
                        let current = {
                            let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
                            load.epoch == supersede_epoch.load(Ordering::Acquire)
                        };
                        if !current { continue; }
                        if let Some(mut old) = player.take() { terminate_player(&mut old).await; }
                        state = PlayerState::default();
                        track = Some(recovery::Track::new(load, Instant::now()));
                    }
                    WorkerCommand::Stop { epoch } => {
                        let current = {
                            let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
                            epoch == supersede_epoch.load(Ordering::Acquire)
                        };
                        if !current { continue; }
                        track = None;
                        state = PlayerState::default();
                        if let Some(mut running) = player.take() { terminate_player(&mut running).await; }
                    }
                    WorkerCommand::Action { epoch, command } => {
                        // Accepted targets are authoritative, including controls queued
                        // while reconnecting. Pause/seek wait until the file is ready.
                        let accepted = {
                            let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
                            if epoch != supersede_epoch.load(Ordering::Acquire)
                                && !matches!(command, ActionCommand::SetVolume) { continue; }
                            intent.lock().unwrap_or_else(|p| p.into_inner()).clone()
                        };
                        let ready = accepted.epoch == epoch && track.as_ref().is_some_and(|track| track.loaded);
                        let command = match command {
                            ActionCommand::SetPaused if ready => Some(json!(["set_property", "pause", accepted.paused])),
                            ActionCommand::Seek if ready => track.as_mut().and_then(|track|
                                track.apply_seek(&accepted, Instant::now()))
                                .map(|seconds| json!(["seek", seconds, "absolute+exact"])),
                            ActionCommand::SetVolume => Some(json!(["set_property", "volume",
                                f64::from_bits(desired_volume.load(Ordering::Acquire))])),
                            _ => None,
                        };
                        if let (Some(command), Some(running)) = (command, player.as_mut()) {
                            fault = send_ipc_command(&mut running.writer, command, None).await.is_err();
                        }
                    }
                    #[cfg(test)]
                    WorkerCommand::ProbeTerminate(done) => {
                        if let Some(running) = player.as_mut() {
                            let _ = running.child.kill();
                        }
                        let _ = done.send(());
                    }
                }
            }
            event = ipc_event_rx.recv() => {
                match event {
                    Some(IpcEvent::Line { session_id, line })
                        if player.as_ref().is_some_and(|running| running.session_id == session_id) => {
                        if let Ok(message) = serde_json::from_str::<Value>(&line) {
                            let loaded;
                            {
                                let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
                                let generation = desired_generation.load(Ordering::Acquire);
                                let epoch = supersede_epoch.load(Ordering::Acquire);
                                let current = track.as_ref().is_some_and(|track|
                                    track.load.generation == generation && track.load.epoch == epoch);
                                if !current { continue; }
                                let active = active_is_current(&state, generation, epoch);
                                let event = message.get("event").and_then(Value::as_str);
                                let matching_end = active && event == Some("end-file")
                                    && message.get("playlist_entry_id").and_then(Value::as_u64) == state.active_entry_id;
                                fault = track.as_ref().is_some_and(|track|
                                    recovery::message_failed(&state, track, &message, generation, epoch));
                                // Retain the prepared fallback's generation/entry checks;
                                // recovery always reloads its original source, never its URL.
                                let prepared_failed = take_prepared_fallback(&mut state, &message, generation, epoch).is_some();
                                fault |= prepared_failed;
                                loaded = active && event == Some("file-loaded");
                                let mut view = snapshot_write(&snapshot);
                                // Do not publish an intermediate ended/error state: the app
                                // may otherwise advance the queue before recovery is scheduled.
                                if !fault {
                                    apply_ipc_message(&mut state, &mut view, &message, generation, epoch);
                                    if let Some(current) = track.as_mut() {
                                        if loaded { current.file_loaded(Instant::now()); }
                                        if active && event == Some("property-change") {
                                            match message.get("name").and_then(Value::as_str) {
                                                Some("time-pos") => {
                                                    if let Some(position) = message.get("data").and_then(finite_non_negative) {
                                                        current.observe_position(position, Instant::now());
                                                        view.position = current.position;
                                                    }
                                                }
                                                Some("duration") => {
                                                    if let Some(duration) = message.get("data").and_then(finite_non_negative) {
                                                        // Some demuxers shorten duration to the
                                                        // bytes received immediately before EOF.
                                                        // Keep the known full duration for recovery.
                                                        current.duration = current.duration.max(duration);
                                                        view.duration = current.duration;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    if matching_end && view.ended { track = None; }
                                }
                                view.volume = f64::from_bits(desired_volume.load(Ordering::Acquire));
                                view.paused = intent.lock().unwrap_or_else(|p| p.into_inner()).paused || view.ended;
                            }
                            if loaded && !fault {
                                let accepted = intent.lock().unwrap_or_else(|p| p.into_inner()).clone();
                                if let (Some(current), Some(running)) = (track.as_mut(), player.as_mut()) {
                                    if let Some(seconds) = current.apply_seek(&accepted, Instant::now()) {
                                        fault = send_ipc_command(&mut running.writer, json!(["seek", seconds, "absolute+exact"]), None).await.is_err();
                                    }
                                    if !fault {
                                        fault = send_ipc_command(&mut running.writer,
                                            json!(["set_property", "pause", accepted.paused]), None).await.is_err();
                                    }
                                }
                            }
                        }
                    }
                    Some(IpcEvent::Closed { session_id })
                        if player.as_ref().is_some_and(|running| running.session_id == session_id) => {
                        fault = track.is_some();
                        if !fault {
                            if let Some(mut failed) = player.take() { terminate_player(&mut failed).await; }
                        }
                    }
                    _ => {}
                }
            }
            _ = watchdog.tick() => {
                let accepted = {
                    let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
                    intent.lock().unwrap_or_else(|p| p.into_inner()).clone()
                };
                if let Some(current) = track.as_mut() {
                    if current.load.epoch != supersede_epoch.load(Ordering::Acquire) { continue; }
                    let now = Instant::now();
                    if current.retry_at.is_some_and(|retry_at| now >= retry_at) {
                        if let Some(mut old) = player.take() { terminate_player(&mut old).await; }
                        state = PlayerState::default();
                        current.apply_seek(&accepted, now);
                        current.session_started(now);
                        let volume = f64::from_bits(desired_volume.load(Ordering::Acquire));
                        match launch_player(&runtimes, volume, &mut shutdown_rx, &ipc_event_tx,
                            next_session_id, &supersede_epoch, current.load.epoch).await {
                            Ok(Some(mut running)) => {
                                next_session_id = next_session_id.wrapping_add(1).max(1);
                                if observe_properties(&mut running).await.is_err() {
                                    fault = true;
                                } else {
                                    let mut load = current.load.clone();
                                    // Resume via mpv's per-file start option before audio starts.
                                    // Starting paused prevents any accepted pause racing loading.
                                    fault = begin_load_at(&mut running, &mut state, &mut load, current.position).await.is_err();
                                }
                                player = Some(running);
                            }
                            Ok(None) => {
                                if *shutdown_rx.borrow() { break; }
                                track = None;
                            }
                            Err(_) => { fault = true; }
                        }
                    } else {
                        fault = current.timed_out(accepted.paused, now);
                    }
                }
            }
        }
        if fault {
            let retry = {
                let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(current) = track.as_mut() {
                    if current.load.generation != desired_generation.load(Ordering::Acquire)
                        || current.load.epoch != supersede_epoch.load(Ordering::Acquire)
                    {
                        continue;
                    }
                    let retry = current.schedule_retry(Instant::now());
                    let mut view = snapshot_write(&snapshot);
                    view.loading = retry;
                    view.ended = false;
                    view.paused = intent.lock().unwrap_or_else(|p| p.into_inner()).paused;
                    view.position = current.position;
                    view.error = (!retry).then(|| "Playback could not recover after two reconnect attempts. Try this track again or check your connection.".into());
                    retry
                } else {
                    false
                }
            };
            if let Some(mut failed) = player.take() {
                terminate_player(&mut failed).await;
            }
            state = PlayerState::default();
            if !retry {
                track = None;
            }
        }
    }
    if let Some(mut running) = player.take() {
        terminate_player(&mut running).await;
    }
    preparation.close();
    let _ = preparation_task.await;
    let _gate = send_gate.lock().unwrap_or_else(|p| p.into_inner());
    reset_snapshot(&mut snapshot_write(&snapshot));
}

#[cfg(windows)]
async fn launch_player(
    runtimes: &RuntimePaths,
    volume: f64,
    shutdown_rx: &mut watch::Receiver<bool>,
    ipc_event_tx: &mpsc::Sender<IpcEvent>,
    session_id: u64,
    desired_epoch: &AtomicU64,
    epoch: u64,
) -> Result<Option<RunningPlayer>, String> {
    use std::os::windows::process::CommandExt;
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_path = unique_pipe_path();
    let mut command = Command::new(&runtimes.mpv);
    command
        .arg("--no-config")
        .arg("--idle=yes")
        .arg("--terminal=no")
        .arg("--input-terminal=no")
        // Tide owns the Windows media session; its helper must not process the
        // same hardware key independently and cause double pause/skip actions.
        .arg("--input-media-keys=no")
        .arg("--force-window=no")
        .arg("--vid=no")
        .arg("--vo=null")
        .arg("--audio-display=no")
        .arg("--osc=no")
        .arg("--osd-level=0")
        .arg("--ytdl=yes")
        .arg("--ytdl-format=bestaudio/best")
        .arg("--ytdl-raw-options=ignore-config=")
        .arg("--cache=yes")
        .arg("--cache-secs=20")
        .arg("--demuxer-readahead-secs=5")
        .arg("--demuxer-max-bytes=16MiB")
        .arg("--demuxer-max-back-bytes=2MiB")
        .arg(format!("--volume={volume}"))
        .arg(format!("--input-ipc-server={pipe_path}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000);
    if let Some(cookies) = &runtimes.cookies {
        let path = cookies.to_string_lossy();
        command.arg(format!(
            "--ytdl-raw-options=ignore-config=,cookies=%{}%{}{}",
            path.len(),
            path,
            if runtimes.account_index > 0 {
                let args = "youtube:player_skip=webpage,configs";
                format!(
                    ",add-headers=X-Goog-AuthUser:{},extractor-args=%{}%{}",
                    runtimes.account_index,
                    args.len(),
                    args
                )
            } else {
                String::new()
            }
        ));
    }

    if runtimes.yt_dlp.is_some() {
        command.arg("--script-opts=ytdl_hook-ytdl_path=yt-dlp.exe");
    }
    command.env("PATH", child_path(runtimes)?);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start mpv.exe: {error}"))?;
    // mpv is idle here and cannot launch a source extractor until loadfile.
    // Refuse playback if process-tree containment could not be installed.
    let job = match OwnedJob::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Could not contain the mpv process tree: {error}"));
        }
    };
    let mut child = OwnedChild {
        process: child,
        _job: job,
    };
    let started = Instant::now();

    let client = loop {
        if *shutdown_rx.borrow() || desired_epoch.load(Ordering::Acquire) != epoch {
            return Ok(None);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Could not check the mpv process: {error}"))?
        {
            return Err(format!(
                "mpv.exe exited before its IPC pipe was ready (status {status}). Check that this is a recent mpv build."
            ));
        }
        match ClientOptions::new().open(&pipe_path) {
            Ok(client) => break client,
            Err(_) if started.elapsed() < IPC_CONNECT_TIMEOUT => {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            return Ok(None);
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(30)) => {}
                }
            }
            Err(error) => {
                return Err(format!(
                    "mpv.exe started, but its named-pipe IPC did not become ready within {} seconds: {error}",
                    IPC_CONNECT_TIMEOUT.as_secs()
                ));
            }
        }
    };

    let (reader, writer) = tokio::io::split(client);
    let mut reader = BufReader::new(reader);
    let event_tx = ipc_event_tx.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            match read_ipc_line(&mut reader).await {
                Ok(Some(line)) => {
                    if event_tx
                        .send(IpcEvent::Line { session_id, line })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = event_tx.send(IpcEvent::Closed { session_id }).await;
                    break;
                }
                Err(_) => {
                    let _ = event_tx.send(IpcEvent::Closed { session_id }).await;
                    break;
                }
            }
        }
    });

    // Send initial controls before a track is loaded. These commands never
    // include user input and use JSON arguments rather than text IPC syntax.
    let mut running = RunningPlayer {
        child,
        writer,
        reader_task,
        session_id,
    };
    if let Err(error) = send_ipc_command(
        &mut running.writer,
        json!(["set_property", "volume", volume]),
        None,
    )
    .await
    {
        terminate_player(&mut running).await;
        return Err(format!("Could not initialize mpv IPC: {error}"));
    }
    Ok(Some(running))
}

#[cfg(windows)]
async fn read_ipc_line(reader: &mut (impl AsyncBufRead + Unpin)) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "mpv IPC line was incomplete",
            ));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if line.len() + take > MAX_IPC_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "mpv IPC line exceeded its size limit",
            ));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return String::from_utf8(line)
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
    }
}

#[cfg(windows)]
fn unique_pipe_path() -> String {
    let counter = PIPE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        r"\\.\pipe\tide-playback-{}-{tick}-{counter}",
        std::process::id()
    )
}

#[cfg(windows)]
fn child_path(runtimes: &RuntimePaths) -> Result<OsString, String> {
    let mut paths = Vec::new();
    if let Some(directory) = runtimes.mpv.parent() {
        paths.push(directory.to_path_buf());
    }
    if let Some(directory) = runtimes.yt_dlp.as_ref().and_then(|path| path.parent()) {
        if !paths.iter().any(|existing| existing == directory) {
            paths.push(directory.to_path_buf());
        }
    }
    if let Some(inherited) = env::var_os("PATH") {
        paths.extend(env::split_paths(&inherited));
    }
    env::join_paths(paths)
        .map_err(|error| format!("Could not prepare the mpv runtime PATH: {error}"))
}

#[cfg(windows)]
async fn observe_properties(player: &mut RunningPlayer) -> io::Result<()> {
    for (id, property) in [
        (1, "time-pos"),
        (2, "duration"),
        (3, "pause"),
        (4, "volume"),
        (5, "media-title"),
    ] {
        send_ipc_command(
            &mut player.writer,
            json!(["observe_property", id, property]),
            None,
        )
        .await?;
    }
    Ok(())
}

#[cfg(windows)]
async fn begin_load_at(
    player: &mut RunningPlayer,
    state: &mut PlayerState,
    load: &mut PendingLoad,
    position: f64,
) -> io::Result<()> {
    send_ipc_command(
        &mut player.writer,
        json!(["set_property", "pause", true]),
        None,
    )
    .await?;
    let mut command = if let Some(prepared) = &load.prepared {
        state.prepared_fallback = Some(PendingLoad {
            source: load.source.clone(),
            generation: load.generation,
            epoch: load.epoch,
            prepared: None,
        });
        prepared.load_command()
    } else {
        state.prepared_fallback = None;
        json!(["loadfile", load.source, "replace", -1, {}])
    };
    command[4]["start"] = json!(position.to_string());
    state.active_generation = Some(load.generation);
    state.active_epoch = Some(load.epoch);
    state.active_entry_id = None;
    state.awaiting_start = Some(load.generation);
    let request_id = next_request_id(state);
    state.requests.insert(
        request_id,
        RequestPurpose::Load {
            generation: load.generation,
            epoch: load.epoch,
        },
    );
    if let Err(error) = send_ipc_command(&mut player.writer, command, Some(request_id)).await {
        state.requests.remove(&request_id);
        state.awaiting_start = None;
        state.active_generation = None;
        state.active_epoch = None;
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn next_request_id(state: &mut PlayerState) -> i64 {
    state.next_request_id = state.next_request_id.wrapping_add(1).max(1);
    state.next_request_id
}

#[cfg(windows)]
async fn send_ipc_command(
    writer: &mut PipeWriter,
    command: Value,
    request_id: Option<i64>,
) -> io::Result<()> {
    let mut message = json!({ "command": command });
    if let Some(request_id) = request_id {
        message["request_id"] = json!(request_id);
    }
    let mut encoded = serde_json::to_vec(&message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    encoded.push(b'\n');
    tokio::time::timeout(IPC_WRITE_TIMEOUT, async {
        writer.write_all(&encoded).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "mpv IPC write timed out"))?
}

#[cfg(windows)]
fn apply_ipc_message(
    state: &mut PlayerState,
    snapshot: &mut PlaybackSnapshot,
    message: &Value,
    desired_generation: u64,
    desired_epoch: u64,
) {
    if let Some(request_id) = message.get("request_id").and_then(Value::as_i64) {
        if let Some(RequestPurpose::Load { generation, epoch }) = state.requests.remove(&request_id)
        {
            let error = message.get("error").and_then(Value::as_str);
            if error.is_some_and(|error| error != "success")
                && generation == desired_generation
                && epoch == desired_epoch
            {
                snapshot.loading = false;
                snapshot.error = Some(
                    "mpv rejected this source. Check that the file or URL is supported and try again."
                        .into(),
                );
                if state.awaiting_start == Some(generation) {
                    state.awaiting_start = None;
                    state.active_generation = None;
                    state.active_epoch = None;
                    state.active_entry_id = None;
                }
            }
        }
    }

    match message.get("event").and_then(Value::as_str) {
        Some("start-file") => {
            if let Some(generation) = state.awaiting_start.take() {
                state.active_generation = Some(generation);
                state.active_entry_id = message.get("playlist_entry_id").and_then(Value::as_u64);
            }
        }
        Some("file-loaded") => {
            if active_is_current(state, desired_generation, desired_epoch) {
                state.prepared_fallback = None;
                snapshot.loading = false;
                snapshot.ended = false;
                snapshot.error = None;
            }
        }
        Some("end-file") => {
            let event_entry = message.get("playlist_entry_id").and_then(Value::as_u64);
            if event_entry.is_some()
                && event_entry == state.active_entry_id
                && active_is_current(state, desired_generation, desired_epoch)
            {
                let reason = message
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                snapshot.loading = false;
                match reason {
                    "eof" => {
                        snapshot.ended = true;
                        snapshot.paused = true;
                    }
                    "error" => {
                        snapshot.ended = true;
                        snapshot.paused = true;
                        snapshot.error = Some(
                            "Playback failed while reading this source. Check the file or URL and connection.".into(),
                        );
                    }
                    _ => {}
                }
                state.active_entry_id = None;
                state.active_generation = None;
                state.active_epoch = None;
            }
        }
        Some("property-change") if active_is_current(state, desired_generation, desired_epoch) => {
            if let (Some(name), Some(data)) = (
                message.get("name").and_then(Value::as_str),
                message.get("data"),
            ) {
                apply_property(snapshot, name, data);
            }
        }
        _ => {}
    }
}

/// Retry a rejected prepared URL once via the normal extractor. Compare both
/// the request/entry and current epoch so superseded tracks never restart.
#[cfg(windows)]
fn take_prepared_fallback(
    state: &mut PlayerState,
    message: &Value,
    generation: u64,
    epoch: u64,
) -> Option<PendingLoad> {
    let fallback = state.prepared_fallback.as_ref()?;
    if fallback.generation != generation || fallback.epoch != epoch {
        return None;
    }
    let rejected_request = message
        .get("request_id")
        .and_then(Value::as_i64)
        .is_some_and(|id| state.requests.contains_key(&id))
        && message
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error != "success");
    let failed_entry = message.get("event").and_then(Value::as_str) == Some("end-file")
        && message.get("reason").and_then(Value::as_str) == Some("error")
        && state.active_entry_id.is_some()
        && message.get("playlist_entry_id").and_then(Value::as_u64) == state.active_entry_id;
    (rejected_request || failed_entry)
        .then(|| state.prepared_fallback.take())
        .flatten()
}

#[cfg(windows)]
fn active_is_current(state: &PlayerState, desired_generation: u64, desired_epoch: u64) -> bool {
    state.awaiting_start.is_none()
        && state.active_entry_id.is_some()
        && state.active_generation == Some(desired_generation)
        && state.active_epoch == Some(desired_epoch)
}

fn apply_property(snapshot: &mut PlaybackSnapshot, name: &str, value: &Value) {
    match name {
        "time-pos" => {
            if let Some(position) = finite_non_negative(value) {
                snapshot.position = position;
            }
        }
        "duration" => {
            if let Some(duration) = finite_non_negative(value) {
                snapshot.duration = duration;
            }
        }
        "pause" => {
            if let Some(paused) = value.as_bool() {
                snapshot.paused = paused;
            }
        }
        "volume" => {
            if let Some(volume) = finite_non_negative(value) {
                snapshot.volume = volume.clamp(0.0, 100.0);
            }
        }
        "media-title" => {
            snapshot.title = value
                .as_str()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_owned);
        }
        _ => {}
    }
}

fn finite_non_negative(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

#[cfg(windows)]
async fn terminate_player(player: &mut RunningPlayer) {
    let _ = tokio::time::timeout(
        Duration::from_millis(200),
        send_ipc_command(&mut player.writer, json!(["quit"]), None),
    )
    .await;

    let deadline = Instant::now() + SHUTDOWN_GRACE;
    loop {
        match player.child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            _ => {
                let _ = player.child.kill();
                let _ = player.child.wait();
                break;
            }
        }
    }
    player.reader_task.abort();
    let _ = (&mut player.reader_task).await;
}

fn reset_snapshot(snapshot: &mut PlaybackSnapshot) {
    snapshot.position = 0.0;
    snapshot.duration = 0.0;
    snapshot.paused = true;
    snapshot.loading = false;
    snapshot.ended = false;
    snapshot.error = None;
    snapshot.title = None;
}

fn snapshot_write(
    snapshot: &RwLock<PlaybackSnapshot>,
) -> std::sync::RwLockWriteGuard<'_, PlaybackSnapshot> {
    snapshot
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Opt-in: own muted local silence and only the mpv owned by this test.
    // No process enumeration, account session, network, or persisted queue.
    #[cfg(windows)]
    #[test]
    #[ignore = "opt-in muted native recovery fault probe requiring mpv"]
    fn native_recovery_fault_probe() {
        fn wait(handle: &PlaybackHandle, condition: impl Fn(&PlaybackSnapshot) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let view = handle.snapshot();
                assert_eq!(view.volume, 0.0, "Probe must remain muted");
                if condition(&view) {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "Probe state timed out: loading={} paused={} position={} ended={} error={:?}",
                    view.loading,
                    view.paused,
                    view.position,
                    view.ended,
                    view.error
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
        fn kill_owned(handle: &PlaybackHandle) {
            let (tx, rx) = std::sync::mpsc::channel();
            handle
                .command_tx
                .blocking_send(WorkerCommand::ProbeTerminate(tx))
                .unwrap();
            rx.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        let path = env::temp_dir().join(format!(
            "tide-recovery-{}-{}.wav",
            std::process::id(),
            PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let sample_bytes = 8_000_u32 * 2 * 60;
        let mut wav = Vec::with_capacity(sample_bytes as usize + 44);
        wav.extend(b"RIFF");
        wav.extend((sample_bytes + 36).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16_u32.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(8_000_u32.to_le_bytes());
        wav.extend(16_000_u32.to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(sample_bytes.to_le_bytes());
        wav.resize(sample_bytes as usize + 44, 0);
        fs::write(&path, &wav).unwrap();
        let handle = start().unwrap();
        handle.send(PlaybackCommand::SetVolume(0.0)).unwrap();
        handle
            .send(PlaybackCommand::Load {
                source: path.to_string_lossy().into_owned(),
                generation: 1,
            })
            .unwrap();
        wait(&handle, |s| !s.loading && s.position > 0.1);
        handle.send(PlaybackCommand::Seek(12.0)).unwrap();
        wait(&handle, |s| s.position >= 12.0);
        kill_owned(&handle);
        wait(&handle, |s| s.loading && s.error.is_none());
        handle.send(PlaybackCommand::SetPaused(true)).unwrap();
        handle.send(PlaybackCommand::Seek(7.0)).unwrap();
        wait(&handle, |s| {
            !s.loading && s.paused && (s.position - 7.0).abs() < 0.2
        });
        thread::sleep(Duration::from_millis(350));
        assert!((handle.snapshot().position - 7.0).abs() < 0.2);
        kill_owned(&handle);
        wait(&handle, |s| s.loading);
        wait(&handle, |s| {
            !s.loading && s.paused && (s.position - 7.0).abs() < 0.2
        });
        kill_owned(&handle);
        wait(&handle, |s| !s.loading && s.error.is_some() && !s.ended);
        handle
            .send(PlaybackCommand::Load {
                source: path.to_string_lossy().into_owned(),
                generation: 2,
            })
            .unwrap();
        wait(&handle, |s| {
            s.generation == 2 && !s.loading && s.position > 0.1
        });
        kill_owned(&handle);
        wait(&handle, |s| s.loading);
        handle
            .send(PlaybackCommand::Load {
                source: path.to_string_lossy().into_owned(),
                generation: 3,
            })
            .unwrap();
        wait(&handle, |s| {
            s.generation == 3 && !s.loading && s.position > 0.1
        });
        kill_owned(&handle);
        wait(&handle, |s| s.loading);
        handle.send(PlaybackCommand::Stop).unwrap();
        thread::sleep(Duration::from_millis(1500));
        let view = handle.snapshot();
        assert!(!view.loading && !view.ended && view.error.is_none());
        assert_eq!(view.position, 0.0);
        println!("PASS owned mpv faults: resume, pause/seek, two reconnects, exhaustion, replacement and Stop");

        // A loopback server deliberately returns only four seconds of a WAV
        // whose header declares sixty. Internal Range refill fails until Tide
        // enters recovery, then requests return the whole source. This exercises
        // real source interruption and demuxer early EOF,
        // original-source reconnect, and resume without terminating mpv.
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server_stop = Arc::new(AtomicBool::new(false));
        let server_done = Arc::clone(&server_stop);
        let requests = Arc::new(AtomicU64::new(0));
        let server_requests = Arc::clone(&requests);
        let allow_reconnect = Arc::new(AtomicBool::new(false));
        let server_allow = Arc::clone(&allow_reconnect);
        let server = thread::spawn(move || {
            while !server_done.load(Ordering::Acquire) {
                let (mut client, _) = match listener.accept() {
                    Ok(client) => client,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(_) => break,
                };
                client
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                client
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0_u8];
                while request.len() < 8 * 1024 {
                    if client.read(&mut byte).ok() != Some(1) {
                        break;
                    }
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let offset = request
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .and_then(|value| value.split('-').next()?.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let first = server_requests.fetch_add(1, Ordering::AcqRel) == 0;
                if !first && !server_allow.load(Ordering::Acquire) {
                    let _ = client.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let end = if first { 44 + 16_000 * 4 } else { wav.len() };
                let offset = offset.min(end);
                let status = if offset > 0 {
                    "206 Partial Content"
                } else {
                    "200 OK"
                };
                let mut header = format!("HTTP/1.1 {status}\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n", wav.len() - offset);
                if offset > 0 {
                    header.push_str(&format!(
                        "Content-Range: bytes {offset}-{}/{}\r\n",
                        end - 1,
                        wav.len()
                    ));
                }
                header.push_str("\r\n");
                let _ = client.write_all(header.as_bytes());
                let _ = client.write_all(&wav[offset..end]);
                // Allow real playback to begin, then close before the promised
                // Content-Length is satisfied to produce a stream read failure.
                if first {
                    thread::sleep(Duration::from_secs(3));
                }
            }
        });
        handle
            .send(PlaybackCommand::Load {
                source: format!("http://{address}/fixture.wav"),
                generation: 4,
            })
            .unwrap();
        println!("HTTP phase: waiting for initial playback");
        wait(&handle, |s| {
            !s.loading && s.position > 0.1 && s.duration > 59.0
        });
        println!(
            "HTTP initial playback at {:.3}s",
            handle.snapshot().position
        );
        wait(&handle, |s| {
            s.loading && s.position > 2.0 && s.error.is_none() && !s.ended
        });
        let resume_position = handle.snapshot().position;
        allow_reconnect.store(true, Ordering::Release);
        wait(&handle, |s| {
            !s.loading && s.position >= resume_position && s.error.is_none()
        });
        assert!(requests.load(Ordering::Acquire) >= 2);
        println!(
            "PASS HTTP read interruption resumed from {:.3}s; duration {:.3}s",
            resume_position,
            handle.snapshot().duration
        );
        handle.send(PlaybackCommand::Stop).unwrap();
        server_stop.store(true, Ordering::Release);
        server.join().unwrap();
        drop(handle);
        fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn prepared_startup_failure_retries_once_and_never_restarts_stale_track() {
        let mut state = PlayerState {
            active_entry_id: Some(7),
            prepared_fallback: Some(PendingLoad {
                source: "https://www.youtube.com/watch?v=public".into(),
                generation: 3,
                epoch: 9,
                prepared: None,
            }),
            ..PlayerState::default()
        };
        let message = json!({"event":"end-file", "reason":"error", "playlist_entry_id":7});
        assert!(take_prepared_fallback(&mut state, &message, 4, 10).is_none());
        assert!(take_prepared_fallback(&mut state, &message, 3, 9).is_some());
        assert!(take_prepared_fallback(&mut state, &message, 3, 9).is_none());
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn ipc_reader_enforces_limit_before_allocating_an_unbounded_line() {
        let bytes = vec![b'x'; MAX_IPC_LINE_BYTES + 1];
        let mut reader = BufReader::new(bytes.as_slice());
        assert_eq!(
            read_ipc_line(&mut reader).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut reader = BufReader::new(b"one\ntwo\n".as_slice());
        assert_eq!(
            read_ipc_line(&mut reader).await.unwrap().as_deref(),
            Some("one\n")
        );
        assert_eq!(
            read_ipc_line(&mut reader).await.unwrap().as_deref(),
            Some("two\n")
        );
        assert!(read_ipc_line(&mut reader).await.unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn dropping_owned_process_terminates_its_descendant() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };

        let directory = env::temp_dir().join(format!(
            "tide-job-test-{}-{}",
            std::process::id(),
            PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let mut process = Command::new(env::current_exe().unwrap())
            .args(["--exact", "tests::job_tree_probe", "--ignored"])
            .env("TIDE_PLAYBACK_TREE_TEST_DIR", &directory)
            .env_remove("TIDE_PLAYBACK_TREE_TEST_LEAF")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let job = match OwnedJob::attach(&process) {
            Ok(job) => job,
            Err(error) => {
                let _ = process.kill();
                let _ = process.wait();
                panic!("Could not attach the test process to a job: {error}");
            }
        };
        let mut owner = OwnedChild { process, _job: job };
        fs::write(directory.join("go"), b"go").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let leaf_pid = loop {
            if let Ok(pid) = fs::read_to_string(directory.join("leaf-pid")) {
                if let Ok(pid) = pid.parse::<u32>() {
                    break pid;
                }
            }
            if let Some(status) = owner.try_wait().unwrap() {
                use std::io::Read;
                let mut details = String::new();
                owner
                    .process
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_string(&mut details)
                    .unwrap();
                owner
                    .process
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut details)
                    .unwrap();
                panic!("Process-tree probe exited early ({status}): {details}");
            }
            assert!(
                Instant::now() < deadline,
                "Child process did not report its descendant"
            );
            thread::sleep(Duration::from_millis(10));
        };
        // SAFETY: OpenProcess returns a new owned handle. OwnedHandle closes it
        // exactly once; WaitForSingleObject only observes process termination.
        let leaf_handle = unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, leaf_pid);
            assert!(
                !handle.is_null(),
                "Could not observe the descendant process"
            );
            OwnedHandle::from_raw_handle(handle)
        };
        drop(owner);
        let result = unsafe { WaitForSingleObject(leaf_handle.as_raw_handle(), 3000) };
        assert_eq!(
            result, WAIT_OBJECT_0,
            "Descendant outlived its owned process tree"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn cancelling_contained_process_terminates_descendant_and_closes_pipes() {
        use std::io::Read;
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        let directory = env::temp_dir().join(format!(
            "tide-cancel-test-{}-{}",
            std::process::id(),
            PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let mut command = Command::new(env::current_exe().unwrap());
        command
            .args(["--exact", "tests::job_tree_probe", "--ignored"])
            .env("TIDE_PLAYBACK_TREE_TEST_DIR", &directory)
            .env_remove("TIDE_PLAYBACK_TREE_TEST_LEAF")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut owner = OwnedProcess::spawn(&mut command).unwrap();
        let mut stdout = owner.take_stdout().unwrap();
        let mut stderr = owner.take_stderr().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let stdout_tx = done_tx.clone();
        let stdout_reader = thread::spawn(move || {
            let _ = stdout.read_to_end(&mut Vec::new());
            let _ = stdout_tx.send(());
        });
        let stderr_reader = thread::spawn(move || {
            let _ = stderr.read_to_end(&mut Vec::new());
            let _ = done_tx.send(());
        });
        fs::write(directory.join("go"), b"go").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let pid = loop {
            if let Ok(pid) = fs::read_to_string(directory.join("leaf-pid")) {
                if let Ok(pid) = pid.parse::<u32>() {
                    break pid;
                }
            }
            assert!(
                owner.try_wait().unwrap().is_none(),
                "Contained root exited early"
            );
            assert!(Instant::now() < deadline, "Descendant did not start");
            thread::sleep(Duration::from_millis(10));
        };
        // SAFETY: OpenProcess returns a new owned handle used only to wait.
        let leaf_handle = unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            assert!(!handle.is_null());
            OwnedHandle::from_raw_handle(handle)
        };
        owner.terminate_tree().unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(leaf_handle.as_raw_handle(), 3000) },
            WAIT_OBJECT_0
        );
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Pipe remained open after cancellation");
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Pipe remained open after cancellation");
        stdout_reader.join().unwrap();
        stderr_reader.join().unwrap();
        drop(owner);
        fs::remove_dir_all(directory).unwrap();
    }

    // Only the cleanup test launches this probe. The root waits until the job
    // is attached before spawning the leaf, exactly as idle mpv does.
    #[cfg(windows)]
    #[test]
    #[ignore = "process-tree cleanup test helper"]
    fn job_tree_probe() {
        use std::os::windows::process::CommandExt;
        let directory = PathBuf::from(
            env::var_os("TIDE_PLAYBACK_TREE_TEST_DIR")
                .expect("This helper must be launched by its owning test"),
        );
        if env::var_os("TIDE_PLAYBACK_TREE_TEST_LEAF").is_none() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !directory.join("go").is_file() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(10));
            }
            let leaf = Command::new(env::current_exe().unwrap())
                .args(["--exact", "tests::job_tree_probe", "--ignored"])
                .env("TIDE_PLAYBACK_TREE_TEST_LEAF", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap();
            fs::write(directory.join("leaf-pid"), leaf.id().to_string()).unwrap();
            // The owner must kill this whole process tree before this returns.
            thread::sleep(Duration::from_secs(30));
            let mut leaf = leaf;
            let _ = leaf.kill();
            let _ = leaf.wait();
        } else {
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn validates_youtube_hosts_without_accepting_suffix_spoofs() {
        assert!(
            validate_source("https://music.youtube.com/watch?v=abc")
                .unwrap()
                .is_youtube
        );
        assert!(validate_source("https://youtu.be/abc").unwrap().is_youtube);
        assert!(
            !validate_source("https://youtube.com.example.org/watch?v=abc")
                .unwrap()
                .is_youtube
        );
    }

    #[test]
    fn rejects_unsupported_schemes_and_embedded_credentials() {
        assert!(validate_source("ftp://example.org/audio.mp3").is_err());
        assert!(validate_source("https://user:pass@example.org/song.mp3").is_err());
        assert!(validate_source("https://example.org/\ntrack").is_err());
    }

    #[test]
    fn accepts_existing_local_files_and_returns_absolute_path() {
        let path = std::env::temp_dir().join(format!(
            "tide-playback-test-{}-{}.mp3",
            std::process::id(),
            PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, b"not decoded by this validation test").unwrap();
        let validated = validate_source(path.to_str().unwrap()).unwrap();
        assert!(Path::new(&validated.source).is_absolute());
        assert!(!validated.is_youtube);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_invalid_control_values() {
        assert!(finite_non_negative(&json!(-1.0)).is_none());
        assert!(finite_non_negative(&json!("NaN")).is_none());
        let mut snapshot = PlaybackSnapshot::default();
        apply_property(&mut snapshot, "volume", &json!(130.0));
        assert_eq!(snapshot.volume, 100.0);
        apply_property(&mut snapshot, "media-title", &json!("  "));
        assert!(snapshot.title.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn ignores_end_file_and_property_events_from_superseded_entries() {
        let mut state = PlayerState {
            active_generation: Some(4),
            active_epoch: Some(2),
            active_entry_id: Some(11),
            ..PlayerState::default()
        };
        let mut snapshot = PlaybackSnapshot {
            generation: 5,
            loading: true,
            volume: 100.0,
            ..PlaybackSnapshot::default()
        };

        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({
                "event": "end-file",
                "reason": "error",
                "playlist_entry_id": 11,
                "file_error": "old URL with sensitive query data"
            }),
            5,
            2,
        );
        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({ "event": "property-change", "name": "media-title", "data": "old title" }),
            5,
            2,
        );

        assert!(snapshot.loading);
        assert!(snapshot.error.is_none());
        assert!(snapshot.title.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn stop_epoch_suppresses_late_events_even_when_generation_is_unchanged() {
        let mut state = PlayerState {
            active_generation: Some(4),
            active_epoch: Some(8),
            active_entry_id: Some(13),
            ..PlayerState::default()
        };
        let mut snapshot = PlaybackSnapshot {
            generation: 4,
            loading: false,
            position: 12.0,
            volume: 100.0,
            ..PlaybackSnapshot::default()
        };

        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({ "event": "end-file", "reason": "error", "playlist_entry_id": 13 }),
            4,
            9,
        );
        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({ "event": "property-change", "name": "time-pos", "data": 99.0 }),
            4,
            9,
        );

        assert!(!snapshot.ended);
        assert!(snapshot.error.is_none());
        assert_eq!(snapshot.position, 12.0);
    }

    #[cfg(windows)]
    #[test]
    fn maps_file_events_to_the_waiting_generation_and_discards_stale_completion() {
        let mut state = PlayerState {
            awaiting_start: Some(6),
            active_generation: Some(6),
            active_epoch: Some(3),
            ..PlayerState::default()
        };
        let mut snapshot = PlaybackSnapshot {
            generation: 6,
            loading: true,
            volume: 100.0,
            ..PlaybackSnapshot::default()
        };
        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({ "event": "start-file", "playlist_entry_id": 12 }),
            6,
            3,
        );
        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({ "event": "file-loaded" }),
            6,
            3,
        );
        assert!(!snapshot.loading);
        apply_ipc_message(
            &mut state,
            &mut snapshot,
            &json!({ "event": "end-file", "reason": "eof", "playlist_entry_id": 12 }),
            6,
            3,
        );
        assert!(snapshot.ended);
        assert!(snapshot.paused);
    }
}
