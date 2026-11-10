use tide_core::{PlaybackSnapshot, Queue};

pub(super) fn seconds(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 1.0 && value < u64::MAX as f64).then(|| value.round() as u64)
}

pub(super) fn format_duration(value: Option<u64>) -> String {
    match value.filter(|value| *value > 0) {
        Some(seconds) => format!("{}:{:02}", seconds / 60, seconds % 60),
        None => "—".into(),
    }
}

pub(super) fn remember_duration(
    queue: &mut Queue,
    generation: u64,
    active: bool,
    snapshot: &PlaybackSnapshot,
) -> bool {
    if !active || snapshot.generation != generation || snapshot.loading || snapshot.error.is_some()
    {
        return false;
    }
    let Some(duration) = seconds(snapshot.duration) else {
        return false;
    };
    let Some(track) = queue.current.and_then(|index| queue.tracks.get_mut(index)) else {
        return false;
    };
    if track.duration_seconds == Some(duration) {
        return false;
    }
    track.duration_seconds = Some(duration);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_rejects_invalid_numbers_and_formats_minutes() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert_eq!(seconds(value), None);
        }
        assert_eq!(format_duration(Some(245)), "4:05");
        assert_eq!(format_duration(None), "—");
        assert_eq!(format_duration(Some(0)), "—");
    }
    #[test]
    fn late_snapshot_cannot_update_another_song_and_metadata_survives_restart() {
        let mut queue = Queue {
            tracks: vec![tide_core::Track {
                title: "song".into(),
                artist: String::new(),
                album: String::new(),
                source: "https://example.com/song".into(),
                duration_seconds: None,
                artwork_url: None,
                recommended: false,
            }],
            current: Some(0),
        };
        let mut snapshot = PlaybackSnapshot {
            generation: 1,
            duration: 245.0,
            ..Default::default()
        };
        assert!(!remember_duration(&mut queue, 2, true, &snapshot));
        assert!(!remember_duration(&mut queue, 1, false, &snapshot));
        snapshot.loading = true;
        assert!(!remember_duration(&mut queue, 1, true, &snapshot));
        snapshot.loading = false;
        assert!(remember_duration(&mut queue, 1, true, &snapshot));
        assert!(!remember_duration(&mut queue, 1, true, &snapshot));
        let restored: Queue = serde_json::from_slice(&serde_json::to_vec(&queue).unwrap()).unwrap();
        assert_eq!(restored.tracks[0].duration_seconds, Some(245));
        let legacy: tide_core::Track = serde_json::from_str(
            r#"{"title":"song","artist":"","album":"","source":"https://example.com/song"}"#,
        )
        .unwrap();
        assert_eq!(legacy.duration_seconds, None);
    }
}

/// Total length of a track list for a playlist header, e.g. "1 hr 32 min".
/// Empty when no track reports a duration.
pub fn format_total_duration(tracks: &[tide_core::Track]) -> String {
    let seconds: u64 = tracks
        .iter()
        .filter_map(|track| track.duration_seconds)
        .sum();
    if seconds == 0 {
        return String::new();
    }
    let minutes = seconds.div_ceil(60);
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} hr"),
        (hours, minutes) => format!("{hours} hr {minutes} min"),
    }
}

#[cfg(test)]
mod total_duration_tests {
    use super::*;

    #[test]
    fn totals_read_like_a_playlist_header() {
        let track = |seconds| tide_core::Track {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            source: String::new(),
            duration_seconds: seconds,
            artwork_url: None,
            recommended: false,
        };
        assert_eq!(format_total_duration(&[]), "");
        assert_eq!(format_total_duration(&[track(None)]), "");
        assert_eq!(
            format_total_duration(&[track(Some(200)), track(Some(100))]),
            "5 min"
        );
        assert_eq!(
            format_total_duration(&[track(Some(3600)), track(Some(1900))]),
            "1 hr 32 min"
        );
        assert_eq!(format_total_duration(&[track(Some(7200))]), "2 hr");
    }
}
