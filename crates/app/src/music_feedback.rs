//! Collection operations are independent of browsing and playback workers.
use super::*;
use tide_provider::{FeedbackOperation, FeedbackRequest, PlaylistDetails, Rating, SavedConnection};

struct Work {
    handle: std::thread::JoinHandle<()>,
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<LibraryResult>,
    request: FeedbackRequest,
    expected: SavedConnection,
    detail_revision: u64,
    label: String,
}
struct Picker {
    video: String,
    expected: SavedConnection,
    choices: Vec<tide_provider::Playlist>,
    refreshing: bool,
}
pub struct Music {
    worker: Option<Work>,
    identity: Option<SavedConnection>,
    listening: history_feedback::Listening,
    synthetic: bool,
    current_video: Option<String>,
    inspected: bool,
    notice_until: Option<Instant>,
    playlist: Option<PlaylistDetails>,
    detail_revision: u64,
    picker: Option<Picker>,
}
impl Music {
    pub fn new(synthetic: bool) -> Self {
        Self {
            worker: None,
            identity: None,
            listening: Default::default(),
            synthetic,
            current_video: None,
            inspected: false,
            notice_until: None,
            playlist: None,
            detail_revision: 0,
            picker: None,
        }
    }
    pub fn shutdown(&mut self) {
        if let Some(work) = self.worker.take() {
            work.cancel.store(true, Ordering::Release);
            let _ = work.handle.join();
        }
    }
}
impl Drop for Music {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn begin(s: &mut State, request: FeedbackRequest) -> bool {
    if s.music_feedback.synthetic
        || s.music_feedback.worker.is_some()
        || s.pending_saved_connection.is_some()
        || !s.account_verified
        || !request.valid()
    {
        return false;
    }
    let (Some(provider), Some(expected)) = (s.provider.clone(), s.saved_connection.clone()) else {
        return false;
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, receiver) = mpsc::sync_channel(1);
    let input = tide_provider::Request::Feedback {
        request: request.clone(),
        expected: expected.clone(),
    };
    let cancelled = cancel.clone();
    let Ok(handle) = std::thread::Builder::new()
        .name("tide-feedback".into())
        .spawn(move || {
            let _ = tx.try_send(provider.execute(input, cancelled));
        })
    else {
        return false;
    };
    s.music_feedback.worker = Some(Work {
        handle,
        cancel,
        receiver,
        request,
        expected,
        detail_revision: s.music_feedback.detail_revision,
        label: String::new(),
    });
    true
}

pub fn install(
    state: &Rc<RefCell<State>>,
    ui: &AppWindow,
    library_tx: &mpsc::SyncSender<LibraryResult>,
) {
    ui.set_history_feedback_enabled(state.borrow().settings.history_feedback);
    ui.set_history_feedback_status(
        "Only real playback is counted. Automated tests never update history.".into(),
    );
    let history_state = state.clone();
    let weak = ui.as_weak();
    ui.on_toggle_history_feedback(move || {
        let Some(u) = weak.upgrade() else { return; };
        let mut s = history_state.borrow_mut();
        s.settings.history_feedback = !s.settings.history_feedback;
        s.music_feedback.listening.reset();
        if !s.settings.history_feedback {
            if let Some(work) = &s.music_feedback.worker {
                if matches!(work.request, FeedbackRequest::History { .. }) {
                    work.cancel.store(true, Ordering::Release);
                }
            }
        }
        u.set_history_feedback_enabled(s.settings.history_feedback);
        u.set_history_feedback_status(if s.settings.history_feedback {
            "New listening will be sent to your selected YouTube Music account after enough actual playback."
        } else { "History feedback is off. New plays will not be submitted." }.into());
        save(&s.settings);
    });
    let rating_state = state.clone();
    let weak = ui.as_weak();
    ui.on_rate_song(move |value| {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let rating = match value {
            1 => Rating::Like,
            -1 => Rating::Dislike,
            0 => Rating::Indifferent,
            _ => return,
        };
        let mut s = rating_state.borrow_mut();
        let Some(video_id) = current_video_id(&s) else {
            return;
        };
        if begin(&mut s, FeedbackRequest::Rate { video_id, rating }) {
            u.set_rating_busy(true);
            u.set_collection_busy(true);
            notice(&mut s, &u, "Saving your rating…");
        } else {
            notice(
                &mut s,
                &u,
                "Wait for the current account action, then try again.",
            );
        }
    });
    let dismiss_state = state.clone();
    let weak = ui.as_weak();
    ui.on_dismiss_feedback(move || {
        if let Some(u) = weak.upgrade() {
            dismiss_state.borrow_mut().music_feedback.notice_until = None;
            u.set_feedback_notice("".into());
        }
    });
    let picker_state = state.clone();
    let weak = ui.as_weak();
    ui.on_add_to_playlist(move |origin, index| {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let mut s = picker_state.borrow_mut();
        let track = usize::try_from(index).ok().and_then(|index| match origin {
            0 => s.settings.queue.tracks.get(index).cloned(),
            1 => s.detail_tracks.get(index).cloned(),
            2 => s.search_tracks.get(index).cloned(),
            3 => s
                .recommendations
                .get(index)
                .filter(|c| c.kind == "song")
                .map(recommendation_track),
            _ => None,
        });
        let Some(track) = track else {
            return;
        };
        let Some(video) = artists::video_id(&track.source) else {
            notice(
                &mut s,
                &u,
                "Only YouTube Music songs can be saved to playlists.",
            );
            return;
        };
        let Some(expected) = s
            .saved_connection
            .clone()
            .filter(|_| s.account_verified && !s.music_feedback.synthetic)
        else {
            return;
        };
        if s.music_feedback.worker.is_some() {
            notice(
                &mut s,
                &u,
                "Wait for the current account action, then choose the playlist.",
            );
            return;
        }
        let choices = s
            .library_playlists
            .iter()
            .filter(|p| p.owned)
            .take(200)
            .cloned()
            .collect();
        s.music_feedback.picker = Some(Picker {
            video,
            expected,
            choices,
            refreshing: false,
        });
        u.set_playlist_picker_artwork_y(0.0);
        update_choices(&s, &u);
        u.set_playlist_picker_song(track.title.into());
        u.set_playlist_choices_busy(false);
        u.set_playlist_picker_open(true);
    });
    let cancel_state = state.clone();
    let weak = ui.as_weak();
    ui.on_cancel_playlist_picker(move || {
        if let Some(u) = weak.upgrade() {
            cancel_state.borrow_mut().music_feedback.picker = None;
            u.set_playlist_picker_open(false);
            u.set_playlist_choices_busy(false);
        }
    });
    let choose_state = state.clone();
    let weak = ui.as_weak();
    ui.on_pick_playlist(move |index| {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let mut s = choose_state.borrow_mut();
        let Some(picker) = s.music_feedback.picker.as_ref() else {
            return;
        };
        if picker.refreshing || s.saved_connection.as_ref() != Some(&picker.expected) {
            return;
        }
        let Some(target) = usize::try_from(index)
            .ok()
            .and_then(|i| picker.choices.get(i))
            .cloned()
        else {
            return;
        };
        let request = FeedbackRequest::PlaylistAdd {
            video_id: picker.video.clone(),
            playlist_id: target.id,
        };
        if begin(&mut s, request) {
            s.music_feedback.worker.as_mut().unwrap().label = target.title;
            s.music_feedback.picker = None;
            u.set_playlist_picker_open(false);
            u.set_collection_busy(true);
            u.set_rating_busy(true);
            notice(&mut s, &u, "Saving song to the playlist…");
        }
    });
    let refresh_state = state.clone();
    let weak = ui.as_weak();
    let tx = library_tx.clone();
    ui.on_refresh_playlist_choices(move || {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let mut s = refresh_state.borrow_mut();
        if s.library_worker.is_some() {
            notice(
                &mut s,
                &u,
                "Wait for the current music refresh, then refresh playlists.",
            );
            return;
        }
        if s.music_feedback.picker.is_none() {
            return;
        }
        request_library(&mut s, &u, &tx, tide_provider::Request::Library);
        let refreshing = s.library_worker.is_some();
        if let Some(picker) = &mut s.music_feedback.picker {
            picker.refreshing = refreshing;
        }
        u.set_playlist_choices_busy(refreshing);
    });
    let remove_state = state.clone();
    let weak = ui.as_weak();
    ui.on_remove_playlist_song(move |index| {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let mut s = remove_state.borrow_mut();
        let Some(index) = usize::try_from(index).ok() else {
            return;
        };
        let Some(context) = s
            .music_feedback
            .playlist
            .as_ref()
            .filter(|p| p.editable && p.valid_for(&p.id, s.detail_tracks.len()))
        else {
            return;
        };
        let Some(entry_id) = context.entries.get(index).and_then(|e| e.clone()) else {
            return;
        };
        let Some(video_id) = s
            .detail_tracks
            .get(index)
            .and_then(|t| artists::video_id(&t.source))
        else {
            return;
        };
        let request = FeedbackRequest::PlaylistRemove {
            video_id,
            playlist_id: context.id.clone(),
            entry_id,
        };
        if begin(&mut s, request) {
            u.set_collection_busy(true);
            u.set_rating_busy(true);
            notice(&mut s, &u, "Removing this entry from the playlist…");
        }
    });
    let refresh_state = state.clone();
    let weak = ui.as_weak();
    let tx = library_tx.clone();
    ui.on_refresh_detail_playlist(move || {
        let Some(u) = weak.upgrade() else {
            return;
        };
        let mut s = refresh_state.borrow_mut();
        let Some(id) = s.music_feedback.playlist.as_ref().map(|p| p.id.clone()) else {
            return;
        };
        request_library(&mut s, &u, &tx, tide_provider::Request::Playlist { id });
    });
}

pub fn update_choices(s: &State, u: &AppWindow) {
    let mut artwork = s.artwork.borrow_mut();
    let choices = s
        .music_feedback
        .picker
        .as_ref()
        .map(|p| p.choices.as_slice())
        .unwrap_or_default();
    let visible = artwork_view::visible_range(
        u.get_playlist_picker_artwork_y(),
        u.get_playlist_picker_artwork_height(),
        choices
            .iter()
            .map(|_| u.global::<LayoutMetrics>().get_browse_row_height()),
    );
    let rows: Vec<PlaylistRow> = choices
        .iter()
        .enumerate()
        .map(|(index, p)| PlaylistRow {
            id: p.id.clone().into(),
            cover_variant: artwork::fallback_variant(&p.id),
            title: p.title.clone().into(),
            subtitle: p.subtitle.clone().into(),
            artwork: p
                .artwork_url
                .as_deref()
                .filter(|_| visible.contains(&index))
                .map(|url| artwork.get_or_request(url))
                .unwrap_or_default(),
        })
        .collect();
    u.set_playlist_choices(update_model(u.get_playlist_choices(), rows));
    u.set_playlist_choices_busy(
        s.music_feedback
            .picker
            .as_ref()
            .is_some_and(|p| p.refreshing),
    );
}

fn notice(s: &mut State, u: &AppWindow, text: &str) {
    u.set_feedback_notice(text.into());
    s.music_feedback.notice_until = Some(Instant::now() + Duration::from_secs(5));
}

pub fn set_playlist(
    s: &mut State,
    u: &AppWindow,
    details: Option<PlaylistDetails>,
    id: Option<&str>,
) {
    s.music_feedback.detail_revision = s.music_feedback.detail_revision.wrapping_add(1);
    s.music_feedback.playlist =
        details.filter(|d| id.is_some_and(|id| d.valid_for(id, s.detail_tracks.len())));
    let editable = s
        .music_feedback
        .playlist
        .as_ref()
        .is_some_and(|p| p.editable);
    u.set_detail_editable(editable);
    u.set_detail_playlist_known(s.music_feedback.playlist.is_some());
    let entries: Vec<bool> = s
        .music_feedback
        .playlist
        .as_ref()
        .map(|p| p.entries.iter().map(|e| editable && e.is_some()).collect())
        .unwrap_or_default();
    u.set_detail_editable_entries(ModelRc::new(VecModel::from(entries)));
}

pub fn take_playlist(s: &mut State, u: &AppWindow) -> Option<PlaylistDetails> {
    let context = s.music_feedback.playlist.take();
    set_playlist(s, u, None, None);
    context
}

pub fn restore_playlist(s: &mut State, u: &AppWindow, context: Option<PlaylistDetails>) {
    let id = context.as_ref().map(|p| p.id.clone());
    set_playlist(s, u, context, id.as_deref());
}

pub fn tick(s: &mut State, u: &AppWindow) {
    if s.music_feedback.identity != s.saved_connection {
        if let Some(work) = &s.music_feedback.worker {
            work.cancel.store(true, Ordering::Release);
        }
        s.music_feedback.listening.reset();
        s.music_feedback.identity = s.saved_connection.clone();
        s.music_feedback.current_video = None;
        s.music_feedback.inspected = false;
        s.music_feedback.picker = None;
        u.set_playlist_picker_open(false);
        set_playlist(s, u, None, None);
        u.set_current_rating(0);
        u.set_history_feedback_status(
            if s.settings.history_feedback {
                "Waiting for enough actual playback in your selected YouTube Music account."
            } else {
                "History feedback is off. New plays will not be submitted."
            }
            .into(),
        );
    }
    if s.music_feedback
        .worker
        .as_ref()
        .is_some_and(|w| w.handle.is_finished())
    {
        let work = s.music_feedback.worker.take().unwrap();
        let _ = work.handle.join();
        if s.saved_connection.as_ref() == Some(&work.expected) {
            let reply = work.receiver.try_recv();
            let error_code = match &reply {
                Ok(Ok(tide_provider::Response::Error { code })) => Some(code.as_str()),
                _ => None,
            };
            let result = match &reply {
                Ok(Ok(tide_provider::Response::Feedback { result }))
                    if work.request.accepts(result) =>
                {
                    Some(result)
                }
                _ => None,
            };
            let confirmed = result.is_some();
            if let Some(result) = result.filter(|r| {
                matches!(
                    r.operation,
                    FeedbackOperation::Rating | FeedbackOperation::Rate
                )
            }) {
                if current_video_id(s).as_deref() == Some(result.video_id.as_str()) {
                    u.set_current_rating(match result.rating {
                        Some(Rating::Like) => 1,
                        Some(Rating::Dislike) => -1,
                        _ => 0,
                    });
                }
            }
            if let FeedbackRequest::Rate { rating, .. } = work.request {
                notice(
                    s,
                    u,
                    if !confirmed {
                        "Your rating was not confirmed. Playback continues."
                    } else {
                        match rating {
                            Rating::Like => "Liked on YouTube Music",
                            Rating::Dislike => "Disliked on YouTube Music",
                            Rating::Indifferent => "Rating removed from YouTube Music",
                        }
                    },
                );
            }
            if let FeedbackRequest::PlaylistAdd { .. } = &work.request {
                notice(
                    s,
                    u,
                    &if confirmed {
                        format!(
                            "Saved to {}",
                            work.label.chars().take(60).collect::<String>()
                        )
                    } else {
                        playlist_error(error_code).into()
                    },
                );
            }
            if let FeedbackRequest::PlaylistRemove {
                playlist_id,
                entry_id,
                ..
            } = &work.request
            {
                if confirmed {
                    if s.music_feedback.detail_revision == work.detail_revision {
                        if let Some(context) = s
                            .music_feedback
                            .playlist
                            .as_mut()
                            .filter(|p| &p.id == playlist_id)
                        {
                            if let Some(index) = context
                                .entries
                                .iter()
                                .position(|e| e.as_deref() == Some(entry_id.as_str()))
                            {
                                if index < s.detail_tracks.len()
                                    && context.entries.len() == s.detail_tracks.len()
                                {
                                    context.entries.remove(index);
                                    s.detail_tracks.remove(index);
                                    let context = s.music_feedback.playlist.take();
                                    restore_playlist(s, u, context);
                                    refresh_content_models(s, u);
                                }
                            }
                        }
                    }
                    notice(
                        s,
                        u,
                        "Removed this entry from the playlist. Your queue is unchanged.",
                    );
                } else {
                    notice(s, u, playlist_error(error_code));
                }
            }
            if s.settings.history_feedback
                && matches!(work.request, FeedbackRequest::History { .. })
            {
                u.set_history_feedback_status(if confirmed {
                    "Recent listening was sent to YouTube Music. Recommendations can adapt over time."
                } else {
                    "The last history update was not confirmed. Music continues; that update will not be retried."
                }.into());
            }
        }
    }
    if u.get_collapsed() {
        u.set_playlist_picker_open(false);
    }
    if !u.get_playlist_picker_open() {
        if s.music_feedback.picker.take().is_some() || u.get_playlist_choices().row_count() > 0 {
            update_choices(s, u);
        }
        u.set_playlist_choices_busy(false);
    }
    if s.library_worker.is_none() {
        if let Some(picker) = s.music_feedback.picker.as_mut().filter(|p| p.refreshing) {
            picker.choices = s
                .library_playlists
                .iter()
                .filter(|p| p.owned)
                .take(200)
                .cloned()
                .collect();
            picker.refreshing = false;
            update_choices(s, u);
            u.set_playlist_choices_busy(false);
        }
    }
    if s.music_feedback
        .notice_until
        .is_some_and(|until| Instant::now() >= until)
    {
        s.music_feedback.notice_until = None;
        u.set_feedback_notice("".into());
    }
    let current = current_video_id(s);
    if current != s.music_feedback.current_video {
        s.music_feedback.current_video = current.clone();
        s.music_feedback.inspected = false;
        u.set_current_rating(0);
    }
    let rating_allowed = !s.music_feedback.synthetic
        && s.account_verified
        && s.saved_connection.is_some()
        && current.is_some();
    u.set_rating_enabled(rating_allowed);
    u.set_collection_enabled(
        !s.music_feedback.synthetic && s.account_verified && s.saved_connection.is_some(),
    );
    if rating_allowed && !s.music_feedback.inspected && s.music_feedback.picker.is_none() {
        if let Some(video_id) = current.clone() {
            if begin(s, FeedbackRequest::Rating { video_id }) {
                s.music_feedback.inspected = true;
            }
        }
    }
    u.set_rating_busy(s.music_feedback.worker.is_some());
    u.set_collection_busy(s.music_feedback.worker.is_some());
    if s.music_feedback.synthetic
        || !s.settings.history_feedback
        || !s.active_playback
        || !s.account_verified
        || s.saved_connection.is_none()
    {
        s.music_feedback.listening.reset();
        return;
    }
    let (Some(video), Some(engine)) = (current_video_id(s), s.engine.as_ref()) else {
        s.music_feedback.listening.reset();
        return;
    };
    let p = engine.snapshot();
    if let Some(video_id) =
        s.music_feedback
            .listening
            .observe(s.generation, &video, &p, Instant::now())
    {
        if s.music_feedback.picker.is_none() && begin(s, FeedbackRequest::History { video_id }) {
            s.music_feedback.listening.submitted(s.generation);
            u.set_rating_busy(true);
            u.set_collection_busy(true);
        }
    }
}

fn playlist_error(code: Option<&str>) -> &'static str {
    match code {
        Some("feedback_readonly") => "This playlist cannot be edited from your selected account.",
        Some("feedback_entry_changed") => "This playlist entry changed. Refresh its songs before trying again.",
        _ => "The playlist update was not confirmed. Refresh the playlist to check its contents before trying again.",
    }
}

/// Bounded, read-only compatibility diagnostic. Never starts a UI, engine,
/// history worker, rating write or playlist mutation. Output contains no account
/// names, song identifiers, credentials or raw provider errors.
pub fn read_probe() -> Result<(), String> {
    use tide_provider::{Provider, Request, Response};
    let fail = || "Collection read check failed; no music feedback was submitted.".to_string();
    let provider = Provider::new().map_err(|_| fail())?;
    let expected = provider
        .saved_connection()
        .map_err(|_| fail())?
        .ok_or_else(fail)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let Response::Library { playlists, .. } = provider
        .execute(Request::Library, cancel.clone())
        .map_err(|_| fail())?
    else {
        return Err(fail());
    };
    let mut checked = 0;
    let mut inspected = false;
    let mut editable_entry = false;
    for playlist in playlists.iter().filter(|p| p.owned).take(3) {
        let Response::Tracks {
            tracks,
            playlist: Some(details),
            ..
        } = provider
            .execute(
                Request::Playlist {
                    id: playlist.id.clone(),
                },
                cancel.clone(),
            )
            .map_err(|_| fail())?
        else {
            return Err(fail());
        };
        if !details.editable || !details.valid_for(&playlist.id, tracks.len()) {
            return Err(fail());
        }
        checked += 1;
        editable_entry |= details.entries.iter().any(Option::is_some);
        if !inspected {
            if let Some(video_id) = tracks.iter().find_map(|t| artists::video_id(&t.source)) {
                let request = FeedbackRequest::Rating { video_id };
                let Response::Feedback { result } = provider
                    .execute(
                        Request::Feedback {
                            request: request.clone(),
                            expected: expected.clone(),
                        },
                        cancel.clone(),
                    )
                    .map_err(|_| fail())?
                else {
                    return Err(fail());
                };
                if !request.accepts(&result) || result.rating.is_none() {
                    return Err(fail());
                }
                inspected = true;
            }
        }
    }
    if checked == 0
        || !inspected
        || !editable_entry
        || provider.saved_connection().map_err(|_| fail())?.as_ref() != Some(&expected)
    {
        return Err(fail());
    }
    println!("Collection read check PASS: {checked} owned playlists; rating metadata available; zero feedback writes.");
    Ok(())
}
