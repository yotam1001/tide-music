//! XDG GlobalShortcuts portal (KDE Plasma, GNOME 48+, Hyprland and others).
//!
//! The desktop shows its own confirmation the first time Tide binds chords and
//! may let the user pick different keys; Tide only proposes the saved chords.
//! Portal round trips can wait on that dialog, so they run on a worker thread
//! and never block the UI.
use super::{fire, Callback, Chord};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const IDS: [&str; 3] = ["play-pause", "next-song", "previous-song"];
const DESCRIPTIONS: [&str; 3] = ["Play / pause", "Next song", "Previous song"];

pub(super) struct PortalShortcuts {
    requests: mpsc::Sender<[Option<Chord>; 3]>,
}

impl PortalShortcuts {
    pub(super) fn new(callback: Callback) -> Result<Self, String> {
        let unavailable = || "The desktop has no global shortcuts portal.".to_string();
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build())
            .map_err(|_| unavailable())?;
        let proxy = Proxy::new(&conn, DEST, PATH, IFACE).map_err(|_| unavailable())?;
        proxy
            .get_property::<u32>("version")
            .map_err(|_| unavailable())?;
        let session: Arc<Mutex<Option<OwnedObjectPath>>> = Arc::default();
        // Activations: matched to the live session so a stale binding never fires.
        {
            let mut signals = proxy
                .receive_signal("Activated")
                .map_err(|_| unavailable())?;
            let session = session.clone();
            std::thread::Builder::new()
                .name("tide-shortcuts-portal".into())
                .spawn(move || {
                    for message in &mut signals {
                        let body = message.body();
                        let Ok((handle, id, _, _)) = body.deserialize::<(
                            OwnedObjectPath,
                            String,
                            u64,
                            HashMap<String, OwnedValue>,
                        )>() else {
                            continue;
                        };
                        let current = session
                            .lock()
                            .ok()
                            .is_some_and(|session| session.as_ref() == Some(&handle));
                        if let Some(slot) = IDS.iter().position(|known| *known == id) {
                            if current {
                                fire(&callback, slot);
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let (requests, receive) = mpsc::channel::<[Option<Chord>; 3]>();
        std::thread::Builder::new()
            .name("tide-shortcuts-bind".into())
            .spawn(move || {
                // Binding waits for the user's answer, so drop the call timeout.
                let Ok(conn) = zbus::blocking::connection::Builder::session()
                    .and_then(|builder| builder.method_timeout(Duration::from_secs(600)).build())
                else {
                    return;
                };
                while let Ok(mut chords) = receive.recv() {
                    // Coalesce rapid edits into the newest configuration.
                    while let Ok(newer) = receive.try_recv() {
                        chords = newer;
                    }
                    let previous = session.lock().ok().and_then(|mut s| s.take());
                    if let Some(previous) = previous {
                        let _ = conn.call_method(
                            Some(DEST),
                            previous.as_str(),
                            Some("org.freedesktop.portal.Session"),
                            "Close",
                            &(),
                        );
                    }
                    if chords.iter().all(Option::is_none) {
                        continue;
                    }
                    if let Ok(handle) = bind(&conn, &chords) {
                        if let Ok(mut current) = session.lock() {
                            *current = Some(handle);
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { requests })
    }

    pub(super) fn apply(&mut self, chords: [Option<Chord>; 3]) -> Result<(), String> {
        self.requests
            .send(chords)
            .map_err(|_| "Global shortcuts stopped. Restart Tide.".to_string())
    }
}

fn token() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    format!(
        "tide_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Call a portal method that answers through a `Request.Response` signal.
fn request(
    conn: &Connection,
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    token: &str,
) -> Result<HashMap<String, OwnedValue>, String> {
    let sender = conn
        .unique_name()
        .ok_or("No D-Bus name")?
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{PATH}/request/{sender}/{token}");
    // Subscribe before calling, so a fast answer cannot be missed.
    let request = Proxy::new(conn, DEST, path.as_str(), "org.freedesktop.portal.Request")
        .map_err(|e| e.to_string())?;
    let mut responses = request
        .receive_signal("Response")
        .map_err(|e| e.to_string())?;
    conn.call_method(Some(DEST), PATH, Some(IFACE), method, body)
        .map_err(|e| e.to_string())?;
    let message = responses.next().ok_or("The portal did not answer")?;
    let (code, results) = message
        .body()
        .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        .map_err(|e| e.to_string())?;
    if code != 0 {
        return Err("Global shortcuts were not allowed".into());
    }
    Ok(results)
}

fn bind(conn: &Connection, chords: &[Option<Chord>; 3]) -> Result<OwnedObjectPath, String> {
    let (handle_token, session_token) = (token(), token());
    let options: HashMap<&str, Value> = HashMap::from([
        ("handle_token", Value::from(handle_token.as_str())),
        ("session_handle_token", Value::from(session_token.as_str())),
    ]);
    let results = request(conn, "CreateSession", &(options,), &handle_token)?;
    let handle = results
        .get("session_handle")
        .and_then(|value| match &**value {
            Value::Str(text) => OwnedObjectPath::try_from(text.as_str().to_owned()).ok(),
            Value::ObjectPath(path) => Some(OwnedObjectPath::from(path.to_owned())),
            _ => None,
        })
        .ok_or("The portal returned no shortcut session")?;
    let triggers: Vec<String> = chords
        .iter()
        .map(|chord| chord.map(|c| c.portal_trigger()).unwrap_or_default())
        .collect();
    let shortcuts: Vec<(&str, HashMap<&str, Value>)> = (0..3)
        .filter(|slot| chords[*slot].is_some())
        .map(|slot| {
            (
                IDS[slot],
                HashMap::from([
                    ("description", Value::from(DESCRIPTIONS[slot])),
                    ("preferred_trigger", Value::from(triggers[slot].as_str())),
                ]),
            )
        })
        .collect();
    let bind_token = token();
    let options: HashMap<&str, Value> =
        HashMap::from([("handle_token", Value::from(bind_token.as_str()))]);
    request(
        conn,
        "BindShortcuts",
        &(handle.as_ref(), shortcuts, "", options),
        &bind_token,
    )?;
    Ok(handle)
}
