//! View-only search retains source indices for every action, including duplicate songs.
use tide_core::Track;
pub fn matching(tracks: &[Track], start: usize, query: &str) -> Vec<usize> {
    let query: String = query.chars().take(200).collect::<String>().to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    tracks
        .iter()
        .enumerate()
        .skip(start)
        .filter_map(|(i, t)| {
            if words.is_empty() {
                return Some(i);
            }
            let text = format!("{} {} {}", t.title, t.artist, t.album).to_lowercase();
            words.iter().all(|word| text.contains(word)).then_some(i)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_all_words_across_fields_without_reordering_or_collapsing_duplicates() {
        let track = |title: &str, artist: &str| Track {
            title: title.into(),
            artist: artist.into(),
            artists: vec![],
            album: "Live".into(),
            source: "local.wav".into(),
            duration_seconds: None,
            artwork_url: None,
            recommended: false,
        };
        let tracks = vec![
            track("Other", "Artist"),
            track("Écho", "Band"),
            track("Écho", "Band"),
        ];
        assert_eq!(matching(&tracks, 0, "BAND live écho"), vec![1, 2]);
        assert_eq!(matching(&tracks, 2, "écho"), vec![2]);
        assert_eq!(matching(&tracks, 1, "  "), vec![1, 2]);
        assert!(matching(&tracks, 0, "absent").is_empty());
        assert_eq!(tracks[1].title, "Écho");
    }
}
