//! Typed, bounded search; results never replace the queue until explicitly selected.
use super::*;
use tide_provider::{Card, Request, Response, SearchKind};
pub(super) enum Results {
    Songs(Vec<Track>),
    Collections(Vec<Card>),
}
pub(super) enum Failure {
    Session,
    Message(String),
}
pub(super) type Sender = mpsc::SyncSender<(u64, Result<Results, Failure>)>;

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
) -> Result<Results, Failure> {
    let Some(provider) = provider else {
        return if kind == SearchKind::Songs {
            search(query, cancel)
                .map(Results::Songs)
                .map_err(Failure::Message)
        } else {
            Err(Failure::Message(
                "Connect your YouTube Music account to search albums, artists and playlists."
                    .into(),
            ))
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
        Ok(Response::Error { code }) if code == "session" => Err(Failure::Session),
        _ => Err(Failure::Message(
            "YouTube Music search could not finish. Retry in a moment.".into(),
        )),
    }
}
pub(super) fn start(s: &mut State, ui: &AppWindow, tx: &Sender, query: String, index: i32) {
    let Some(kind) = kind(index) else {
        return;
    };
    if s.search_worker.is_some() {
        return;
    }
    ui.set_forward_collection(false);
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
struct SuggestionWork {
    query: String,
    connection: tide_provider::SavedConnection,
    receiver: mpsc::Receiver<Result<Response, String>>,
}

fn wire_suggestions(ui: &AppWindow, state: Rc<RefCell<State>>) {
    let timer = Rc::new(slint::Timer::default());
    let work = Rc::new(RefCell::new(None::<SuggestionWork>));
    let weak = ui.as_weak();
    ui.on_suggest_search(move |input| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        ui.set_search_suggestions(ModelRc::default());
        let query = input.trim().to_owned();
        let eligible = (2..=120).contains(&query.chars().count())
            && !query.contains(['/', '\\', ':'])
            && !query.chars().any(char::is_control)
            && !Path::new(&query).is_file();
        let weak = weak.clone();
        let state = state.clone();
        let owner = Rc::downgrade(&timer);
        let work = work.clone();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(350),
            move || {
                let (Some(ui), Some(timer)) = (weak.upgrade(), owner.upgrade()) else {
                    return;
                };
                let state = state.borrow();
                let mut work = work.borrow_mut();
                let show = eligible
                    && ui.get_suggestions_open()
                    && state.account_verified
                    && ui.get_source_input().trim() == query
                    && !ui.get_account_open()
                    && !ui.get_settings_open()
                    && !ui.get_collapsed()
                    && !ui.get_lyrics_open()
                    && !ui.get_jam_open();
                if let Some(active) = work.as_ref() {
                    let result = match active.receiver.try_recv() {
                        Ok(result) => result.ok(),
                        Err(mpsc::TryRecvError::Empty) => return,
                        Err(mpsc::TryRecvError::Disconnected) => None,
                    };
                    let matches = show
                        && active.query == query
                        && state.saved_connection.as_ref() == Some(&active.connection);
                    *work = None;
                    if matches {
                        if let Some(Response::Suggestions { items }) = result {
                            ui.set_search_suggestions(ModelRc::new(VecModel::from(
                                items
                                    .into_iter()
                                    .take(5)
                                    .filter(|value| {
                                        !value.is_empty()
                                            && value.len() <= 480
                                            && !value.contains(['/', '\\', ':'])
                                            && !value.chars().any(char::is_control)
                                    })
                                    .map(slint::SharedString::from)
                                    .collect::<Vec<_>>(),
                            )));
                        }
                        timer.stop();
                        return;
                    }
                }
                if !show {
                    timer.stop();
                    return;
                }
                let (Some(provider), Some(connection)) =
                    (state.provider.clone(), state.saved_connection.clone())
                else {
                    timer.stop();
                    return;
                };
                let (sender, receiver) = mpsc::sync_channel(1);
                let request = Request::Suggestions {
                    query: query.clone(),
                };
                if std::thread::Builder::new()
                    .name("tide-suggestions".into())
                    .spawn(move || {
                        let _ = sender
                            .send(provider.execute(request, Arc::new(AtomicBool::new(false))));
                    })
                    .is_ok()
                {
                    *work = Some(SuggestionWork {
                        query: query.clone(),
                        connection,
                        receiver,
                    });
                } else {
                    timer.stop();
                }
            },
        );
    });
}

pub(super) fn wire(
    ui: &AppWindow,
    state: Rc<RefCell<State>>,
    tx: Sender,
    library_tx: mpsc::SyncSender<LibraryResult>,
) {
    wire_suggestions(ui, state.clone());
    let weak = ui.as_weak();
    let search_state = state.clone();
    let search_tx = tx.clone();
    ui.on_choose_suggestion(move |suggestion| {
        if let Some(ui) = weak.upgrade() {
            start(
                &mut search_state.borrow_mut(),
                &ui,
                &search_tx,
                suggestion.to_string(),
                ui.get_search_kind(),
            );
        }
    });
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
            featured_artwork_url: None,
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
