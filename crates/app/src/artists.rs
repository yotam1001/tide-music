//! Bounded artist navigation. API identities are never inferred from display names.
use super::*;
use tide_core::Artist;

pub(super) struct Origin {
    open: bool,
    title: String,
    artist: bool,
    note: String,
    tracks: Vec<Track>,
    scroll: f32,
    search: bool,
    queue: bool,
}

pub(super) fn video_id(source: &str) -> Option<String> {
    if !is_youtube_source(source) {
        return None;
    }
    let url = url::Url::parse(source).ok()?;
    let id = if url.host_str() == Some("youtu.be") {
        url.path_segments()?.next()?.to_string()
    } else {
        url.query_pairs()
            .find(|(key, _)| key == "v")?
            .1
            .into_owned()
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
    .then_some(id)
}

pub(super) fn can_open(track: &Track) -> bool {
    track.artists.iter().any(Artist::is_valid)
        || (!track.artist.trim().is_empty() && video_id(&track.source).is_some())
}

pub(super) fn open(
    s: &mut State,
    u: &AppWindow,
    tx: &mpsc::SyncSender<LibraryResult>,
    track: Track,
) {
    if !can_open(&track) || s.library_worker.is_some() || s.pending_saved_connection.is_some() {
        return;
    }
    if !u.get_account_connected() {
        u.set_account_status("Connect YouTube Music to open artist pages.".into());
        u.set_account_open(true);
        return;
    }
    if s.artist_origin.is_none() {
        s.artist_origin = Some(Origin {
            open: u.get_detail_open(),
            title: u.get_detail_title().to_string(),
            artist: u.get_detail_is_artist(),
            note: u.get_detail_note().to_string(),
            tracks: if u.get_detail_open() {
                std::mem::take(&mut s.detail_tracks)
            } else {
                Vec::new()
            },
            scroll: u.get_detail_artwork_y(),
            search: u.get_search_open(),
            queue: u.get_queue_open(),
        });
    }
    s.detail_tracks.clear();
    u.set_detail_open(true);
    u.set_detail_is_artist(true);
    u.set_detail_title(track.artist.clone().into());
    u.set_detail_note("".into());
    u.set_detail_artwork_y(0.0);
    u.set_search_open(false);
    u.set_queue_open(false);
    u.set_detail_busy(true);
    refresh_content_models(s, u);
    let artists: Vec<_> = track
        .artists
        .into_iter()
        .filter(Artist::is_valid)
        .take(5)
        .collect();
    if artists.is_empty() {
        if let Some(video_id) = video_id(&track.source) {
            request_library(s, u, tx, tide_provider::Request::Artists { video_id });
        } else {
            unavailable(u);
        }
    } else {
        resolved(s, u, tx, artists);
    }
}

pub(super) fn resolved(
    s: &mut State,
    u: &AppWindow,
    tx: &mpsc::SyncSender<LibraryResult>,
    artists: Vec<Artist>,
) {
    if !u.get_detail_open() || !u.get_detail_is_artist() {
        return;
    }
    let mut unique = Vec::new();
    for artist in artists.into_iter().filter(Artist::is_valid).take(5) {
        if !unique.iter().any(|other: &Artist| other.id == artist.id) {
            unique.push(artist);
        }
    }
    if unique.is_empty() {
        unavailable(u);
        return;
    }
    if unique.len() == 1 {
        let artist = unique.remove(0);
        u.set_detail_title(artist.name.into());
        request_library(s, u, tx, tide_provider::Request::Artist { id: artist.id });
    } else {
        u.set_artist_choices(
            Rc::new(VecModel::from(
                unique
                    .into_iter()
                    .map(|artist| ArtistRow {
                        id: artist.id.into(),
                        name: artist.name.into(),
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        u.set_detail_busy(false);
        u.set_artist_picker_open(true);
    }
}

fn unavailable(u: &AppWindow) {
    u.set_detail_busy(false);
    u.set_detail_note("YouTube Music did not return an artist page for this song.".into());
}

pub(super) fn back(s: &mut State, u: &AppWindow) {
    u.set_artist_picker_open(false);
    u.set_detail_busy(false);
    if let Some(origin) = s.artist_origin.take() {
        s.detail_tracks = origin.tracks;
        u.set_detail_title(origin.title.into());
        u.set_detail_is_artist(origin.artist);
        u.set_detail_note(origin.note.into());
        u.set_detail_artwork_y(origin.scroll);
        u.set_detail_open(origin.open);
        u.set_search_open(origin.search);
        u.set_queue_open(origin.queue);
    } else {
        u.set_detail_open(false);
    }
    refresh_content_models(s, u);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_require_a_canonical_artist_or_a_youtube_song_not_an_uploader_guess() {
        for (url, expected) in [
            ("https://www.youtube.com/watch?v=K4DyBUG242c", true),
            ("https://youtu.be/K4DyBUG242c", true),
            ("https://other.invalid/watch?v=K4DyBUG242c", false),
            ("https://youtube.com/watch?v=bad", false),
            ("fixture.wav", false),
        ] {
            assert_eq!(video_id(url).is_some(), expected);
        }
        assert!(Artist {
            id: "UC1234567890123456789012".into(),
            name: "Band".into()
        }
        .is_valid());
        assert!(!Artist {
            id: "https://other.invalid".into(),
            name: "Band".into()
        }
        .is_valid());
    }
}
