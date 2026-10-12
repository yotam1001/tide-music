//! Bounded, in-memory credential updates; never format or serialize these.
use std::collections::VecDeque;
use std::sync::Mutex;
use zeroize::Zeroizing;

pub(crate) const MAX_COOKIE_BYTES: usize = 65536;
const MAX_UPDATES: usize = 4;

pub struct CookieUpdate {
    pub account_index: u32,
    baseline: Zeroizing<Vec<u8>>,
    updated: Zeroizing<Vec<u8>>,
}

impl CookieUpdate {
    pub fn baseline(&self) -> &[u8] {
        &self.baseline
    }
    pub fn updated(&self) -> &[u8] {
        &self.updated
    }
    pub(crate) fn new(account_index: u32, baseline: Vec<u8>, updated: Vec<u8>) -> Option<Self> {
        let update = Self {
            account_index,
            baseline: Zeroizing::new(baseline),
            updated: Zeroizing::new(updated),
        };
        (account_index < 10
            && update.baseline.len() <= MAX_COOKIE_BYTES
            && update.updated.len() <= MAX_COOKIE_BYTES
            && update.baseline != update.updated)
            .then_some(update)
    }
}

#[derive(Default)]
pub(crate) struct CookieUpdates(Mutex<VecDeque<CookieUpdate>>);

impl CookieUpdates {
    pub(crate) fn push(&self, update: CookieUpdate) -> bool {
        let mut queue = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.len() == MAX_UPDATES {
            // Preserve the earliest outstanding baseline; evicting it could
            // make subsequent updates impossible to merge without rollback.
            return false;
        }
        queue.push_back(update);
        true
    }
    pub(crate) fn take(&self) -> Vec<CookieUpdate> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_preserves_order_is_bounded_and_drains_owned_updates() {
        let queue = CookieUpdates::default();
        for index in 0..MAX_UPDATES {
            assert!(
                queue.push(CookieUpdate::new(1, vec![index as u8], vec![index as u8 + 1]).unwrap())
            );
        }
        assert!(!queue.push(
            CookieUpdate::new(
                1,
                b"overflow baseline".to_vec(),
                b"overflow changed".to_vec()
            )
            .unwrap()
        ));
        let updates = queue.take();
        assert_eq!(updates.len(), MAX_UPDATES);
        for (index, update) in updates.iter().enumerate() {
            assert_eq!(update.account_index, 1);
            assert_eq!(update.baseline(), &[index as u8]);
            assert_eq!(update.updated(), &[index as u8 + 1]);
        }
        assert!(queue.take().is_empty());
        assert!(queue.push(
            CookieUpdate::new(1, b"new baseline".to_vec(), b"new changed".to_vec()).unwrap()
        ));
    }
    #[test]
    fn unchanged_invalid_and_oversized_updates_are_not_queued() {
        assert!(CookieUpdate::new(0, b"same".to_vec(), b"same".to_vec()).is_none());
        assert!(CookieUpdate::new(10, vec![0], vec![1]).is_none());
        assert!(CookieUpdate::new(0, vec![0; MAX_COOKIE_BYTES + 1], vec![1]).is_none());
        assert!(CookieUpdate::new(0, vec![0], vec![1; MAX_COOKIE_BYTES + 1]).is_none());
    }
}
