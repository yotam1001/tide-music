//! Local-only login probes. Never perform network work or expose credentials.
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tide_provider::{Provider, SavedConnection};

const PROBE_INTERVAL: Duration = Duration::from_secs(2);
pub struct AdoptionRetry {
    failures: u8,
    next_attempt: Option<Instant>,
}
impl AdoptionRetry {
    pub fn new() -> Self {
        Self {
            failures: 0,
            next_attempt: None,
        }
    }
    pub fn ready(&self) -> bool {
        self.next_attempt.is_none_or(|next| Instant::now() >= next)
    }
    pub fn failed(&mut self) -> bool {
        self.failures += 1;
        if self.failures >= 3 {
            self.next_attempt = None;
            return false;
        }
        self.next_attempt = Some(Instant::now() + PROBE_INTERVAL);
        true
    }
}
type Probe = (
    Option<SavedConnection>,
    Result<Option<SavedConnection>, String>,
);

pub struct Monitor {
    tx: mpsc::SyncSender<Probe>,
    rx: mpsc::Receiver<Probe>,
    worker: Option<thread::JoinHandle<()>>,
    next_probe: Instant,
}
impl Monitor {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        Self {
            tx,
            rx,
            worker: None,
            next_probe: Instant::now() + PROBE_INTERVAL,
        }
    }
    pub fn poll(
        &mut self,
        provider: Option<&Provider>,
        current: &Option<SavedConnection>,
    ) -> Option<Option<SavedConnection>> {
        if let Ok((baseline, result)) = self.rx.try_recv() {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            if let Some(change) = accept_probe(&baseline, current, result) {
                return Some(change);
            }
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
        if self.worker.is_none() && Instant::now() >= self.next_probe {
            self.next_probe = Instant::now() + PROBE_INTERVAL;
            if let Some(provider) = provider {
                let provider = provider.clone();
                let baseline = current.clone();
                let tx = self.tx.clone();
                self.worker = Some(thread::spawn(move || {
                    let _ = tx.try_send((baseline, provider.saved_connection()));
                }));
            }
        }
        None
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn accept_probe(
    baseline: &Option<SavedConnection>,
    current: &Option<SavedConnection>,
    result: Result<Option<SavedConnection>, String>,
) -> Option<Option<SavedConnection>> {
    // A picker or other local account action may have changed state while this
    // read was in flight. Never restore that probe's superseded selection.
    if baseline != current {
        return None;
    }
    let saved = result.ok()?;
    (saved != *current).then_some(saved)
}
pub fn same_identity(
    previous: &Option<SavedConnection>,
    current: &Option<SavedConnection>,
) -> bool {
    match (previous, current) {
        (Some(previous), Some(current)) => {
            previous.account_index == current.account_index && previous.page_id == current.page_id
        }
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn connection(revision: &str, account_index: u32, page: &str) -> Option<SavedConnection> {
        Some(SavedConnection {
            revision: Some(revision.into()),
            account_index,
            page_id: Some(page.into()),
        })
    }
    #[test]
    fn reconnect_is_detected_without_changing_identity() {
        let old = connection("old", 1, "123");
        let new = connection("new", 1, "123");
        assert!(same_identity(&old, &new));
        assert!(accept_probe(&old, &old, Ok(new.clone())) == Some(new));
        assert!(accept_probe(&old, &old, Ok(old.clone())).is_none());
    }
    #[test]
    fn brand_and_google_account_changes_require_new_playback_credentials() {
        let old = connection("old", 1, "123");
        assert!(!same_identity(&old, &connection("new", 1, "456")));
        assert!(!same_identity(&old, &connection("new", 2, "123")));
        assert!(!same_identity(&None, &old));
        assert!(!same_identity(&old, &None));
    }
    #[test]
    fn stale_probe_and_read_failure_cannot_restore_or_disconnect_an_account() {
        let old = connection("old", 1, "123");
        let new = connection("new", 2, "456");
        assert!(accept_probe(&old, &new, Ok(old.clone())).is_none());
        assert!(accept_probe(&old, &None, Ok(old.clone())).is_none());
        assert!(accept_probe(&old, &old, Err("busy".into())).is_none());
        assert!(accept_probe(&old, &old, Ok(None)) == Some(None));
    }
}
