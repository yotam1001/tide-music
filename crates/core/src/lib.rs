use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source: String,
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
    /// Randomize upcoming tracks while keeping the current and played prefix.
    pub fn shuffle_upcoming(&mut self, mut seed: u64) {
        let start = self.current.map_or(0, |i| i + 1);
        if start >= self.tracks.len() {
            return;
        }
        for i in (start + 1..self.tracks.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = start + (seed as usize % (i - start + 1));
            self.tracks.swap(i, j);
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
        }
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
}
