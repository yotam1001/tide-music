//! One bounded read-only Library section, separate from personalized Home.
use super::*;
use tide_provider::{BrowseSection, Card, Request};

#[derive(Default)]
pub(super) struct Browse {
    pub section: Option<BrowseSection>,
    pub cards: Vec<Card>,
}
pub(super) fn reset(s: &mut State, u: &AppWindow) {
    s.library_browse = Browse::default();
    u.set_library_section(0);
    u.set_library_note("".into());
    u.set_library_cards(ModelRc::new(VecModel::default()));
}
fn request(section: BrowseSection) -> Request {
    match section {
        BrowseSection::Albums => Request::LibraryAlbums,
        BrowseSection::Artists => Request::LibraryArtists,
        BrowseSection::History => Request::History,
    }
}
pub(super) fn refresh(s: &mut State, u: &AppWindow, tx: &mpsc::SyncSender<LibraryResult>) {
    let request = s
        .library_browse
        .section
        .map(request)
        .unwrap_or(Request::Library);
    request_library(s, u, tx, request);
}
pub(super) fn choose(
    s: &mut State,
    u: &AppWindow,
    tx: &mpsc::SyncSender<LibraryResult>,
    index: i32,
) {
    if s.library_worker.is_some() || s.pending_saved_connection.is_some() {
        return;
    }
    let section = match index {
        0 => None,
        1 => Some(BrowseSection::Albums),
        2 => Some(BrowseSection::Artists),
        3 => Some(BrowseSection::History),
        _ => return,
    };
    s.library_browse.section = section;
    s.library_browse.cards.clear();
    u.set_library_section(index);
    u.set_library_note("".into());
    u.set_library_artwork_y(0.0);
    refresh_library_models(s, u);
    if section.is_some() || s.library_playlists.is_empty() {
        refresh(s, u, tx);
    }
}
pub(super) fn receive(
    s: &mut State,
    u: &AppWindow,
    section: BrowseSection,
    cards: Vec<Card>,
    truncated: bool,
) {
    if s.library_browse.section != Some(section) {
        return;
    }
    s.library_browse.cards = cards
        .into_iter()
        .take(200)
        .filter(|card| accepts(section, card))
        .collect();
    u.set_library_note(
        if truncated {
            "Showing the first 200 items from YouTube Music"
        } else {
            ""
        }
        .into(),
    );
    u.set_library_artwork_y(0.0);
    refresh_library_models(s, u);
}
fn accepts(section: BrowseSection, card: &Card) -> bool {
    matches!(
        (section, card_navigation::action(card)),
        (BrowseSection::Albums, Some(card_navigation::Action::Album))
            | (
                BrowseSection::Artists,
                Some(card_navigation::Action::Artist)
            )
            | (BrowseSection::History, Some(card_navigation::Action::Song))
    )
}
pub(super) fn song(s: &State, index: usize) -> Option<Track> {
    s.library_browse
        .cards
        .get(index)
        .filter(|card| card_navigation::action(card) == Some(card_navigation::Action::Song))
        .map(recommendation_track)
}
pub(super) fn wire(ui: &AppWindow, state: Rc<RefCell<State>>, tx: mpsc::SyncSender<LibraryResult>) {
    let section_state = state.clone();
    let weak = ui.as_weak();
    let section_tx = tx.clone();
    ui.on_select_library_section(move |index| {
        if let Some(u) = weak.upgrade() {
            choose(&mut section_state.borrow_mut(), &u, &section_tx, index);
        }
    });
    let open_state = state.clone();
    let weak = ui.as_weak();
    ui.on_select_library_card(move |index| {
        if let Some(u) = weak.upgrade() {
            let mut s = open_state.borrow_mut();
            let card = usize::try_from(index)
                .ok()
                .and_then(|index| s.library_browse.cards.get(index))
                .cloned();
            if let Some(card) = card {
                open_provider_card(&mut s, &u, &tx, card);
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_queue_library_card(move |index, next| {
        if let Some(u) = weak.upgrade() {
            let mut s = state.borrow_mut();
            if let Some(track) = usize::try_from(index)
                .ok()
                .and_then(|index| song(&s, index))
            {
                queue_choice(&mut s, &u, track, next);
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sections_reject_mismatched_or_invalid_identities() {
        let mut card = Card {
            id: "K4DyBUG242c".into(),
            title: "A song".into(),
            kind: "song".into(),
            subtitle: String::new(),
            shelf: String::new(),
            artists: Vec::new(),
            artwork_url: None,
            duration_seconds: None,
        };
        assert!(accepts(BrowseSection::History, &card));
        assert!(!accepts(BrowseSection::Albums, &card));
        card.kind = "artist".into();
        assert!(!accepts(BrowseSection::Artists, &card));
        card.id = "UC1234567890123456789012".into();
        assert!(accepts(BrowseSection::Artists, &card));
        assert!(!accepts(BrowseSection::History, &card));
    }
}
