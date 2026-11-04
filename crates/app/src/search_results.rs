//! Typed, bounded search; results never replace the queue until explicitly selected.
use super::*;
use tide_provider::{Card, Request, Response, SearchKind};
pub(super) enum Results {
    Songs(Vec<Track>),
    Collections(Vec<Card>),
}
pub(super) type Sender = mpsc::SyncSender<(u64, Result<Results, String>)>;

fn kind(index: i32) -> Option<SearchKind> {
    match index {
        0 => Some(SearchKind::Songs),
        1 => Some(SearchKind::Albums),
        2 => Some(SearchKind::Artists),
        3 => Some(SearchKind::Playlists),
        _ => None,
    }
}
fn accepts(kind: SearchKind, card: &Card) -> bool {
    matches!(
        (kind, card_navigation::action(card)),
        (SearchKind::Songs, Some(card_navigation::Action::Song))
            | (SearchKind::Albums, Some(card_navigation::Action::Album))
            | (SearchKind::Artists, Some(card_navigation::Action::Artist))
            | (
                SearchKind::Playlists,
                Some(card_navigation::Action::Playlist)
            )
    )
}
fn fetch(
    query: String,
    kind: SearchKind,
    provider: Option<tide_provider::Provider>,
    cancel: Arc<AtomicBool>,
) -> Result<Results, String> {
    let Some(provider) = provider else {
        return if kind == SearchKind::Songs {
            search(query, cancel).map(Results::Songs)
        } else {
            Err(
                "Connect your YouTube Music account to search albums, artists and playlists."
                    .into(),
            )
        };
    };
    match provider.execute(Request::Search { query, kind }, cancel) {
        Ok(Response::Search {
            kind: returned,
            cards,
        }) if returned == kind => {
            let cards: Vec<_> = cards
                .into_iter()
                .take(40)
                .filter(|card| accepts(kind, card))
                .collect();
            Ok(if kind == SearchKind::Songs {
                Results::Songs(cards.iter().map(recommendation_track).collect())
            } else {
                Results::Collections(cards)
            })
        }
        _ => Err(
            "YouTube Music search could not finish. Retry, or check your connection in Account."
                .into(),
        ),
    }
}
pub(super) fn start(s: &mut State, ui: &AppWindow, tx: &Sender, query: String, index: i32) {
    let Some(kind) = kind(index) else {
        return;
    };
    if s.search_worker.is_some() {
        return;
    }
    if query.trim().is_empty() || query.len() > 512 || query.chars().any(char::is_control) {
        ui.set_search_open(true);
        ui.set_detail_open(false);
        ui.set_search_note("Enter a shorter song, album, artist or playlist name.".into());
        return;
    }
    s.search_generation += 1;
    let generation = s.search_generation;
    s.search_query = query.clone();
    s.search_tracks.clear();
    s.search_cards.clear();
    let cancel = Arc::new(AtomicBool::new(false));
    s.search_cancel = cancel.clone();
    let provider = s
        .provider
        .clone()
        .filter(|_| s.account_verified && s.pending_saved_connection.is_none());
    ui.set_search_kind(index);
    ui.set_search_artwork_y(0.0);
    ui.set_search_open(true);
    ui.set_detail_open(false);
    ui.set_search_busy(true);
    ui.set_search_note("".into());
    refresh_content_models(s, ui);
    let tx = tx.clone();
    s.search_worker = Some(std::thread::spawn(move || {
        let _ = tx.try_send((generation, fetch(query, kind, provider, cancel)));
    }));
}
pub(super) fn invalidate(s: &mut State, ui: &AppWindow) {
    s.search_cancel.store(true, Ordering::Release);
    s.search_generation += 1;
    s.search_tracks.clear();
    s.search_cards.clear();
    s.search_query.clear();
    ui.set_search_busy(false);
    ui.set_search_note("".into());
    ui.set_search_rows(ModelRc::new(VecModel::default()));
    ui.set_search_cards(ModelRc::new(VecModel::default()));
}
pub(super) fn refresh(s: &State, ui: &AppWindow) {
    let mut cache = s.artwork.borrow_mut();
    let visible = artwork_view::visible_range(
        ui.get_search_artwork_y(),
        ui.get_search_artwork_height(),
        s.search_cards
            .iter()
            .map(|_| ui.global::<LayoutMetrics>().get_browse_row_height()),
    );
    let rows = s
        .search_cards
        .iter()
        .enumerate()
        .map(|(index, card)| RecommendationRow {
            id: card.id.clone().into(),
            title: card.title.clone().into(),
            subtitle: card.subtitle.clone().into(),
            kind: card.kind.clone().into(),
            shelf: "".into(),
            cover_variant: artwork::fallback_variant(&card.id),
            artwork: card
                .artwork_url
                .as_deref()
                .filter(|_| {
                    ui.get_search_open() && !ui.get_detail_open() && visible.contains(&index)
                })
                .map(|url| cache.get_or_request(url))
                .unwrap_or_default(),
        })
        .collect();
    ui.set_search_cards(update_model(ui.get_search_cards(), rows));
}
pub(super) fn wire(
    ui: &AppWindow,
    state: Rc<RefCell<State>>,
    tx: Sender,
    library_tx: mpsc::SyncSender<LibraryResult>,
) {
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_select_search_kind(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut s = s.borrow_mut();
            let query = s.search_query.clone();
            start(&mut s, &ui, &tx, query, index);
        }
    });
    let weak = ui.as_weak();
    ui.on_select_search_card(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut s = state.borrow_mut();
            if let Some(card) = usize::try_from(index)
                .ok()
                .and_then(|index| s.search_cards.get(index))
                .cloned()
            {
                open_provider_card(&mut s, &ui, &library_tx, card);
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_filters_do_not_accept_wrong_or_unplayable_identities() {
        let mut card = Card {
            id: "UC1234567890123456789012".into(),
            title: "Band".into(),
            kind: "artist".into(),
            subtitle: String::new(),
            shelf: String::new(),
            artists: Vec::new(),
            artwork_url: None,
            duration_seconds: None,
        };
        assert!(accepts(SearchKind::Artists, &card));
        assert!(!accepts(SearchKind::Songs, &card));
        card.id = "https://example.test".into();
        assert!(!accepts(SearchKind::Artists, &card));
        assert!(kind(-1).is_none());
        assert!(kind(4).is_none());
    }
}
