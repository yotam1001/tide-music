use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_QUEUE_TRACKS: usize = 2000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Artist {
    pub id: String,
    pub name: String,
}
impl Artist {
    pub fn is_valid(&self) -> bool {
        self.id.len() == 24
            && self.id.starts_with("UC")
            && self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            && !self.name.trim().is_empty()
            && self.name.chars().count() <= 300
            && !self.name.chars().any(char::is_control)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub artists: Vec<Artist>,
    pub album: String,
    pub source: String,
    #[serde(default)]
    pub duration_seconds: Option<u64>,
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

#[derive(Clone, Debug)]
pub struct PlaybackSnapshot {
    pub generation: u64,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub loading: bool,
    pub buffering: bool,
    pub retry_attempt: u8,
    pub audio_codec: Option<String>,
    pub audio_bitrate_bps: Option<u64>,
    pub audio_sample_rate: Option<u32>,
    pub ended: bool,
    pub volume: f64,
    pub error: Option<String>,
    /// Classified audio-output failure; never an authentication/network error.
    pub output_unavailable: bool,
    pub title: Option<String>,
}

impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            generation: 0,
            position: 0.0,
            duration: 0.0,
            paused: true,
            loading: false,
            buffering: false,
            retry_attempt: 0,
            audio_codec: None,
            audio_bitrate_bps: None,
            audio_sample_rate: None,
            ended: false,
            volume: 100.0,
            error: None,
            output_unavailable: false,
            title: None,
        }
    }
}

/// SplitMix64 for reproducible, non-cryptographic playlist shuffling.
fn shuffle_index(seed: &mut u64, bound: usize) -> usize {
    let bound = bound as u64;
    let threshold = bound.wrapping_neg() % bound;
    loop {
        *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = *seed;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^= value >> 31;
        // Rejection avoids modulo bias, including for non-power-of-two bounds.
        if value >= threshold {
            return (value % bound) as usize;
        }
    }
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
        let start = self
            .current
            .map_or(0, |i| i.saturating_add(1))
            .min(self.tracks.len());
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
        let start = self.current.map_or(0, |i| i.saturating_add(1));
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

    /// Move an upcoming row to its final absolute queue index, shifting intervening rows.
    /// Both positions must belong to the same contiguous manual or autoplay section.
    /// Played/current rows, invalid positions, and no-op drops leave the queue unchanged.
    pub fn move_upcoming_to(&mut self, source: usize, target: usize) -> bool {
        let start = self.current.map_or(0, |i| i.saturating_add(1));
        if source == target
            || source < start
            || target < start
            || source >= self.tracks.len()
            || target >= self.tracks.len()
        {
            return false;
        }
        let recommended = self.tracks[source].recommended;
        if self.tracks[source.min(target)..=source.max(target)]
            .iter()
            .any(|track| track.recommended != recommended)
        {
            return false;
        }
        let track = self.tracks.remove(source);
        self.tracks.insert(target, track);
        true
    }
    pub fn remove_autoplay_upcoming(&mut self) {
        let start = self
            .current
            .map_or(0, |i| i.saturating_add(1))
            .min(self.tracks.len());
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

    /// Keep a bounded recent history while retaining the current and all upcoming
    /// songs. Long radio sessions must not fill the queue with already-played rows.
    pub fn trim_played_history(&mut self, keep: usize) {
        if let Some(current) = self.current.filter(|i| *i < self.tracks.len()) {
            let removed = current.saturating_sub(keep);
            if removed > 0 {
                self.tracks.drain(..removed);
                self.current = Some(current - removed);
            }
        }
    }

    /// Randomize a freshly loaded playlist, including its first track.
    pub fn shuffle_all(&mut self, seed: u64) {
        self.current = None;
        self.shuffle_upcoming(seed);
        self.current = (!self.tracks.is_empty()).then_some(0);
    }

    /// Randomize upcoming tracks while keeping the current and played prefix.
    pub fn shuffle_upcoming(&mut self, mut seed: u64) {
        let start = self.current.map_or(0, |i| i.saturating_add(1));
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
                let j = from + shuffle_index(&mut seed, i - from + 1);
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
        let index = self.current.map_or(0, |i| i.saturating_add(1));
        if index >= self.tracks.len() && !repeat {
            return None;
        }
        if self.tracks.is_empty() {
            return None;
        }
        self.select(index % self.tracks.len())
    }
    pub fn previous_track(&mut self) -> Option<&Track> {
        if self.tracks.is_empty() {
            self.current = None;
            return None;
        }
        self.select(
            self.current
                .unwrap_or(0)
                .saturating_sub(1)
                .min(self.tracks.len() - 1),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shuffle_uses_more_than_seed_parity_and_reaches_every_three_song_order() {
        let mut orders = HashSet::new();
        let mut swapped_even = 0;
        let mut swapped_odd = 0;
        for seed in 0..1024 {
            let mut pair = Queue {
                tracks: vec![track("a"), track("b")],
                current: None,
            };
            pair.shuffle_upcoming(seed);
            if pair.tracks[0].title == "b" {
                if seed % 2 == 0 {
                    swapped_even += 1;
                } else {
                    swapped_odd += 1;
                }
            }
            let mut triple = Queue {
                tracks: vec![track("a"), track("b"), track("c")],
                current: None,
            };
            triple.shuffle_upcoming(seed);
            orders.insert(
                triple
                    .tracks
                    .iter()
                    .map(|track| track.title.clone())
                    .collect::<Vec<_>>(),
            );
        }
        assert!((200..312).contains(&swapped_even));
        assert!((200..312).contains(&swapped_odd));
        assert_eq!(orders.len(), 6);
    }
    #[test]
    fn corrupt_selection_does_not_overflow_and_previous_recovers() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b")],
            current: Some(usize::MAX),
        };
        q.shuffle_upcoming(1);
        assert!(q.next_track(false).is_none());
        assert_eq!(q.previous_track().unwrap().title, "b");
        q.tracks.clear();
        assert!(q.previous_track().is_none());
        assert_eq!(q.current, None);
    }
    #[test]
    fn idle_snapshot_is_paused_at_normal_volume() {
        let snapshot = PlaybackSnapshot::default();
        assert!(snapshot.paused);
        assert_eq!(snapshot.volume, 100.0);
    }
    fn track(title: &str) -> Track {
        Track {
            artists: Vec::new(),
            title: title.into(),
            artist: String::new(),
            album: String::new(),
            source: title.into(),
            duration_seconds: None,
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
    fn drag_reorder_shifts_rows_both_ways_and_preserves_played_and_current() {
        let mut q = Queue {
            tracks: ["played", "current", "a", "b", "c", "d"]
                .map(track)
                .to_vec(),
            current: Some(1),
        };
        assert!(q.move_upcoming_to(2, 5));
        assert_eq!(
            q.tracks
                .iter()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            ["played", "current", "b", "c", "d", "a"]
        );
        assert!(q.move_upcoming_to(5, 2));
        assert_eq!(
            q.tracks
                .iter()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            ["played", "current", "a", "b", "c", "d"]
        );
        assert_eq!(q.current, Some(1));
    }

    #[test]
    fn drag_reorder_rejects_invalid_noop_prefix_and_cross_section_drops() {
        let mut auto_a = track("auto-a");
        auto_a.recommended = true;
        let mut auto_b = track("auto-b");
        auto_b.recommended = true;
        let mut q = Queue {
            tracks: vec![
                track("played"),
                track("current"),
                track("manual"),
                auto_a,
                auto_b,
            ],
            current: Some(1),
        };
        let before = q.tracks.clone();
        for (source, target) in [
            (0, 2),
            (1, 2),
            (2, 1),
            (2, 2),
            (2, 3),
            (4, 2),
            (5, 2),
            (2, 5),
            (usize::MAX, 2),
        ] {
            assert!(!q.move_upcoming_to(source, target));
            assert_eq!(q.tracks, before);
            assert_eq!(q.current, Some(1));
        }
        assert!(q.move_upcoming_to(4, 3));
        assert_eq!(q.tracks[3].title, "auto-b");
        assert_eq!(q.tracks[4].title, "auto-a");
        assert!(q.tracks[3..].iter().all(|t| t.recommended));
    }

    #[test]
    fn drag_reorder_supports_unselected_queues_and_rejects_noncontiguous_groups() {
        let mut q = Queue {
            tracks: vec![track("a"), track("b"), track("c")],
            current: None,
        };
        assert!(q.move_upcoming_to(2, 0));
        assert_eq!(q.tracks[0].title, "c");
        assert_eq!(q.current, None);
        q.tracks[1].recommended = true;
        let before = q.tracks.clone();
        assert!(!q.move_upcoming_to(0, 2));
        assert_eq!(q.tracks, before);
        assert!(!Queue::default().move_upcoming_to(0, 0));
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
    #[test]
    fn long_radio_sessions_free_history_without_losing_current_or_manual_upcoming() {
        let mut queue = Queue {
            tracks: (0..MAX_QUEUE_TRACKS)
                .map(|i| track(&format!("song-{i}")))
                .collect(),
            current: Some(MAX_QUEUE_TRACKS - 2),
        };
        let current = queue.tracks[MAX_QUEUE_TRACKS - 2].clone();
        let upcoming = queue.tracks[MAX_QUEUE_TRACKS - 1].clone();
        queue.trim_played_history(20);
        assert_eq!(queue.current, Some(20));
        assert_eq!(queue.tracks[20], current);
        assert_eq!(queue.tracks[21], upcoming);
        assert_eq!(
            queue.append_deduplicated([track("new"), current, upcoming]),
            1
        );
        assert_eq!(queue.tracks.len(), 23);
        // A distant heard song can return; immediate repeats remain deduplicated.
        assert_eq!(queue.append_deduplicated([track("song-0")]), 1);
    }
}
