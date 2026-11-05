//! Only canonical provider identities can open a playable/navigable card.
use tide_provider::Card;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Song,
    Playlist,
    Album,
    Artist,
}

pub(super) fn action(card: &Card) -> Option<Action> {
    let id = &card.id;
    if id.is_empty()
        || id.len() > 160
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    match card.kind.as_str() {
        "song" if id.len() == 11 => Some(Action::Song),
        "playlist" => Some(Action::Playlist),
        "album" if id.starts_with("MPRE") && id.len() >= 8 => Some(Action::Album),
        "artist"
            if (tide_core::Artist {
                id: id.clone(),
                name: card.title.clone(),
            })
            .is_valid() =>
        {
            Some(Action::Artist)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_requires_supported_kind_and_canonical_identity() {
        for (kind, id, expected) in [
            ("song", "K4DyBUG242c", Some(Action::Song)),
            ("playlist", "PLfixture", Some(Action::Playlist)),
            ("album", "MPREb_fixture", Some(Action::Album)),
            ("artist", "UC1234567890123456789012", Some(Action::Artist)),
            ("artist", "UCbad", None),
            ("album", "MPRE", None),
            ("album", "https://evil.invalid", None),
            ("song", "bad", None),
            ("unknown", "PLfixture", None),
            ("playlist", "bad\npath", None),
        ] {
            let card = Card {
                id: id.into(),
                kind: kind.into(),
                title: "A title".into(),
                subtitle: String::new(),
                shelf: String::new(),
                artists: Vec::new(),
                featured_artwork_url: None,
                artwork_url: None,
                duration_seconds: None,
            };
            assert_eq!(action(&card), expected, "{kind}: {id}");
        }
    }
}
