//! Unix IPC paths and process-group ownership. No fork-time Rust callbacks.
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn geteuid() -> u32;
    fn kill(pid: i32, signal: i32) -> i32;
}

pub(super) struct SocketPath {
    path: PathBuf,
}
impl SocketPath {
    pub fn new() -> io::Result<Self> {
        // SAFETY: geteuid takes no pointers and has no failure mode.
        let uid = unsafe { geteuid() };
        let base = std::env::temp_dir();
        let root = base.join(format!("tide-{uid}"));
        // macOS TMPDIR can be long; /tmp is a sticky system directory.
        let root = if root.as_os_str().len() > 55 {
            PathBuf::from(format!("/tmp/tide-{uid}"))
        } else {
            root
        };
        match fs::DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        let meta = fs::symlink_metadata(&root)?;
        if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unsafe IPC directory",
            ));
        }
        // Atomically reserve a unique directory so stale sockets are never reused.
        for _ in 0..100 {
            let count = super::PIPE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = root.join(format!("{}-{count:x}", std::process::id()));
            match fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {
                    return Ok(Self {
                        path: dir.join("ipc"),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "IPC directory collisions",
        ))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(dir) = self.path.parent() {
            let _ = fs::remove_dir(dir);
        }
    }
}

/// Owns a new process group. Descendants must not deliberately leave the group.
/// mpv additionally receives a dedicated inherited lifetime IPC connection.
pub struct OwnedProcess {
    pub(super) process: Child,
    group: i32,
    _lifetime: Option<UnixStream>,
    _socket: Option<SocketPath>,
    terminated: bool,
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let process = command.process_group(0).spawn()?;
        Ok(Self {
            group: process.id() as i32,
            process,
            _lifetime: None,
            _socket: None,
            terminated: false,
        })
    }
    pub(super) fn spawn_mpv(command: &mut Command, socket: SocketPath) -> io::Result<Self> {
        let (owner, child) = UnixStream::pair()?;
        // Stdio safely duplicates only the child's end to fd 0. The owner's end
        // stays CLOEXEC, including in unrelated concurrently spawned helpers.
        command
            .arg("--input-ipc-client=fd://0")
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(child)));
        let mut owned = Self::spawn(command)?;
        owned._lifetime = Some(owner);
        owned._socket = Some(socket);
        Ok(owned)
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.process.stdout.take()
    }
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.process.stdin.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.process.stderr.take()
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }
    pub(super) fn kill(&mut self) -> io::Result<()> {
        self.signal(9)
    }
    pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.process.wait()
    }
    fn signal(&self, signal: i32) -> io::Result<()> {
        // SAFETY: a negative PID addresses only our owned process group.
        if unsafe { kill(-self.group, signal) } == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(3) {
            Ok(())
        } else {
            Err(e)
        }
    }
    pub fn terminate_tree(&mut self) -> io::Result<()> {
        if self.terminated {
            return Ok(());
        }
        self._lifetime.take();
        if let Err(error) = self.signal(15) {
            if error.kind() != io::ErrorKind::PermissionDenied || self.process.try_wait()?.is_none()
            {
                return Err(error);
            }
        }
        let deadline = Instant::now() + super::SHUTDOWN_GRACE;
        // Do not reap the group leader until signaling is finished: its PID
        // cannot be reused for an unrelated group in this interval.
        while Instant::now() < deadline {
            // SAFETY: signal zero only checks group existence.
            if unsafe { kill(-self.group, 0) } != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let killed = self.signal(9);
        let status = self.process.try_wait()?;
        // Darwin can report EPERM rather than ESRCH for an all-zombie group.
        // Accept that only after confirming our direct child has exited.
        if let Err(error) = killed {
            if error.kind() != io::ErrorKind::PermissionDenied || status.is_none() {
                return Err(error);
            }
        }
        self.process.wait()?;
        self.terminated = true;
        Ok(())
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.terminate_tree();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_paths_are_private_short_unique_and_removed() {
        let a = SocketPath::new().unwrap();
        let b = SocketPath::new().unwrap();
        assert_ne!(a.path(), b.path());
        assert!(a.path().as_os_str().len() < 104);
        let dir = a.path().parent().unwrap().to_owned();
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        let listener = std::os::unix::net::UnixListener::bind(a.path()).unwrap();
        drop(listener);
        drop(a);
        assert!(!dir.exists());
    }
    #[test]
    fn runtime_discovery_order() {
        let socket = SocketPath::new().unwrap();
        let root = socket.path().parent().unwrap();
        let override_dir = root.join("override");
        let exe = root.join("Tide.app/Contents/MacOS/Tide");
        let cwd = root.join("dev");
        let dirs = crate::runtime_directories(
            Some(override_dir.clone()),
            Some(exe.clone()),
            Some(cwd.clone()),
        );
        let resources = exe.parent().unwrap().join("../Resources/bin");
        assert_eq!(
            &dirs[..3],
            &[
                override_dir.clone(),
                resources.clone(),
                exe.parent().unwrap().to_owned()
            ]
        );
        assert!(
            dirs.iter()
                .position(|d| *d == cwd.join(".tools/runtime"))
                .unwrap()
                < dirs
                    .iter()
                    .position(|d| *d == PathBuf::from("/opt/homebrew/bin"))
                    .unwrap()
        );
        let candidates = [
            override_dir,
            resources,
            exe.parent().unwrap().to_owned(),
            cwd.join(".tools/runtime"),
            root.join("path"),
        ];
        for dir in &candidates {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join("mpv"), b"fixture").unwrap();
        }
        for dir in &candidates {
            assert_eq!(
                crate::find_binary_in("mpv", candidates.to_vec()),
                Some(fs::canonicalize(dir.join("mpv")).unwrap())
            );
            fs::remove_file(dir.join("mpv")).unwrap();
        }
        // Remove only this test's reserved directory, retaining SocketPath's root.
        fs::remove_dir_all(root.join("Tide.app")).unwrap();
        fs::remove_dir_all(root.join("dev")).unwrap();
        fs::remove_dir(root.join("override")).unwrap();
        fs::remove_dir(root.join("path")).unwrap();
    }
    #[test]
    #[ignore = "requires real mpv"]
    fn real_mpv_exits_when_lifetime_connection_closes() {
        let mpv = crate::find_runtime_binary("mpv").expect("mpv installed");
        let socket = SocketPath::new().unwrap();
        let path = socket.path().to_owned();
        let mut command = Command::new(mpv);
        command
            .args([
                "--no-config",
                "--idle=yes",
                "--terminal=no",
                "--ao=null",
                "--vo=null",
            ])
            .arg(format!("--input-ipc-server={}", path.display()));
        let mut owned = OwnedProcess::spawn_mpv(&mut command, socket).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::os::unix::net::UnixStream::connect(&path).is_err() {
            assert!(Instant::now() < deadline, "mpv IPC never ready");
            assert!(owned.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(20));
        }
        // Model kernel closure on Tide crash; do not send quit or any signal.
        owned._lifetime.take();
        while owned.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "mpv survived lifetime EOF");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn drop_terminates_process_group() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 60 & wait"]);
        let mut owner = OwnedProcess::spawn(&mut command).unwrap();
        let group = owner.group;
        owner.terminate_tree().unwrap();
        assert!(owner.try_wait().unwrap().is_some());
        // SAFETY: signal zero never changes process state.
        assert_ne!(unsafe { kill(-group, 0) }, 0);
    }
}
