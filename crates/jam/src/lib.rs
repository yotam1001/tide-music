//! Nearby rooms shared by Tide and Grove. Only track metadata and timing travel.
pub mod jam;
pub mod matching;
pub mod sync;
pub mod transport;

/// Canonical provider identifiers only; arbitrary URLs and files never enter a room.
pub fn track_uri(input: &str) -> Result<String, String> {
    let input = input.trim();
    if let Some(id) = input.strip_prefix("spotify:track:") {
        if id.len() == 22 && id.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Ok(input.into());
        }
    }
    if let Some(id) = input.strip_prefix("youtube:") {
        if youtube_id(id) {
            return Ok(input.into());
        }
    }
    if let Ok(url) = url::Url::parse(input) {
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err("Use a Spotify song or YouTube video link.".into());
        }
        match url.host_str().unwrap_or("") {
            "open.spotify.com" => {
                let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
                if let ["track", id] = parts.as_slice() {
                    return track_uri(&format!("spotify:track:{id}"));
                }
            }
            "youtube.com" | "www.youtube.com" | "music.youtube.com" if url.path() == "/watch" => {
                if let Some((_, id)) = url.query_pairs().find(|(k, _)| k == "v") {
                    if youtube_id(&id) {
                        return Ok(format!("youtube:{id}"));
                    }
                }
            }
            "youtu.be" => {
                let id = url.path().trim_start_matches('/');
                if youtube_id(id) {
                    return Ok(format!("youtube:{id}"));
                }
            }
            _ => {}
        }
    }
    Err("Use a Spotify song or YouTube video link.".into())
}
fn youtube_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_are_provider_scoped_and_never_arbitrary_sources() {
        assert_eq!(
            track_uri("https://music.youtube.com/watch?v=abcdefghijk&list=x").unwrap(),
            "youtube:abcdefghijk"
        );
        assert_eq!(
            track_uri("https://open.spotify.com/track/aaaaaaaaaaaaaaaaaaaaaa?si=x").unwrap(),
            "spotify:track:aaaaaaaaaaaaaaaaaaaaaa"
        );
        for bad in [
            "file:///C:/song.mp3",
            "https://evil.test/watch?v=abcdefghijk",
            "https://user@youtube.com/watch?v=abcdefghijk",
            "youtube:../whatever",
            "spotify:album:aaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(track_uri(bad).is_err());
        }
    }
}
