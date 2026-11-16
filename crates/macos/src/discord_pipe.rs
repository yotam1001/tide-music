//! Bounded, deadline-aware Discord Unix IPC.
use std::{
    io::{self, Read, Write},
    os::unix::{io::AsRawFd, net::UnixStream},
    time::{Duration, Instant},
};
pub struct DiscordPipe(UnixStream);
impl DiscordPipe {
    pub fn connect() -> io::Result<Self> {
        for dir in [
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::var_os("TMPDIR"),
            Some("/tmp".into()),
        ]
        .into_iter()
        .flatten()
        {
            for index in 0..10 {
                if let Ok(socket) = UnixStream::connect(
                    std::path::Path::new(&dir).join(format!("discord-ipc-{index}")),
                ) {
                    socket.set_nonblocking(true)?;
                    return Ok(Self(socket));
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "Discord desktop is not available",
        ))
    }
    fn ready(&self, write: bool, deadline: Instant) -> io::Result<()> {
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(io::ErrorKind::TimedOut)?;
            let mut fd = libc::pollfd {
                fd: self.0.as_raw_fd(),
                events: if write { libc::POLLOUT } else { libc::POLLIN },
                revents: 0,
            };
            let result = unsafe {
                libc::poll(
                    &mut fd,
                    1,
                    remaining.as_millis().clamp(1, i32::MAX as u128) as i32,
                )
            };
            if result > 0 {
                return Ok(());
            }
            if result == 0 {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    fn transfer(&self, bytes: &mut [u8], write: bool, deadline: Instant) -> io::Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            self.ready(write, deadline)?;
            let result = if write {
                (&self.0).write(&bytes[offset..])
            } else {
                (&self.0).read(&mut bytes[offset..])
            };
            match result {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
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
        let mut header = [0; 8];
        self.transfer(&mut header, false, deadline)?;
        let opcode = u32::from_le_bytes(header[..4].try_into().unwrap());
        let len = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if len > 16384 {
            return Err(io::Error::other("Discord response too large"));
        }
        let mut bytes = vec![0; len];
        self.transfer(&mut bytes, false, deadline)?;
        Ok((opcode, bytes))
    }
    pub fn poll(&self) -> io::Result<Option<(u32, Vec<u8>)>> {
        let mut fd = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut fd, 1, 0) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if result == 0 {
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
    #[test]
    fn fragmented_and_bounded() {
        let (a, mut b) = UnixStream::pair().unwrap();
        a.set_nonblocking(true).unwrap();
        let pipe = DiscordPipe(a);
        assert!(pipe.poll().unwrap().is_none());
        let worker = std::thread::spawn(move || {
            b.write_all(&1u32.to_le_bytes()).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            b.write_all(&2u32.to_le_bytes()).unwrap();
            b.write_all(b"ok").unwrap();
        });
        assert_eq!(
            pipe.receive(SelfDeadline::deadline()).unwrap(),
            (1, b"ok".to_vec())
        );
        worker.join().unwrap();
        assert!(pipe
            .send(1, &vec![0; 16385], DiscordPipe::deadline())
            .is_err());
    }
    type SelfDeadline = DiscordPipe;
    #[test]
    fn stalled_peer_times_out() {
        let (a, _b) = UnixStream::pair().unwrap();
        a.set_nonblocking(true).unwrap();
        assert_eq!(
            DiscordPipe(a)
                .receive(Instant::now() + Duration::from_millis(20))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
