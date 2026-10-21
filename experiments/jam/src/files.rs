//! Private local-file inputs only. Never open a pipe/device as an invite or key.
use std::{fs, io::Read, path::Path};
use zeroize::Zeroizing;

const LOCAL_FILE: &str = "Select an ordinary private file in a local folder";

/// Lexical checks happen before touching a potentially remote/device namespace.
/// This does not identify network-mapped drives or provide a filesystem deadline.
pub fn check_private_path(path: &Path) -> Result<(), &'static str> {
    if path.as_os_str().is_empty() {
        return Err(LOCAL_FILE);
    }
    #[cfg(windows)]
    for component in path.components() {
        use std::path::{Component, Prefix};
        match component {
            Component::Prefix(prefix)
                if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
            {
                return Err(LOCAL_FILE);
            }
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                let base = name.split('.').next().unwrap_or("").trim_end_matches(' ');
                let base = base.to_uppercase();
                let numbered_device = ["COM", "LPT"].iter().any(|prefix| {
                    base.strip_prefix(prefix).is_some_and(|number| {
                        matches!(
                            number,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                });
                if name.contains(':')
                    || matches!(
                        base.as_str(),
                        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
                    )
                    || numbered_device
                {
                    return Err(LOCAL_FILE);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_regular(metadata: &fs::Metadata, limit: usize) -> Result<(), &'static str> {
    if !metadata.file_type().is_file() {
        return Err(LOCAL_FILE);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT: reject the selected link/placeholder.
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(LOCAL_FILE);
        }
    }
    if metadata.len() > limit as u64 {
        return Err("Private Jam file is too large");
    }
    Ok(())
}

pub fn read_private(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    check_private_path(path)?;
    if limit > crate::protocol::MAX_BODY {
        return Err("Private Jam file limit is too large");
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| "Cannot open private Jam file")?;
    check_regular(&metadata, limit)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the selected reparse point itself, not its target; then check
        // the opened handle. Share reads only while consuming the private file.
        // https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-createfilew
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot open private Jam file")?;
    check_regular(
        &file
            .metadata()
            .map_err(|_| "Cannot inspect private Jam file")?,
        limit,
    )?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read private Jam file")?;
    if bytes.len() > limit {
        return Err("Private Jam file is too large");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tide-jam-input-{}",
                crate::protocol::room_id().unwrap()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            for name in ["input", "link", "socket"] {
                let _ = fs::remove_file(self.0.join(name));
            }
            let _ = fs::remove_dir(&self.0);
        }
    }
    #[test]
    fn ordinary_private_files_are_bounded_and_directories_are_rejected() {
        let scratch = Scratch::new();
        let path = scratch.0.join("input");
        fs::write(&path, b"synthetic fixture").unwrap();
        assert_eq!(
            read_private(&path, 17).unwrap().as_slice(),
            b"synthetic fixture"
        );
        assert_eq!(
            read_private(&path, 16).unwrap_err(),
            "Private Jam file is too large"
        );
        assert!(read_private(&scratch.0, 32).is_err());
        assert!(read_private(&path, crate::protocol::MAX_BODY + 1).is_err());
        assert!(check_private_path(Path::new("")).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_device_share_and_alternate_stream_paths_are_rejected_lexically() {
        for path in [
            r"\\.\pipe\test.jam-invite.json",
            r"\\?\GLOBALROOT\Device\test",
            r"\\server.invalid\share\test.jam-invite.json",
            r"\\?\UNC\server.invalid\share\test",
            "NUL.jam-invite.json",
            "con.jam-operator-key",
            "COM1.json",
            "LPT².json",
            "CONOUT$.json",
            "NUL .json",
            "file:stream.json",
        ] {
            assert!(
                check_private_path(Path::new(path)).is_err(),
                "accepted special path"
            );
        }
        for path in [
            r"C:\private\test.jam-invite.json",
            r"\\?\C:\private\test.jam-invite.json",
            "private/test.jam-invite.json",
        ] {
            assert!(check_private_path(Path::new(path)).is_ok());
        }
    }
    #[cfg(unix)]
    #[test]
    fn unix_links_and_nonregular_sockets_are_rejected_without_opening() {
        let scratch = Scratch::new();
        let path = scratch.0.join("input");
        fs::write(&path, b"fixture").unwrap();
        std::os::unix::fs::symlink(&path, scratch.0.join("link")).unwrap();
        assert!(read_private(&scratch.0.join("link"), 32).is_err());
        let _socket = std::os::unix::net::UnixListener::bind(scratch.0.join("socket")).unwrap();
        assert!(read_private(&scratch.0.join("socket"), 32).is_err());
    }
}
