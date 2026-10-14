//! Owned Windows process trees for short-lived runtime tasks such as search.
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

use super::{OwnedChild, OwnedJob};

/// Contains a directly owned process and every descendant in a kill-on-close
/// job. Dropping the owner terminates and reaps its process tree.
pub struct OwnedProcess {
    pub(super) child: OwnedChild,
}

impl OwnedProcess {
    /// Spawn without a visible console. The new process remains suspended until
    /// its job is attached, so it cannot launch an uncontained child beforehand.
    /// Overrides Windows creation flags with CREATE_NO_WINDOW | CREATE_SUSPENDED.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let mut process = command.creation_flags(0x08000004).spawn()?;
        let job = match OwnedJob::attach(&process) {
            Ok(job) => job,
            Err(error) => {
                let _ = process.kill();
                let _ = process.wait();
                return Err(error);
            }
        };
        let owned = Self {
            child: OwnedChild { process, _job: job },
        };
        resume_owned_main_thread(owned.child.process.id())?;
        Ok(owned)
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.process.stdout.take()
    }
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.process.stdin.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.process.stderr.take()
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Kill every process in the owned job before waiting for pipe readers.
    pub fn terminate_tree(&mut self) -> io::Result<()> {
        self.child.terminate_tree()
    }
}

fn resume_owned_main_thread(process_id: u32) -> io::Result<()> {
    // SAFETY: ToolHelp returns a new snapshot handle. OwnedHandle closes each
    // successful Win32 handle once. Only the thread belonging to the directly
    // owned, suspended process is opened and resumed; other threads are ignored.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle::from_raw_handle(snapshot);
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut present = Thread32First(snapshot.as_raw_handle(), &mut entry) != 0;
        while present {
            if entry.th32OwnerProcessID == process_id {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = OwnedHandle::from_raw_handle(thread);
                if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            present = Thread32Next(snapshot.as_raw_handle(), &mut entry) != 0;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Owned process primary thread was not found",
    ))
}
