//! Optional browser native-messaging receiver. No credentials or exceptions are
//! logged; stdout is reserved for one length-prefixed, credential-free reply.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

const MAX_MESSAGE: usize = 65536;
const FIREFOX_ID: &str = "tide-session@tide-music";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    action: String,
    headers: Headers,
}

#[derive(Default)]
struct Headers(BTreeMap<String, String>);
impl Drop for Headers {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}
impl<'de> Deserialize<'de> for Headers {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Headers;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("allowlisted session headers")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Headers, M::Error> {
                let mut result = Headers::default();
                while let Some((key, mut value)) = map.next_entry::<String, String>()? {
                    let key = key.to_ascii_lowercase();
                    let limit = match key.as_str() {
                        "cookie" => 32768,
                        "x-goog-authuser" => 4,
                        "x-goog-pageid" => 128,
                        "x-goog-visitor-id" => 8192,
                        "user-agent" => 1024,
                        "origin" => 128,
                        _ => {
                            value.zeroize();
                            return Err(serde::de::Error::custom("unsupported header"));
                        }
                    };
                    if value.is_empty()
                        || value.len() > limit
                        || value.chars().any(char::is_control)
                        || result.0.contains_key(&key)
                    {
                        value.zeroize();
                        return Err(serde::de::Error::custom("invalid header"));
                    }
                    result.0.insert(key, value);
                }
                Ok(result)
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[derive(Serialize)]
struct Reply {
    version: u8,
    ok: bool,
    code: &'static str,
}

fn read_message(input: &mut impl Read) -> Result<Zeroizing<Vec<u8>>, ()> {
    let mut length = [0; 4];
    input.read_exact(&mut length).map_err(|_| ())?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE {
        return Err(());
    }
    let mut payload = Zeroizing::new(vec![0; length]);
    input.read_exact(&mut payload).map_err(|_| ())?;
    Ok(payload)
}

fn chromium_origin(origin: &str) -> bool {
    origin
        .strip_prefix("chrome-extension://")
        .and_then(|s| s.strip_suffix('/'))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
}

fn allowed_caller(args: &[String]) -> bool {
    // Firefox passes the host-manifest path followed by the extension ID.
    if args.len() == 2 && args[1] == FIREFOX_ID && std::path::Path::new(&args[0]).is_absolute() {
        return true;
    }
    let Some(origin) = args.first().filter(|s| chromium_origin(s)) else {
        return false;
    };
    if args.len() > 2
        || args.get(1).is_some_and(|s| {
            !s.strip_prefix("--parent-window=")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
    {
        return false;
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Policy {
        allowed_origins: Vec<String>,
    }
    let policy = (|| {
        let path = std::env::current_exe()
            .ok()?
            .parent()?
            .join("tide-session-host-policy.json");
        let file = std::fs::File::open(path).ok()?;
        let mut data = Vec::new();
        file.take(2049).read_to_end(&mut data).ok()?;
        if data.len() > 2048 {
            return None;
        }
        serde_json::from_slice::<Policy>(&data).ok()
    })();
    policy.is_some_and(|p| {
        p.allowed_origins.len() <= 16
            && p.allowed_origins.iter().all(|s| chromium_origin(s))
            && p.allowed_origins.contains(origin)
    })
}

fn import_message(payload: &[u8], cancel: Arc<AtomicBool>) -> &'static str {
    let Ok(request) = serde_json::from_slice::<Request>(payload) else {
        return "invalid_request";
    };
    if request.version != 1 || request.action != "import_session" {
        return "invalid_request";
    }
    if !request.headers.0.contains_key("cookie")
        || !request
            .headers
            .0
            .get("x-goog-authuser")
            .is_some_and(|s| s.len() <= 4 && s.bytes().all(|b| b.is_ascii_digit()))
        || request
            .headers
            .0
            .get("origin")
            .is_some_and(|s| s != "https://music.youtube.com")
    {
        return "invalid_headers";
    }
    let Ok(encoded) = serde_json::to_vec(&request.headers.0) else {
        return "invalid_headers";
    };
    let encoded = Zeroizing::new(encoded);
    let Ok(provider) = tide_provider::Provider::new() else {
        return "storage_error";
    };
    use tide_provider::SessionImportError::*;
    match provider.import_browser_session(&encoded, cancel) {
        Ok(()) => "imported",
        Err(InvalidHeaders) => "invalid_headers",
        Err(SessionRejected) => "session_rejected",
        Err(SessionChanged) => "session_changed",
        Err(StorageError) => "storage_error",
        Err(VerificationUnavailable | Cancelled) => "internal_error",
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !allowed_caller(&args) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(read_message(&mut std::io::stdin().lock()));
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let code = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(payload)) => {
            // The browser keeps stdin open while awaiting its reply. Closing
            // the native port (Cancel) or sending an unsolicited second frame
            // invalidates the pending import before its storage commit.
            let cancelled = cancel.clone();
            std::thread::spawn(move || {
                let mut byte = [0];
                let _ = std::io::stdin().lock().read(&mut byte);
                cancelled.store(true, Ordering::Release);
            });
            import_message(&payload, cancel.clone())
        }
        _ => "invalid_request",
    };
    if cancel.load(Ordering::Acquire) {
        return;
    }
    let reply = Reply {
        version: 1,
        ok: code == "imported",
        code,
    };
    if let Ok(reply) = serde_json::to_vec(&reply) {
        let mut output = std::io::stdout().lock();
        let _ = output
            .write_all(&(reply.len() as u32).to_le_bytes())
            .and_then(|_| output.write_all(&reply))
            .and_then(|_| output.flush());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_rejects_empty_oversized_and_truncated_input() {
        for size in [0u32, 65537, u32::MAX] {
            assert!(read_message(&mut size.to_le_bytes().as_slice()).is_err());
        }
        assert!(read_message(&mut [2, 0, 0, 0, 1].as_slice()).is_err());
        assert_eq!(
            &**read_message(&mut [2, 0, 0, 0, 1, 2].as_slice()).unwrap(),
            &[1, 2]
        );
    }
    #[test]
    fn payload_rejects_duplicate_headers_unrelated_fields_and_credentials() {
        for data in [
            r#"{"version":1,"action":"import_session","headers":{"cookie":"one","Cookie":"two"}}"#,
            r#"{"version":1,"action":"import_session","headers":{"authorization":"private fixture"}}"#,
            r#"{"version":1,"action":"import_session","headers":{},"url":"https://elsewhere.invalid"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(data).is_err());
        }
    }
    #[test]
    fn invalid_messages_do_not_access_session_storage() {
        for data in [
            b"{}".as_slice(),
            br#"{"version":1,"action":"import_session","headers":{"cookie":"fixture"}}"#,
        ] {
            assert!(matches!(
                import_message(data, Arc::new(AtomicBool::new(false))),
                "invalid_request" | "invalid_headers"
            ));
        }
    }
    #[test]
    fn browser_origins_are_exact_and_never_wildcards() {
        assert!(chromium_origin(&format!(
            "chrome-extension://{}/",
            "a".repeat(32)
        )));
        for origin in [
            "chrome-extension://*/",
            "https://music.youtube.com",
            "chrome-extension://short/",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaz/",
        ] {
            assert!(!chromium_origin(origin));
        }
        assert!(!allowed_caller(&[]));
        assert!(!allowed_caller(&[
            "relative-manifest.json".into(),
            FIREFOX_ID.into()
        ]));
    }
}
