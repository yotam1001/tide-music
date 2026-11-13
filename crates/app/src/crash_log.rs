//! Keep the last panic on disk. Release builds have no console, so without
//! this a crash leaves no trace. The file stays local next to the settings and
//! holds only the panic message, its location and a backtrace.
use std::{fs, io::Write, panic::PanicHookInfo, path::PathBuf};

const LIMIT: usize = 64 * 1024;

/// Chain a hook that records each panic to `path` after the default output.
pub(super) fn install(path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned();
        let text = report(info, &thread, &backtrace);
        if let Some(folder) = path.parent() {
            let _ = fs::create_dir_all(folder);
        }
        if let Ok(mut file) = fs::File::create(&path) {
            let _ = file.write_all(text.as_bytes());
        }
    }));
}

fn report(info: &PanicHookInfo<'_>, thread: &str, backtrace: &str) -> String {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-text panic".into());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "unknown".into());
    bounded(format!(
        "Tide {} crashed\nthread: {thread}\nmessage: {message}\nlocation: {location}\n\n{backtrace}",
        env!("CARGO_PKG_VERSION"),
    ))
}

fn bounded(mut text: String) -> String {
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[truncated]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_bounded_on_a_character_boundary() {
        let text = bounded("é".repeat(LIMIT));
        assert!(text.len() <= LIMIT + "\n[truncated]".len());
        assert!(text.ends_with("[truncated]"));
        assert_eq!(bounded("short".into()), "short");
    }

    #[test]
    fn panic_message_and_location_are_recorded() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let sink = captured.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Other tests may panic meanwhile; keep only this one.
            let text = report(info, "main", "frames");
            if text.contains("Recursion detected") {
                *sink.lock().unwrap() = text;
            }
        }));
        let _ = std::panic::catch_unwind(|| panic!("Recursion detected"));
        std::panic::set_hook(previous);
        let text = captured.lock().unwrap().clone();
        assert!(text.contains("message: Recursion detected"), "{text}");
        assert!(text.contains("crash_log.rs"), "{text}");
        assert!(
            text.contains("thread: main") && text.ends_with("frames"),
            "{text}"
        );
    }
}
