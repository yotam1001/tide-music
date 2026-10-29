//! On-demand lyrics. One cancellable worker, one song, no disk cache.
use super::*;
use tide_provider::{LyricLine, Response, SavedConnection};

#[derive(Clone, PartialEq, Eq)]
struct Key {
    generation: u64,
    video: String,
    connection: SavedConnection,
}
struct Work {
    key: Key,
    cancel: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
    receiver: mpsc::Receiver<Result<Response, String>>,
}
#[derive(Default)]
struct Runtime {
    key: Option<Key>,
    worker: Option<Work>,
    lines: Vec<LyricLine>,
    attempted: bool,
}
impl Runtime {
    fn accepts(&self, key: &Key, cancelled: bool) -> bool {
        !cancelled && self.key.as_ref() == Some(key)
    }
}

pub struct Lyrics {
    timer: slint::Timer,
    runtime: Rc<RefCell<Runtime>>,
}
impl Drop for Lyrics {
    fn drop(&mut self) {
        self.timer.stop();
        if let Some(work) = self.runtime.borrow_mut().worker.take() {
            work.cancel.store(true, Ordering::Release);
            let _ = work.handle.join();
        }
    }
}

fn valid(lines: &[LyricLine], synced: bool, source: &str) -> bool {
    lines.len() <= 1000
        && source.len() <= 512
        && !source.chars().any(char::is_control)
        && lines.iter().map(|l| l.text.len()).sum::<usize>() <= 128_000
        && lines.iter().all(|l| {
            l.text.chars().count() <= 500
                && !l.text.chars().any(char::is_control)
                && if synced {
                    l.start_ms.is_some_and(|t| {
                        t <= 86_400_000 && l.end_ms.is_none_or(|end| end >= t && end <= 86_400_000)
                    })
                } else {
                    l.start_ms.is_none() && l.end_ms.is_none()
                }
        })
        && (!synced || lines.windows(2).all(|p| p[0].start_ms <= p[1].start_ms))
}

fn active_line(lines: &[LyricLine], position: f64) -> i32 {
    if !position.is_finite() || position < 0.0 || lines.first().is_none_or(|l| l.start_ms.is_none())
    {
        return -1;
    }
    let end = lines.partition_point(|l| l.start_ms.unwrap_or(u64::MAX) as f64 <= position * 1000.0);
    end.checked_sub(1)
        .filter(|&i| {
            !lines[i].text.is_empty()
                && lines[i]
                    .end_ms
                    .is_none_or(|end| position * 1000.0 < end as f64)
        })
        .map_or(-1, |i| i as i32)
}

fn clear(ui: &AppWindow) {
    ui.set_lyrics_lines(ModelRc::new(VecModel::<LyricRow>::default()));
    ui.set_lyrics_active(-1);
    ui.set_lyrics_synced(false);
    ui.set_lyrics_source("".into());
}

pub fn install(state: &Rc<RefCell<State>>, ui: &AppWindow) -> Lyrics {
    let runtime = Rc::new(RefCell::new(Runtime::default()));
    {
        let runtime = runtime.clone();
        let weak = ui.as_weak();
        ui.on_reload_lyrics(move || {
            let mut r = runtime.borrow_mut();
            if r.worker.is_none() {
                r.attempted = false;
                r.lines.clear();
                if let Some(ui) = weak.upgrade() {
                    clear(&ui);
                }
            }
        });
    }
    {
        let state = state.clone();
        let runtime = runtime.clone();
        let weak = ui.as_weak();
        ui.on_seek_lyric(move |index| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let s = state.borrow();
            let r = runtime.borrow();
            let Some(key) = &r.key else {
                return;
            };
            if !s.active_playback
                || !local_controls_allowed(&s)
                || key.generation != s.generation
                || s.saved_connection.as_ref() != Some(&key.connection)
                || current_video_id(&s).as_deref() != Some(key.video.as_str())
            {
                return;
            }
            let time = usize::try_from(index)
                .ok()
                .and_then(|i| r.lines.get(i))
                .and_then(|l| l.start_ms);
            drop(r);
            drop(s);
            if let Some(time) = time {
                ui.invoke_seek(time as f32 / 1000.0);
            }
        });
    }
    let timer = slint::Timer::default();
    {
        let state = state.clone();
        let runtime = runtime.clone();
        let weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let s = state.borrow();
                let mut r = runtime.borrow_mut();
                let key = if ui.get_lyrics_open()
                    && !ui.get_collapsed()
                    && !ui.window().is_minimized()
                    && s.active_playback
                {
                    current_video_id(&s).zip(s.saved_connection.clone()).map(
                        |(video, connection)| Key {
                            generation: s.generation,
                            video,
                            connection,
                        },
                    )
                } else {
                    None
                };
                if key != r.key {
                    // Finish the bounded read to preserve refreshed cookies;
                    // the generation/account key rejects its stale UI result.
                    r.key = key.clone();
                    r.attempted = false;
                    r.lines.clear();
                    clear(&ui);
                    ui.set_lyrics_status(
                        if key.is_some() {
                            "Loading lyrics…"
                        } else {
                            "Play a YouTube Music song to see its lyrics."
                        }
                        .into(),
                    );
                }
                if r.worker.as_ref().is_some_and(|w| w.handle.is_finished()) {
                    let work = r.worker.take().unwrap();
                    let _ = work.handle.join();
                    if r.accepts(&work.key, work.cancel.load(Ordering::Acquire)) {
                        match work.receiver.try_recv() {
                            Ok(Ok(Response::Lyrics {
                                lines,
                                synced,
                                source,
                            })) if valid(&lines, synced, &source) => {
                                ui.set_lyrics_synced(synced);
                                ui.set_lyrics_source(source.into());
                                ui.set_lyrics_status(
                                    if lines.is_empty() {
                                        "Lyrics aren’t available for this song."
                                    } else if synced {
                                        "Synced lyrics"
                                    } else {
                                        "Lyrics · timing unavailable"
                                    }
                                    .into(),
                                );
                                ui.set_lyrics_lines(ModelRc::new(VecModel::from(
                                    lines
                                        .iter()
                                        .map(|l| LyricRow {
                                            text: l.text.clone().into(),
                                        })
                                        .collect::<Vec<_>>(),
                                )));
                                r.lines = lines;
                            }
                            _ => ui.set_lyrics_status("Couldn’t load lyrics. Try again.".into()),
                        }
                    }
                }
                if key.is_none() && ui.get_lyrics_open() {
                    let message = if s.active_playback && current_video_id(&s).is_some() {
                        "Connect your YouTube Music account to load lyrics."
                    } else {
                        "Play a YouTube Music song to see its lyrics."
                    };
                    if ui.get_lyrics_status().as_str() != message {
                        ui.set_lyrics_status(message.into());
                    }
                }
                if let Some(key) = key {
                    if !r.attempted && r.worker.is_none() {
                        r.attempted = true;
                        ui.set_lyrics_status("Loading lyrics…".into());
                        if let Some(provider) = s.provider.clone() {
                            let (tx, receiver) = mpsc::sync_channel(1);
                            let cancel = Arc::new(AtomicBool::new(false));
                            let flag = cancel.clone();
                            let video_id = key.video.clone();
                            match std::thread::Builder::new()
                                .name("tide-lyrics".into())
                                .spawn(move || {
                                    let _ = tx.try_send(provider.execute(
                                        tide_provider::Request::Lyrics { video_id },
                                        flag,
                                    ));
                                }) {
                                Ok(handle) => {
                                    r.worker = Some(Work {
                                        key: key.clone(),
                                        cancel,
                                        handle,
                                        receiver,
                                    })
                                }
                                Err(_) => {
                                    ui.set_lyrics_status("Couldn’t load lyrics. Try again.".into())
                                }
                            }
                        } else {
                            ui.set_lyrics_status(
                                "Connect your YouTube Music account to load lyrics.".into(),
                            );
                        }
                    }
                    let position = s
                        .engine
                        .as_ref()
                        .map(|e| e.snapshot())
                        .filter(|p| p.generation == key.generation && !p.loading)
                        .map_or(f64::NAN, |p| p.position);
                    let active = active_line(&r.lines, position);
                    if ui.get_lyrics_active() != active {
                        ui.set_lyrics_active(active);
                    }
                }
            },
        );
    }
    Lyrics { timer, runtime }
}

/// Read-only maintainer probe: counts only, never lyrics or account material.
pub fn probe(video: String) -> Result<(), String> {
    if video.len() != 11
        || !video
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("Expected a YouTube video ID".into());
    }
    match tide_provider::Provider::new()?.execute(
        tide_provider::Request::Lyrics { video_id: video },
        Arc::new(AtomicBool::new(false)),
    )? {
        Response::Lyrics {
            lines,
            synced,
            source,
        } if valid(&lines, synced, &source) => {
            println!("PASS lyrics read: {} lines, synced={}", lines.len(), synced);
            Ok(())
        }
        _ => Err("Lyrics could not be read".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(time: Option<u64>, text: &str) -> LyricLine {
        LyricLine {
            text: text.into(),
            start_ms: time,
            end_ms: None,
        }
    }
    #[test]
    fn stale_songs_accounts_and_cancelled_results_are_never_accepted() {
        let first = Key {
            generation: 3,
            video: "fixture0001".into(),
            connection: SavedConnection {
                revision: Some("fixture-revision".into()),
                account_index: 0,
                page_id: None,
            },
        };
        let mut r = Runtime {
            key: Some(first.clone()),
            ..Default::default()
        };
        assert!(r.accepts(&first, false));
        assert!(!r.accepts(&first, true));
        let mut next = first.clone();
        next.generation += 1;
        r.key = Some(next);
        assert!(!r.accepts(&first, false));
        let mut account = first.clone();
        account.connection.account_index = 1;
        r.key = Some(account);
        assert!(!r.accepts(&first, false));
        r.key = None;
        assert!(!r.accepts(&first, false));
    }

    #[test]
    fn follows_real_position_seeks_and_instrumental_breaks() {
        let lines = vec![
            line(Some(1000), "First"),
            line(Some(5000), ""),
            line(Some(7000), "Next"),
        ];
        assert_eq!(active_line(&lines, 0.0), -1);
        assert_eq!(active_line(&lines, 1.0), 0);
        assert_eq!(active_line(&lines, 6.0), -1);
        assert_eq!(active_line(&lines, 7.0), 2);
        assert_eq!(active_line(&lines, 2.0), 0);
        let ended = LyricLine {
            text: "Finished line".into(),
            start_ms: Some(1000),
            end_ms: Some(1500),
        };
        assert_eq!(active_line(&[ended], 1.6), -1);
        assert_eq!(active_line(&lines, f64::NAN), -1);
        assert_eq!(active_line(&[line(None, "Plain")], 10.0), -1);
    }
    #[test]
    fn rejects_oversized_mixed_and_unordered_timings() {
        assert!(valid(&[line(Some(0), "Hello")], true, "Source"));
        assert!(!valid(&[line(Some(9), "a"), line(Some(3), "b")], true, ""));
        assert!(!valid(&[line(None, "a")], true, ""));
        assert!(!valid(&[line(Some(0), "a")], false, ""));
        assert!(!valid(&vec![line(None, "x"); 1001], false, ""));
        assert!(!valid(&[line(Some(0), &"x".repeat(501))], true, ""));
        assert!(!valid(&[line(Some(0), "a\u{1b}b")], true, ""));
    }
}
