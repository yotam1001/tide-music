//! Opt-in `--drive-generated-playlists`: with the signed-in account, open My
//! Supermix and every YouTube-generated list (mixes, recaps: ids that are not
//! `PL`/`VL`) from the library and Home, then scroll each one hard while real
//! artwork loads. Settings are not saved and nothing is played. Output names
//! only list kinds, id prefixes and counts.
use super::*;
use slint::platform::WindowEvent;

const WAIT: Duration = Duration::from_secs(90);
const SCROLL: Duration = Duration::from_secs(25);

#[derive(Clone, Debug)]
enum Target {
    Supermix,
    Library(usize),
    Home(usize),
}

fn generated(id: &str) -> bool {
    !id.starts_with("PL") && !id.starts_with("VL")
}

fn prefix(id: &str) -> String {
    id.chars().take(4).collect()
}

pub(super) fn start(state: &Rc<RefCell<State>>, ui: &AppWindow) -> slint::Timer {
    let timer = slint::Timer::default();
    let state = state.clone();
    let weak = ui.as_weak();
    let began = Instant::now();
    let mut targets: Option<Vec<(Target, String)>> = None;
    let mut current: Option<(Target, String, Instant, Option<Instant>)> = None;
    let mut ticks = 0_u64;
    let mut seed = 0x2545_f491_u32;
    let mut direction = -1.0_f32;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(u) = weak.upgrade() else { return };
            ticks += 1;
            if targets.is_none() {
                let s = state.borrow();
                let ready = s.account_verified
                    && !s.library_playlists.is_empty()
                    && !s.recommendations.is_empty()
                    && s.library_worker.is_none();
                if !ready {
                    if began.elapsed() > WAIT {
                        eprintln!(
                            "DRIVE: account/library not ready (verified={}, playlists={}, home={})",
                            s.account_verified,
                            s.library_playlists.len(),
                            s.recommendations.len()
                        );
                        let _ = slint::quit_event_loop();
                    }
                    return;
                }
                let mut list = vec![(Target::Supermix, "supermix".to_owned())];
                for (i, p) in s.library_playlists.iter().enumerate() {
                    if generated(&p.id) {
                        list.push((Target::Library(i), prefix(&p.id)));
                    }
                }
                for (i, c) in s.recommendations.iter().enumerate() {
                    if c.kind == "playlist" && generated(&c.id) {
                        list.push((Target::Home(i), prefix(&c.id)));
                    }
                }
                list.truncate(12);
                println!(
                    "DRIVE: {} targets ({} library playlists, {} Home cards)",
                    list.len(),
                    s.library_playlists.len(),
                    s.recommendations.len()
                );
                targets = Some(list);
                return;
            }
            let Some((target, label, opened, loaded)) = current.as_mut() else {
                let next = targets
                    .as_mut()
                    .and_then(|t| (!t.is_empty()).then(|| t.remove(0)));
                let Some((target, label)) = next else {
                    println!("DRIVE: finished without a crash");
                    let _ = slint::quit_event_loop();
                    return;
                };
                if state.borrow().library_worker.is_some() {
                    targets.as_mut().unwrap().insert(0, (target, label));
                    return;
                }
                println!("DRIVE: opening {target:?} [{label}]");
                match target {
                    Target::Supermix => u.invoke_open_supermix(),
                    Target::Library(i) => u.invoke_select_library_playlist(i as i32),
                    Target::Home(i) => u.invoke_select_recommendation(i as i32),
                }
                current = Some((target, label, Instant::now(), None));
                return;
            };
            if loaded.is_none() {
                let rows = u.get_detail_rows().row_count();
                if u.get_detail_open() && !u.get_detail_busy() && rows > 0 {
                    println!("DRIVE: {target:?} [{label}] loaded {rows} tracks; scrolling");
                    *loaded = Some(Instant::now());
                    u.set_detail_artwork_y(0.0);
                } else if opened.elapsed() > Duration::from_secs(40) {
                    println!("DRIVE: {target:?} [{label}] did not load; skipping");
                    current = None;
                }
                return;
            }
            let row = u.get_detail_row_height().max(1.0);
            let view = u.get_detail_artwork_height();
            let count = u.get_detail_rows().row_count() as f32;
            let max_y = (count * row - view).max(0.0);
            let y = -u.get_detail_artwork_y();
            if (y >= max_y - 1.0 && direction < 0.0) || (y <= 1.0 && direction > 0.0) {
                direction = -direction;
            }
            let size = u.window().size().to_logical(u.window().scale_factor());
            let (w, h) = (size.width.max(400.0), size.height.max(400.0));
            u.window().dispatch_event(WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(w * 0.6, h * 0.8),
                delta_x: 0.0,
                delta_y: direction * if ticks % 50 < 25 { 120.0 } else { 15.0 },
            });
            for step in 0..3 {
                let x = w * 0.25 + ((ticks * 37 + step * 113) % (w as u64 / 2)) as f32;
                let yy = h * 0.2 + ((ticks * 53 + step * 71) % (h as u64 * 7 / 10)) as f32;
                u.window().dispatch_event(WindowEvent::PointerMoved {
                    position: slint::LogicalPosition::new(x, yy),
                });
            }
            if ticks.is_multiple_of(9) {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                u.set_detail_artwork_y(-(seed as f32 / u32::MAX as f32) * max_y);
            }
            if loaded.is_some_and(|at| at.elapsed() > SCROLL) {
                println!("DRIVE: {target:?} [{label}] survived");
                current = None;
            }
        },
    );
    timer
}

/// Opt-in `--drive-playback`: with the signed-in account, play up to five
/// Home songs one at a time (muted, settings unsaved) and report whether each
/// starts or which failure category the player shows.
pub(super) fn start_playback(state: &Rc<RefCell<State>>, ui: &AppWindow) -> slint::Timer {
    let timer = slint::Timer::default();
    let state = state.clone();
    let weak = ui.as_weak();
    let began = Instant::now();
    let mut songs: Option<Vec<Track>> = None;
    let mut current: Option<(String, Instant)> = None;
    let (mut played, mut failed) = (0_u32, 0_u32);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(200),
        move || {
            let Some(u) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            if songs.is_none() {
                if !s.account_verified || s.recommendations.is_empty() {
                    if began.elapsed() > WAIT {
                        eprintln!(
                            "PLAY: account/Home not ready (verified={})",
                            s.account_verified
                        );
                        let _ = slint::quit_event_loop();
                    }
                    return;
                }
                let list: Vec<Track> = s
                    .recommendations
                    .iter()
                    .filter(|c| c.kind == "song")
                    .take(5)
                    .map(recommendation_track)
                    .collect();
                println!("PLAY: {} Home songs to try", list.len());
                songs = Some(list);
                return;
            }
            if let Some((id, started)) = &current {
                let Some(p) = s.engine.as_ref().map(|e| e.snapshot()) else {
                    println!("PLAY: no playback engine");
                    let _ = slint::quit_event_loop();
                    return;
                };
                if p.generation == s.generation && !p.loading && p.position > 3.0 {
                    println!("PLAY: {id} plays ({} retries)", p.retry_attempt);
                    played += 1;
                } else if p.error.is_some() {
                    println!(
                        "PLAY: {id} failed: {:?}; engine: {:?}; banner: {}",
                        p.source_failure,
                        p.error,
                        u.get_error_message()
                    );
                    failed += 1;
                } else if started.elapsed() > Duration::from_secs(75) {
                    println!(
                        "PLAY: {id} timed out (loading={}, retry={})",
                        p.loading, p.retry_attempt
                    );
                    failed += 1;
                } else {
                    return;
                }
                current = None;
                return;
            }
            let next = songs
                .as_mut()
                .and_then(|l| (!l.is_empty()).then(|| l.remove(0)));
            let Some(track) = next else {
                println!("PLAY: done, {played} played, {failed} failed");
                let _ = slint::quit_event_loop();
                return;
            };
            let id = source_video_id(&track.source).unwrap_or_default();
            s.settings.queue = Queue {
                tracks: vec![track],
                current: Some(0),
            };
            if !play(&mut s, &u) {
                println!("PLAY: {id} could not start");
                failed += 1;
                return;
            }
            current = Some((id, Instant::now()));
        },
    );
    timer
}
