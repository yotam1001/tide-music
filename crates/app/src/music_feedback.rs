//! Collection operations are independent of browsing and playback workers.
use super::*;
use tide_provider::{FeedbackRequest, SavedConnection};

struct Work {
    handle: std::thread::JoinHandle<()>,
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<LibraryResult>,
    request: FeedbackRequest,
    expected: SavedConnection,
}
pub struct Music {
    worker: Option<Work>,
    identity: Option<SavedConnection>,
    listening: history_feedback::Listening,
    synthetic: bool,
}
impl Music {
    pub fn new(synthetic: bool) -> Self {
        Self {
            worker: None,
            identity: None,
            listening: Default::default(),
            synthetic,
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
    });
    true
}

pub fn install(state: &Rc<RefCell<State>>, ui: &AppWindow) {
    ui.set_history_feedback_enabled(state.borrow().settings.history_feedback);
    ui.set_history_feedback_status(
        "Only real playback is counted. Automated tests never update history.".into(),
    );
    let state = state.clone();
    let weak = ui.as_weak();
    ui.on_toggle_history_feedback(move || {
        let Some(u) = weak.upgrade() else { return; };
        let mut s = state.borrow_mut();
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
}

pub fn tick(s: &mut State, u: &AppWindow) {
    if s.music_feedback.identity != s.saved_connection {
        if let Some(work) = &s.music_feedback.worker {
            work.cancel.store(true, Ordering::Release);
        }
        s.music_feedback.listening.reset();
        s.music_feedback.identity = s.saved_connection.clone();
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
            let confirmed = matches!(work.receiver.try_recv(),
                Ok(Ok(tide_provider::Response::Feedback { result })) if work.request.accepts(&result));
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
        if begin(s, FeedbackRequest::History { video_id }) {
            s.music_feedback.listening.submitted(s.generation);
        }
    }
}
