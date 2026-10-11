use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_QUEUE_TRACKS: usize = 2000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub recommended: bool,
}

#[derive(Clone, Debug)]
pub enum PlaybackCommand {
    Load { source: String, generation: u64 },
    TogglePause,
    SetPaused(bool),
    Seek(f64),
    SetVolume(f64),
    Stop,
    Shutdown,
}

#[derive(Clone, Debug, Default)]
pub struct PlaybackSnapshot {
    pub generation: u64,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub loading: bool,
    pub ended: bool,
    pub volume: f64,
    pub error: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Queue {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
}

impl Queue {
    /// Remove a row and keep the active track selected when it still exists.
    /// Removing the active row selects its successor, or the final predecessor.
    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index >= self.tracks.len() {
            return None;
        }
        let removed = self.tracks.remove(index);
        self.current = self.current.and_then(|current| {
            if self.tracks.is_empty() {
                None
            } else if index < current {
                Some(current.saturating_sub(1).min(self.tracks.len() - 1))
            } else {
                Some(current.min(self.tracks.len() - 1))
            }
        });
        Some(removed)
    }

    /// Insert a user choice before autoplay suggestions, keeping the active row stable.
    pub fn insert_manual(&mut self, mut track: Track, play_next: bool) -> bool {
        if self.tracks.len() >= MAX_QUEUE_TRACKS {
            return false;
        }
        track.recommended = false;
        let start = self.current.map_or(0, |i| i + 1).min(self.tracks.len());
        let index = if play_next {
            start
        } else {
            self.tracks
                .iter()
                .enumerate()
                .skip(start)
                .find(|(_, t)| t.recommended)
                .map_or(self.tracks.len(), |(i, _)| i)
        };
        self.tracks.insert(index, track);
        if self.current.is_none() {
            self.current = Some(0);
        }
        true
    }

    /// Move an upcoming row within its manual or autoplay group.
    pub fn move_upcoming(&mut self, index: usize, direction: i32) -> bool {
        let start = self.current.map_or(0, |i| i + 1);
        let target = match direction {
            -1 => index.checked_sub(1),
            1 => index.checked_add(1),
            _ => None,
        };
        let Some(target) = target else {
            return false;
        };
        if index < start
            || target < start
            || index >= self.tracks.len()
            || target >= self.tracks.len()
            || self.tracks[index].recommended != self.tracks[target].recommended
        {
            return false;
        }
        self.tracks.swap(index, target);
        true
    }
    pub fn remove_autoplay_upcoming(&mut self) {
        let start = self.current.map_or(0, |i| i + 1).min(self.tracks.len());
        let mut next = self.tracks.split_off(start);
        next.retain(|track| !track.recommended);
        self.tracks.extend(next);
    }

    /// Append recommendations once per source without changing the selection.
    /// Examine at most one queue's worth of inputs, even from an unbounded iterator.
    pub fn append_deduplicated(&mut self, tracks: impl IntoIterator<Item = Track>) -> usize {
        let mut sources: HashSet<String> = self.tracks.iter().map(|t| t.source.clone()).collect();
        let mut appended = 0;
        for track in tracks.into_iter().take(MAX_QUEUE_TRACKS) {
            if self.tracks.len() >= MAX_QUEUE_TRACKS {
                break;
            }
            if !track.source.trim().is_empty() && sources.insert(track.source.clone()) {
                self.tracks.push(track);
                appended += 1;
            }
        }
        appended
    }

    /// Randomize a freshly loaded playlist, including its first track.
    pub fn shuffle_all(&mut self, seed: u64) {
        self.current = None;
        self.shuffle_upcoming(seed);
        self.current = (!self.tracks.is_empty()).then_some(0);
    }

    /// Randomize upcoming tracks while keeping the current and played prefix.
    pub fn shuffle_upcoming(&mut self, mut seed: u64) {
        let start = self.current.map_or(0, |i| i + 1);
        if start >= self.tracks.len() {
            return;
        }
        self.tracks[start..].sort_by_key(|track| track.recommended);
        let split = self
            .tracks
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, track)| track.recommended)
            .map_or(self.tracks.len(), |(i, _)| i);
        for (from, to) in [(start, split), (split, self.tracks.len())] {
            for i in (from + 1..to).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = from + (seed as usize % (i - from + 1));
                self.tracks.swap(i, j);
            }
        }
    }

    pub fn select(&mut self, index: usize) -> Option<&Track> {
        if index >= self.tracks.len() {
            return None;
        }
        self.current = Some(index);
        self.tracks.get(index)
    }
    pub fn next_track(&mut self, repeat: bool) -> Option<&Track> {
        let index = self.current.map_or(0, |i| i + 1);
        if index >= self.tracks.len() && !repeat {
            return None;
        }
        if self.tracks.is_empty() {
            return None;
        }
        self.select(index % self.tracks.len())
    }
    pub fn previous_track(&mut self) -> Option<&Track> {
        self.select(self.current.unwrap_or(0).saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(title: &str) -> Track {
        Track {
            title: title.into(),
            artist: String::new(),
            album: String::new(),
            source: title.into(),
            artwork_url: None,
            recommended: false,
        }
    }
    #[test]
    fn reorder_cannot_move_current_or_cross_autoplay_group() {
        let mut auto = track("auto");
        auto.recommended = true;
        let mut q = Queue {
            tracks: vec![track("current"), track("one"), track("two"), auto],
            current: Some(0),
        };
        assert!(!q.move_upcoming(1, -1));
        assert!(q.move_upcoming(2, -1));
        assert_eq!(q.tracks[1].title, "two");
        assert!(!q.move_upcoming(2, 1));
        assert_eq!(q.current, Some(0));
    }
    #[test]
    fn disabling_autoplay_preserves_current_and_manual_choices() {
        let mut auto = track("auto");
        auto.recommended = true;
        let mut q = Queue {
            tracks: vec![auto.clone(), track("manual"), auto],
            current: Some(0),
        };
        q.remove_autoplay_upcoming();
        assert_eq!(q.tracks.len(), 2);
        assert_eq!(q.current, Some(0));
        assert_eq!(q.tracks[1].title, "manual");
    }

    #[test]
    fn manual_choices_precede_autoplay_without_changing_current() {
        let mut auto = track("auto");
        auto.recommended = true;
        let mut q = Queue {
            tracks: vec![track("playing"), track("chosen"), auto],
            current: Some(0),
        };
        assert!(q.insert_manual(track("added"), false));
        assert_eq!(
            q.tracks
                .iter()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            ["playing", "chosen", "added", "auto"]
        );
        assert!(q.insert_manual(track("next"), true));
        assert_eq!(q.tracks[1].title, "next");
        assert_eq!(q.current, Some(0));
        assert!(!q.tracks[1].recommended);
    }
    #[test]
    fn shuffle_keeps_manual_tracks_ahead_of_recommendations() {
        let mut a = track("a");
        a.recommended = true;
        let mut b = track("b");
        b.recommended = true;
        let mut q = Queue {
            tracks: vec![track("playing"), a, track("manual"), b],
            current: Some(0),
        };
        q.shuffle_upcoming(19);
        assert_eq!(q.tracks[0].title, "playing");
        assert_eq!(q.tracks[1].title, "manual");
        assert!(q.tracks[2..].iter().all(|t| t.recommended));
    }

    #[test]
    fn queue_does_not_wrap_without_repeat() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b")],
            current: None,
        };
        assert_eq!(q.next_track(false).unwrap().title, "a");
        assert_eq!(q.next_track(false).unwrap().title, "b");
        assert!(q.next_track(false).is_none());
        assert_eq!(q.current, Some(1));
        assert_eq!(q.next_track(true).unwrap().title, "a");
    }
    #[test]
    fn invalid_selection_preserves_current_track() {
        let mut q = Queue {
            tracks: vec![track("a")],
            current: Some(0),
        };
        assert!(q.select(2).is_none());
        assert_eq!(q.current, Some(0));
        assert_eq!(q.previous_track().unwrap().title, "a");
    }
    #[test]
    fn shuffle_preserves_current_and_all_upcoming_tracks() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b"), track("c"), track("d"), track("e")],
            current: Some(1),
        };
        q.shuffle_upcoming(123);
        assert_eq!(q.current, Some(1));
        assert_eq!(q.tracks[0].title, "a");
        assert_eq!(q.tracks[1].title, "b");
        let mut upcoming: Vec<_> = q.tracks[2..].iter().map(|t| t.title.as_str()).collect();
        upcoming.sort();
        assert_eq!(upcoming, vec!["c", "d", "e"]);
    }
    #[test]
    fn removal_preserves_active_identity_and_reselects_deleted_current() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b"), track("c"), track("d")],
            current: Some(1),
        };
        assert_eq!(q.remove(3).unwrap().source, "d");
        assert_eq!(q.current, Some(1));
        assert_eq!(q.tracks[q.current.unwrap()].source, "b");
        assert_eq!(q.remove(0).unwrap().source, "a");
        assert_eq!(q.current, Some(0));
        assert_eq!(q.tracks[q.current.unwrap()].source, "b");
        assert_eq!(q.remove(0).unwrap().source, "b");
        assert_eq!(q.current, Some(0));
        assert_eq!(q.tracks[q.current.unwrap()].source, "c");
        assert!(q.remove(1).is_none());
        assert_eq!(q.current, Some(0));
        assert_eq!(q.remove(0).unwrap().source, "c");
        assert!(q.tracks.is_empty());
        assert_eq!(q.current, None);
    }
    #[test]
    fn removal_of_final_current_selects_previous_and_unselected_queue_stays_unselected() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b")],
            current: Some(1),
        };
        q.remove(1);
        assert_eq!(q.current, Some(0));
        q.current = None;
        q.remove(0);
        assert_eq!(q.current, None);
    }
    #[test]
    fn append_skips_existing_and_incoming_duplicates_and_respects_capacity() {
        let mut q = Queue {
            tracks: vec![track("a")],
            current: Some(0),
        };
        assert_eq!(
            q.append_deduplicated([track("a"), track("b"), track("b"), track("")]),
            1
        );
        assert_eq!(q.current, Some(0));
        assert_eq!(
            q.append_deduplicated((0..MAX_QUEUE_TRACKS).map(|i| track(&format!("new-{i}")))),
            MAX_QUEUE_TRACKS - 2
        );
        assert_eq!(q.tracks.len(), MAX_QUEUE_TRACKS);
        assert_eq!(q.append_deduplicated([track("overflow")]), 0);
        assert_eq!(q.current, Some(0));
    }
    #[test]
    fn append_bounds_duplicate_only_iterators() {
        let mut q = Queue {
            tracks: vec![track("a")],
            current: Some(0),
        };
        assert_eq!(q.append_deduplicated(std::iter::repeat(track("a"))), 0);
    }
    #[test]
    fn shuffle_all_is_reproducible_and_can_change_first_track() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b"), track("c"), track("d"), track("e")],
            current: Some(3),
        };
        let mut copy = q.clone();
        q.shuffle_all(123);
        copy.shuffle_all(123);
        assert_eq!(q.tracks, copy.tracks);
        assert_eq!(q.current, Some(0));
        let mut titles: Vec<_> = q.tracks.iter().map(|t| t.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, vec!["a", "b", "c", "d", "e"]);
        assert!((0..10).any(|seed| {
            let mut other = copy.clone();
            other.shuffle_all(seed);
            other.tracks[0].source != "a"
        }));
        let mut empty = Queue::default();
        empty.shuffle_all(1);
        assert_eq!(empty.current, None);
    }
}
