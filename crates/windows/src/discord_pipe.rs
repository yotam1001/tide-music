//! Bounded Discord IPC transport. No browser, TCP listener, token or SDK process.
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
        IO::OVERLAPPED,
    },
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ReadFile(
        file: HANDLE,
        buffer: *mut u8,
        size: u32,
        done: *mut u32,
        overlapped: *mut OVERLAPPED,
    ) -> i32;
    fn WriteFile(
        file: HANDLE,
        buffer: *const u8,
        size: u32,
        done: *mut u32,
        overlapped: *mut OVERLAPPED,
    ) -> i32;
    fn GetOverlappedResult(
        file: HANDLE,
        overlapped: *mut OVERLAPPED,
        done: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(file: HANDLE, overlapped: *mut OVERLAPPED) -> i32;
    fn PeekNamedPipe(
        file: HANDLE,
        buffer: *mut u8,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        remaining: *mut u32,
    ) -> i32;
}

pub struct DiscordPipe(File);
impl DiscordPipe {
    pub fn connect() -> io::Result<Self> {
        for index in 0..10 {
            // Identification-only prevents a malicious local pipe impersonating Tide.
            if let Ok(file) = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(0x40000000 | 0x00100000 | 0x00010000)
                .open(format!(r"\\.\pipe\discord-ipc-{index}"))
            {
                return Ok(Self(file));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "Discord desktop is not available",
        ))
    }
    pub fn send(&self, opcode: u32, payload: &[u8], deadline: Instant) -> io::Result<()> {
        if payload.len() > 16384 {
            return Err(io::Error::other("Discord message too large"));
        }
        let mut packet = Vec::with_capacity(payload.len() + 8);
        packet.extend(opcode.to_le_bytes());
        packet.extend((payload.len() as u32).to_le_bytes());
        packet.extend(payload);
        self.transfer(&mut packet, true, deadline)
    }
    pub fn receive(&self, deadline: Instant) -> io::Result<(u32, Vec<u8>)> {
        let mut header = [0u8; 8];
        self.transfer(&mut header, false, deadline)?;
        let opcode = u32::from_le_bytes(header[..4].try_into().unwrap());
        let len = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if len > 16384 {
            return Err(io::Error::other("Discord response too large"));
        }
        let mut data = vec![0; len];
        self.transfer(&mut data, false, deadline)?;
        Ok((opcode, data))
    }
    fn transfer(&self, bytes: &mut [u8], write: bool, deadline: Instant) -> io::Result<()> {
        let handle = HANDLE(self.0.as_raw_handle());
        let event = unsafe { CreateEventW(None, true, false, None) }.map_err(io::Error::other)?;
        // The event and OVERLAPPED live until completion, including cancellation.
        let result = (|| {
            let mut offset = 0;
            while offset < bytes.len() {
                let timeout = deadline.saturating_duration_since(Instant::now());
                if timeout.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                unsafe {
                    ResetEvent(event).map_err(io::Error::other)?;
                }
                let mut operation = OVERLAPPED {
                    hEvent: event,
                    ..Default::default()
                };
                let mut done = 0;
                let ok = unsafe {
                    if write {
                        WriteFile(
                            handle,
                            bytes[offset..].as_ptr(),
                            (bytes.len() - offset) as u32,
                            &mut done,
                            &mut operation,
                        )
                    } else {
                        ReadFile(
                            handle,
                            bytes[offset..].as_mut_ptr(),
                            (bytes.len() - offset) as u32,
                            &mut done,
                            &mut operation,
                        )
                    }
                };
                if ok == 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(997) {
                        return Err(error);
                    }
                    let completed = unsafe {
                        WaitForSingleObject(event, timeout.as_millis().clamp(1, 500) as u32)
                    }
                    .0 == 0;
                    if !completed {
                        unsafe {
                            CancelIoEx(handle, &mut operation);
                            GetOverlappedResult(handle, &mut operation, &mut done, 1);
                        }
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                    if unsafe { GetOverlappedResult(handle, &mut operation, &mut done, 0) } == 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if done == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                offset += done as usize;
            }
            Ok(())
        })();
        unsafe {
            let _ = CloseHandle(event);
        }
        result
    }
    /// Nonblocking idle poll: service Discord pings and detect closed clients.
    pub fn poll(&self) -> io::Result<Option<(u32, Vec<u8>)>> {
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                HANDLE(self.0.as_raw_handle()),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if available == 0 {
            return Ok(None);
        }
        self.receive(Self::deadline()).map(Some)
    }
    pub fn deadline() -> Instant {
        Instant::now() + Duration::from_millis(500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::windows::io::FromRawHandle,
        sync::atomic::{AtomicU64, Ordering},
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open: u32,
            mode: u32,
            instances: u32,
            out: u32,
            input: u32,
            timeout: u32,
            security: *const std::ffi::c_void,
        ) -> HANDLE;
        fn ConnectNamedPipe(pipe: HANDLE, overlapped: *mut OVERLAPPED) -> i32;
    }
    fn pair(
        server: impl FnOnce(File) + Send + 'static,
    ) -> (DiscordPipe, std::thread::JoinHandle<()>) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            r"\\.\pipe\tide-discord-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let handle =
            unsafe { CreateNamedPipeW(wide.as_ptr(), 3, 0, 1, 4096, 4096, 0, std::ptr::null()) };
        assert_ne!(handle.0 as isize, -1);
        let file = unsafe { File::from_raw_handle(handle.0) };
        let thread = std::thread::spawn(move || {
            unsafe {
                ConnectNamedPipe(HANDLE(file.as_raw_handle()), std::ptr::null_mut());
            }
            server(file);
        });
        let client = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x40000000 | 0x00100000 | 0x00010000)
            .open(name)
            .unwrap();
        (DiscordPipe(client), thread)
    }
    #[test]
    fn fragmented_frames_and_pending_completion() {
        let (pipe, server) = pair(|mut file| {
            let mut request = [0; 11];
            file.read_exact(&mut request).unwrap();
            assert_eq!(&request[8..], b"abc");
            // Split reads exercise pending I/O and event reset, not only buffered success.
            file.write_all(&1u32.to_le_bytes()).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            file.write_all(&2u32.to_le_bytes()).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            file.write_all(b"ok").unwrap();
        });
        pipe.send(0, b"abc", DiscordPipe::deadline()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let reply = loop {
            if let Some(reply) = pipe.poll().unwrap() {
                break reply;
            }
            assert!(
                Instant::now() < deadline,
                "Idle poll never received the fragmented reply"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(reply, (1, b"ok".to_vec()));
        server.join().unwrap();
    }
    #[test]
    fn stalled_peer_times_out_and_cancels_safely() {
        let (pipe, server) = pair(|_file| std::thread::sleep(Duration::from_millis(700)));
        let started = Instant::now();
        assert_eq!(
            pipe.receive(started + Duration::from_millis(100))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(pipe);
        server.join().unwrap();
    }
    #[test]
    fn oversized_peer_is_rejected_before_allocation() {
        let (pipe, server) = pair(|mut file| {
            file.write_all(&1u32.to_le_bytes()).unwrap();
            file.write_all(&u32::MAX.to_le_bytes()).unwrap();
        });
        assert!(pipe.receive(DiscordPipe::deadline()).is_err());
        assert!(pipe
            .send(1, &vec![0; 16385], DiscordPipe::deadline())
            .is_err());
        server.join().unwrap();
    }
}
