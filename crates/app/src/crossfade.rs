//! Crossfade between songs with a second, short-lived playback engine.
//!
//! The playback engine plays one stream. To overlap two songs, a standby
//! engine (its own mpv) loads the next song silently and paused shortly
//! before the current one ends, then both play while their gains cross over
//! with equal-power curves. When the fade completes the standby becomes the
//! main engine and the old one shuts down, so idle memory is unchanged.
//! Gains use mpv's `volume-gain`, so the volume setting never moves.
//!
//! Any user action (seek, pause, skip, queue change), an error, a Jam or a
//! song too short to fade cancels it and restores the main engine's gain.
use super::*;
use tide_playback::{PlaybackHandle, SILENT_GAIN_DB};

/// Crossfade lengths offered in Settings, in seconds; 0 is off.
pub const LENGTHS: [u8; 5] = [0, 3, 6, 9, 12];
/// Start loading the next song this long before the fade begins.
const PRELOAD_SECONDS: f64 = 12.0;
/// Resend a gain only when it moves by at least this much.
const GAIN_STEP_DB: f64 = 0.5;

pub struct Crossfade<E = PlaybackHandle> {
    phase: Phase,
    standby: Option<E>,
    /// Gain last sent to the main engine; 0 when not fading.
    primary_db: f64,
    standby_db: f64,
}

impl<E> Default for Crossfade<E> {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            standby: None,
            primary_db: 0.0,
            standby_db: 0.0,
        }
    }
}

pub(crate) trait CrossfadeEngine: Sized {
    fn snapshot(&self) -> tide_core::PlaybackSnapshot;
    fn send(&self, command: PlaybackCommand) -> Result<(), String>;
    fn load_at(&self, source: String, generation: u64, position: f64) -> Result<(), String>;
    fn configure_audio(&self, options: tide_playback::AudioOptions) -> Result<(), String>;
    fn retire(self);
}

impl CrossfadeEngine for PlaybackHandle {
    fn snapshot(&self) -> tide_core::PlaybackSnapshot {
        PlaybackHandle::snapshot(self)
    }

    fn send(&self, command: PlaybackCommand) -> Result<(), String> {
        PlaybackHandle::send(self, command)
    }

    fn load_at(&self, source: String, generation: u64, position: f64) -> Result<(), String> {
        PlaybackHandle::load_at(self, source, generation, position)
    }

    fn configure_audio(&self, options: tide_playback::AudioOptions) -> Result<(), String> {
        PlaybackHandle::configure_audio(self, options)
    }

    /// Drop the standby engine off the UI thread; shutdown joins its worker.
    fn retire(self) {
        let _ = std::thread::Builder::new()
            .name("tide-crossfade-retire".into())
            .spawn(move || drop(self));
    }
}

#[derive(Clone, Copy)]
struct TickContext {
    local_controls_allowed: bool,
    jam_active: bool,
    active_playback: bool,
    sleep_end_of_song: bool,
    volume: f64,
    next: Option<usize>,
}

#[derive(Default, Clone, PartialEq, Debug)]
enum Phase {
    #[default]
    Idle,
    Preloading {
        index: usize,
        source: String,
        generation: u64,
    },
    Mixing {
        index: usize,
        source: String,
        generation: u64,
    },
}

/// Equal-power crossfade gains (outgoing, incoming) in dB at progress `t`.
pub fn gains(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let db = |linear: f64| {
        if linear <= 0.0 {
            SILENT_GAIN_DB
        } else {
            (20.0 * linear.log10()).max(SILENT_GAIN_DB)
        }
    };
    let angle = t * std::f64::consts::FRAC_PI_2;
    (db(angle.cos()), db(angle.sin()))
}

impl<E: CrossfadeEngine> Crossfade<E> {
    /// The main engine's current crossfade gain, for audio settings changes.
    pub fn primary_gain_db(&self) -> f64 {
        self.primary_db
    }

    fn set_primary_with(&mut self, settings: &Settings, primary: Option<&E>, gain_db: f64) {
        let options = audio_settings::options(settings, gain_db);
        if let Some(engine) = primary {
            let _ = engine.configure_audio(options);
        }
        self.primary_db = gain_db;
    }

    fn set_standby_with(&mut self, settings: &Settings, gain_db: f64) {
        let options = audio_settings::options(settings, gain_db);
        if let Some(engine) = self.standby.as_ref() {
            let _ = engine.configure_audio(options);
        }
        self.standby_db = gain_db;
    }

    /// Keep any preloaded or mixing standby engine at the same volume as the main engine.
    pub(crate) fn set_volume_with(
        &mut self,
        settings: &Settings,
        primary: Option<&E>,
        volume: f64,
    ) {
        let clamped = volume.clamp(0.0, 100.0);
        let failed = self
            .standby
            .as_ref()
            .is_some_and(|engine| engine.send(PlaybackCommand::SetVolume(clamped)).is_err());
        if failed {
            self.cancel_with(settings, primary);
        }
    }

    /// Abandon a pending or running crossfade and restore the main engine.
    pub(crate) fn cancel_with(&mut self, settings: &Settings, primary: Option<&E>) {
        if self.phase == Phase::Idle && self.standby.is_none() && self.primary_db == 0.0 {
            return;
        }
        self.phase = Phase::Idle;
        if let Some(standby) = self.standby.take() {
            let _ = standby.send(PlaybackCommand::Stop);
            standby.retire();
        }
        self.standby_db = 0.0;
        if self.primary_db != 0.0 {
            self.set_primary_with(settings, primary, 0.0);
        }
    }

    fn tick_with<F>(
        &mut self,
        primary: &mut Option<E>,
        settings: &mut Settings,
        generation: &mut u64,
        context: TickContext,
        p: &tide_core::PlaybackSnapshot,
        start_standby: F,
    ) -> bool
    where
        F: FnOnce() -> Result<E, String>,
    {
        let seconds = f64::from(settings.crossfade_seconds);
        // The outgoing song reaching its end completes a running mix: at EOF the
        // engine reports itself paused, which must not read as the user pausing.
        if let Phase::Mixing {
            index,
            source,
            generation: standby_generation,
        } = self.phase.clone()
        {
            let finished = p.generation == *generation
                && (p.ended || p.duration - p.position <= 0.15)
                && self
                    .standby
                    .as_ref()
                    .is_some_and(|engine| engine.snapshot().error.is_none());
            if finished {
                trace(|| "outgoing song ended; standby takes over".into());
                // A failed check cancels the mix; the ordinary transition follows.
                return self.take_over_with(
                    primary,
                    settings,
                    generation,
                    context.next,
                    index,
                    &source,
                    standby_generation,
                );
            }
        }
        let next = context.next;
        let next_source = next.and_then(|index| {
            settings
                .queue
                .tracks
                .get(index)
                .map(|track| track.source.clone())
        });
        let eligible = seconds > 0.0
            && !settings.bit_perfect
            && context.local_controls_allowed
            && !context.jam_active
            && context.active_playback
            && !context.sleep_end_of_song
            && primary.is_some()
            && p.generation == *generation
            && !p.paused
            && !p.loading
            && p.error.is_none()
            && p.duration.is_finite()
            && p.duration >= 2.0 * seconds + PRELOAD_SECONDS
            && next_source.is_some();
        if !eligible {
            if self.phase != Phase::Idle {
                trace(|| {
                    format!(
                        "cancelled: paused={} loading={} ended={} remaining={:.1}",
                        p.paused,
                        p.loading,
                        p.ended,
                        p.duration - p.position
                    )
                });
            }
            self.cancel_with(settings, primary.as_ref());
            return false;
        }
        let (Some(index), Some(source)) = (next, next_source) else {
            return false;
        };
        let remaining = p.duration - p.position;
        match self.phase.clone() {
            Phase::Idle => {
                if remaining > seconds + PRELOAD_SECONDS || remaining <= seconds + 1.0 {
                    return false;
                }
                let standby = match self.standby.take() {
                    Some(engine) => engine,
                    None => match start_standby() {
                        Ok(engine) => engine,
                        Err(_) => return false,
                    },
                };
                let standby_generation = (*generation).wrapping_add(1);
                self.standby = Some(standby);
                self.set_standby_with(settings, SILENT_GAIN_DB);
                let engine = self.standby.as_ref().unwrap();
                let loaded = engine
                    .send(PlaybackCommand::SetVolume(context.volume.clamp(0.0, 100.0)))
                    .and_then(|()| engine.load_at(source.clone(), standby_generation, 0.0))
                    .and_then(|()| engine.send(PlaybackCommand::SetPaused(true)));
                if loaded.is_err() {
                    self.cancel_with(settings, primary.as_ref());
                    return false;
                }
                trace(|| format!("preloading next song at {remaining:.1}s left"));
                self.phase = Phase::Preloading {
                    index,
                    source,
                    generation: standby_generation,
                };
                false
            }
            Phase::Preloading {
                index: planned,
                source: planned_source,
                generation: standby_generation,
            } => {
                // The queue, a seek or a skip changed what plays next.
                if planned != index
                    || planned_source != source
                    || remaining > seconds + PRELOAD_SECONDS + 2.0
                {
                    self.cancel_with(settings, primary.as_ref());
                    return false;
                }
                let Some(standby) = self.standby.as_ref().map(|e| e.snapshot()) else {
                    self.cancel_with(settings, primary.as_ref());
                    return false;
                };
                if standby.error.is_some() {
                    self.cancel_with(settings, primary.as_ref());
                    return false;
                }
                let ready = standby.generation == standby_generation
                    && !standby.loading
                    && standby.duration > 0.0;
                if remaining <= seconds && ready {
                    let engine = self.standby.as_ref().unwrap();
                    if engine.send(PlaybackCommand::SetPaused(false)).is_err() {
                        self.cancel_with(settings, primary.as_ref());
                        return false;
                    }
                    trace(|| format!("mixing from {remaining:.1}s left"));
                    self.phase = Phase::Mixing {
                        index,
                        source,
                        generation: standby_generation,
                    };
                } else if remaining <= 1.0 {
                    trace(|| {
                        format!(
                            "standby not ready in time (loading={} duration={})",
                            standby.loading, standby.duration
                        )
                    });
                    // Not ready in time: let the ordinary transition happen.
                    self.cancel_with(settings, primary.as_ref());
                }
                false
            }
            Phase::Mixing {
                index: planned,
                source: planned_source,
                generation: standby_generation,
            } => {
                if planned != index || planned_source != source || remaining > seconds + 1.0 {
                    self.cancel_with(settings, primary.as_ref());
                    return false;
                }
                let standby = self.standby.as_ref().map(|e| e.snapshot());
                if standby
                    .as_ref()
                    .is_none_or(|snapshot| snapshot.error.is_some())
                {
                    self.cancel_with(settings, primary.as_ref());
                    return false;
                }
                let t = 1.0 - (remaining / seconds);
                if t >= 1.0 || p.ended || remaining <= 0.15 {
                    return self.take_over_with(
                        primary,
                        settings,
                        generation,
                        context.next,
                        index,
                        &source,
                        standby_generation,
                    );
                }
                let (outgoing, incoming) = gains(t);
                if (outgoing - self.primary_db).abs() >= GAIN_STEP_DB {
                    self.set_primary_with(settings, primary.as_ref(), outgoing);
                }
                if (incoming - self.standby_db).abs() >= GAIN_STEP_DB {
                    self.set_standby_with(settings, incoming);
                }
                false
            }
        }
    }

    /// Promote the standby engine: it already plays the next song at full gain.
    /// Returns false, after cancelling the mix, when the plan no longer matches
    /// the queue or the standby engine.
    #[allow(clippy::too_many_arguments)]
    fn take_over_with(
        &mut self,
        primary: &mut Option<E>,
        settings: &mut Settings,
        generation: &mut u64,
        next: Option<usize>,
        index: usize,
        source: &str,
        standby_generation: u64,
    ) -> bool {
        let standby = self.standby.as_ref().map(|engine| engine.snapshot());
        if !promotion_matches(
            &settings.queue,
            next,
            index,
            source,
            standby_generation,
            standby.as_ref(),
        ) {
            trace(|| "planned song changed before the handoff; cancelled".into());
            self.cancel_with(settings, primary.as_ref());
            return false;
        }
        let Some(standby) = self.standby.take() else {
            return false;
        };
        self.phase = Phase::Idle;
        if let Some(old) = primary.replace(standby) {
            old.retire();
        }
        self.primary_db = self.standby_db;
        self.standby_db = 0.0;
        self.set_primary_with(settings, primary.as_ref(), 0.0);
        *generation = standby_generation;
        settings.queue.select(index);
        settings.playback_bookmark = None;
        true
    }
}

impl Crossfade {
    /// Abandon a pending or running crossfade and restore the main engine.
    pub fn cancel(s: &mut State) {
        s.crossfade.cancel_with(&s.settings, s.engine.as_ref());
    }
}

fn start_standby(
    lease: Option<PathBuf>,
    index: u32,
    connection_probe: bool,
) -> Result<PlaybackHandle, String> {
    if connection_probe {
        tide_playback::start_connection_probe(lease, index)
    } else {
        tide_playback::start_with_account(lease, index)
    }
}

/// Debug builds print phase changes when TIDE_CROSSFADE_TRACE is set.
fn trace(message: impl FnOnce() -> String) {
    if cfg!(debug_assertions) && std::env::var_os("TIDE_CROSSFADE_TRACE").is_some() {
        eprintln!("crossfade: {}", message());
    }
}

/// Advance the crossfade from the main engine's latest snapshot. Returns
/// true when the standby took over; the caller must then skip the rest of
/// this tick, since `p` describes the retired engine.
pub fn tick(s: &mut State, u: &AppWindow, p: &tide_core::PlaybackSnapshot) -> bool {
    let next = next_queue_index(s).filter(|&index| Some(index) != s.settings.queue.current);
    let context = TickContext {
        local_controls_allowed: local_controls_allowed(s),
        jam_active: u.get_jam_active(),
        active_playback: s.active_playback,
        sleep_end_of_song: s.sleep_timer == Some(SleepTimer::EndOfSong),
        volume: f64::from(u.get_volume()),
        next,
    };
    let lease = s.cookie_lease.as_ref().map(|l| l.path().to_path_buf());
    let index = s
        .saved_connection
        .as_ref()
        .map_or(0, |connection| connection.account_index);
    let connection_probe = s.connection_probe;
    if !s.crossfade.tick_with(
        &mut s.engine,
        &mut s.settings,
        &mut s.generation,
        context,
        p,
        || start_standby(lease, index, connection_probe),
    ) {
        return false;
    }
    s.active_playback = true;
    s.load_requested_at = None;
    refresh(u, s);
    u.set_paused(false);
    save(&s.settings);
    true
}

/// Whether the standby engine still holds exactly the song planned for the
/// queue row that plays next. A queue edit can shift another song into that
/// row between ticks, so the row index alone is not enough.
fn promotion_matches(
    queue: &Queue,
    next: Option<usize>,
    index: usize,
    source: &str,
    generation: u64,
    standby: Option<&tide_core::PlaybackSnapshot>,
) -> bool {
    next == Some(index)
        && queue
            .tracks
            .get(index)
            .is_some_and(|track| track.source == source)
        && standby
            .is_some_and(|snapshot| snapshot.generation == generation && snapshot.error.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_power_gains_cross_at_minus_three_db_and_end_silent() {
        assert_eq!(gains(0.0), (0.0, SILENT_GAIN_DB));
        let (out, inc) = gains(0.5);
        assert!((out - inc).abs() < 1e-9);
        assert!((out + 3.0103).abs() < 0.001);
        let (out, inc) = gains(1.0);
        assert!(out <= -90.0 + 1e-9 || out < -60.0);
        assert!(inc.abs() < 1e-9);
        assert_eq!(gains(-1.0), gains(0.0));
        assert_eq!(gains(2.0), gains(1.0));
    }

    fn track(source: &str) -> Track {
        Track {
            title: source.into(),
            artist: String::new(),
            artists: Vec::new(),
            album: String::new(),
            source: source.into(),
            duration_seconds: Some(120),
            artwork_url: None,
            recommended: false,
        }
    }

    #[test]
    fn handoff_rejects_a_song_that_moved_into_the_planned_row() {
        // A is mixing into B (row 1, standby generation 8) when B is removed
        // just before the final tick, so C slides into row 1.
        let queue = Queue {
            tracks: vec![track("a"), track("c")],
            current: Some(0),
        };
        let standby = tide_core::PlaybackSnapshot {
            generation: 8,
            ..Default::default()
        };
        assert!(!promotion_matches(
            &queue,
            Some(1),
            1,
            "b",
            8,
            Some(&standby)
        ));

        let planned = Queue {
            tracks: vec![track("a"), track("b"), track("c")],
            current: Some(0),
        };
        assert!(promotion_matches(
            &planned,
            Some(1),
            1,
            "b",
            8,
            Some(&standby)
        ));
        // A stale standby generation, a failed standby or a changed next row
        // also refuse the promotion.
        assert!(!promotion_matches(
            &planned,
            Some(1),
            1,
            "b",
            9,
            Some(&standby)
        ));
        let failed = tide_core::PlaybackSnapshot {
            generation: 8,
            error: Some("failed".into()),
            ..Default::default()
        };
        assert!(!promotion_matches(
            &planned,
            Some(1),
            1,
            "b",
            8,
            Some(&failed)
        ));
        assert!(!promotion_matches(
            &planned,
            Some(2),
            1,
            "b",
            8,
            Some(&standby)
        ));
        assert!(!promotion_matches(&planned, Some(1), 1, "b", 8, None));
    }

    #[derive(Clone, Default)]
    struct FakeEngineState {
        snapshot: tide_core::PlaybackSnapshot,
        reject_volume: bool,
        reject_controls: bool,
        gain_db: f64,
        stopped: bool,
        retired: bool,
    }

    #[derive(Clone)]
    struct FakeEngine(Rc<RefCell<FakeEngineState>>);

    impl FakeEngine {
        fn new(volume: f64) -> Self {
            Self(Rc::new(RefCell::new(FakeEngineState {
                snapshot: tide_core::PlaybackSnapshot {
                    volume,
                    paused: true,
                    ..Default::default()
                },
                ..Default::default()
            })))
        }

        fn state(&self) -> std::cell::Ref<'_, FakeEngineState> {
            self.0.borrow()
        }

        fn state_mut(&self) -> std::cell::RefMut<'_, FakeEngineState> {
            self.0.borrow_mut()
        }
    }

    impl CrossfadeEngine for FakeEngine {
        fn snapshot(&self) -> tide_core::PlaybackSnapshot {
            self.0.borrow().snapshot.clone()
        }

        fn send(&self, command: PlaybackCommand) -> Result<(), String> {
            let mut state = self.0.borrow_mut();
            match command {
                PlaybackCommand::Seek(_) | PlaybackCommand::SetPaused(_)
                    if state.reject_controls =>
                {
                    return Err("control queue full".into());
                }
                PlaybackCommand::SetVolume(volume) => {
                    if state.reject_volume {
                        return Err("volume queue full".into());
                    }
                    if !volume.is_finite() || !(0.0..=100.0).contains(&volume) {
                        return Err("invalid volume".into());
                    }
                    state.snapshot.volume = volume;
                }
                PlaybackCommand::SetPaused(paused) => {
                    state.snapshot.paused = paused;
                }
                PlaybackCommand::Seek(position) => {
                    state.snapshot.position = position;
                }
                PlaybackCommand::Stop | PlaybackCommand::Shutdown => {
                    state.stopped = true;
                    state.snapshot.paused = true;
                    state.snapshot.position = 0.0;
                    state.snapshot.duration = 0.0;
                    state.snapshot.loading = false;
                }
                _ => {}
            }
            Ok(())
        }

        fn load_at(&self, _source: String, generation: u64, position: f64) -> Result<(), String> {
            let mut state = self.0.borrow_mut();
            state.stopped = false;
            state.snapshot.generation = generation;
            state.snapshot.position = position;
            state.snapshot.duration = 0.0;
            state.snapshot.paused = false;
            state.snapshot.loading = true;
            state.snapshot.ended = false;
            state.snapshot.error = None;
            Ok(())
        }

        fn configure_audio(&self, options: tide_playback::AudioOptions) -> Result<(), String> {
            self.0.borrow_mut().gain_db = options.gain_db;
            Ok(())
        }

        fn retire(self) {
            self.0.borrow_mut().retired = true;
        }
    }

    fn crossfade_settings() -> Settings {
        Settings {
            crossfade_seconds: 6,
            queue: Queue {
                tracks: vec![track("a"), track("b")],
                current: Some(0),
            },
            ..Settings::default()
        }
    }

    fn tick_context(volume: f64, next: Option<usize>) -> TickContext {
        TickContext {
            local_controls_allowed: true,
            jam_active: false,
            active_playback: true,
            sleep_end_of_song: false,
            volume,
            next,
        }
    }

    #[test]
    fn rejected_primary_mute_stops_the_incoming_track() {
        let primary = FakeEngine::new(60.0);
        primary.state_mut().reject_volume = true;
        let standby = FakeEngine::new(60.0);
        standby.state_mut().snapshot.paused = false;
        let mut crossfade = Crossfade {
            standby: Some(standby.clone()),
            phase: Phase::Mixing {
                index: 1,
                source: "b".into(),
                generation: 2,
            },
            primary_db: -3.0,
            standby_db: -3.0,
        };
        let result = route_command(
            Some(&primary),
            &mut crossfade,
            &crossfade_settings(),
            PlaybackCommand::SetVolume(0.0),
        );
        assert_eq!(result.unwrap_err(), "volume queue full");
        assert_eq!(crossfade.phase, Phase::Idle);
        assert!(standby.state().stopped && standby.state().retired);
        assert_eq!(primary.state().gain_db, 0.0);
    }

    #[test]
    fn seek_and_pause_cancel_the_crossfade_only_when_accepted() {
        for command in [
            PlaybackCommand::Seek(32.0),
            PlaybackCommand::SetPaused(true),
        ] {
            let settings = crossfade_settings();
            let primary = FakeEngine::new(60.0);
            {
                let mut state = primary.state_mut();
                state.reject_controls = true;
                state.snapshot.paused = false;
                state.gain_db = -3.0;
            }
            let standby = FakeEngine::new(60.0);
            standby.state_mut().snapshot.paused = false;
            let mut crossfade = Crossfade {
                standby: Some(standby.clone()),
                phase: Phase::Mixing {
                    index: 1,
                    source: "b".into(),
                    generation: 2,
                },
                primary_db: -3.0,
                standby_db: -3.0,
            };

            let rejected =
                route_command(Some(&primary), &mut crossfade, &settings, command.clone());
            assert_eq!(rejected.unwrap_err(), "control queue full");
            assert!(matches!(crossfade.phase, Phase::Mixing { .. }));
            assert!(!standby.state().stopped && !standby.state().retired);
            assert_eq!(primary.state().gain_db, -3.0);

            primary.state_mut().reject_controls = false;
            route_command(Some(&primary), &mut crossfade, &settings, command).unwrap();
            assert_eq!(crossfade.phase, Phase::Idle);
            assert!(standby.state().stopped && standby.state().retired);
            assert_eq!(primary.state().gain_db, 0.0);
        }
    }

    #[test]
    fn volume_changes_during_preload_and_mixing_reach_standby_and_survive_promotion() {
        let mut settings = crossfade_settings();
        let mut generation = 1u64;
        let primary_probe = FakeEngine::new(60.0);
        {
            let mut p = primary_probe.state_mut();
            p.snapshot.generation = generation;
            p.snapshot.duration = 120.0;
            p.snapshot.position = 105.0; // 15s remaining -> triggers preload
            p.snapshot.paused = false;
        }
        let mut primary = Some(primary_probe.clone());
        let standby_probe = FakeEngine::new(100.0);
        let mut crossfade = Crossfade::<FakeEngine>::default();

        // Enter Preloading at 60% volume.
        let promoted = crossfade.tick_with(
            &mut primary,
            &mut settings,
            &mut generation,
            tick_context(60.0, Some(1)),
            &primary_probe.snapshot(),
            || Ok(standby_probe.clone()),
        );
        assert!(!promoted);
        assert!(matches!(crossfade.phase, Phase::Preloading { .. }));
        assert_eq!(primary_probe.state().snapshot.volume, 60.0);
        assert_eq!(standby_probe.state().snapshot.volume, 60.0);

        // Mute during preload: both engines must drop to 0%.
        route_command(
            primary.as_ref(),
            &mut crossfade,
            &settings,
            PlaybackCommand::SetVolume(0.0),
        )
        .unwrap();
        assert_eq!(primary_probe.state().snapshot.volume, 0.0);
        assert_eq!(standby_probe.state().snapshot.volume, 0.0);

        // Standby finishes loading and overlap begins at 6s remaining.
        {
            let mut s = standby_probe.state_mut();
            s.snapshot.loading = false;
            s.snapshot.duration = 120.0;
        }
        primary_probe.state_mut().snapshot.position = 114.5;
        let promoted = crossfade.tick_with(
            &mut primary,
            &mut settings,
            &mut generation,
            tick_context(0.0, Some(1)),
            &primary_probe.snapshot(),
            || Err("unexpected standby start".into()),
        );
        assert!(!promoted);
        assert!(matches!(crossfade.phase, Phase::Mixing { .. }));
        assert!(!standby_probe.state().snapshot.paused);
        assert_eq!(standby_probe.state().snapshot.volume, 0.0);

        // Change volume during mixing (0% -> 25% -> 0%): both engines follow every change.
        primary_probe.state_mut().snapshot.position = 117.0;
        let promoted = crossfade.tick_with(
            &mut primary,
            &mut settings,
            &mut generation,
            tick_context(0.0, Some(1)),
            &primary_probe.snapshot(),
            || Err("unexpected standby start".into()),
        );
        assert!(!promoted);
        route_command(
            primary.as_ref(),
            &mut crossfade,
            &settings,
            PlaybackCommand::SetVolume(25.0),
        )
        .unwrap();
        assert_eq!(primary_probe.state().snapshot.volume, 25.0);
        assert_eq!(standby_probe.state().snapshot.volume, 25.0);

        route_command(
            primary.as_ref(),
            &mut crossfade,
            &settings,
            PlaybackCommand::SetVolume(0.0),
        )
        .unwrap();
        assert_eq!(primary_probe.state().snapshot.volume, 0.0);
        assert_eq!(standby_probe.state().snapshot.volume, 0.0);

        // Complete handoff: promoted engine stays muted at 0% and normal 0 dB gain.
        primary_probe.state_mut().snapshot.position = 119.9;
        let promoted = crossfade.tick_with(
            &mut primary,
            &mut settings,
            &mut generation,
            tick_context(0.0, Some(1)),
            &primary_probe.snapshot(),
            || Err("unexpected standby start".into()),
        );
        assert!(promoted);
        assert!(primary_probe.state().retired);
        let promoted_engine = primary.as_ref().unwrap();
        assert_eq!(promoted_engine.state().snapshot.volume, 0.0);
        assert_eq!(promoted_engine.state().gain_db, 0.0);
        assert_eq!(crossfade.primary_gain_db(), 0.0);
    }

    #[test]
    fn clear_queue_and_media_stop_during_crossfade_stop_both_engines_and_restore_gain() {
        for clear_queue in [true, false] {
            let mut settings = crossfade_settings();
            let mut generation = 1u64;
            let primary_probe = FakeEngine::new(70.0);
            {
                let mut p = primary_probe.state_mut();
                p.snapshot.generation = generation;
                p.snapshot.duration = 120.0;
                p.snapshot.position = 105.0;
                p.snapshot.paused = false;
            }
            let mut primary = Some(primary_probe.clone());
            let standby_probe = FakeEngine::new(100.0);
            let mut crossfade = Crossfade::<FakeEngine>::default();

            // Preload, then advance into audible Mixing overlap so primary gain is attenuated.
            assert!(!crossfade.tick_with(
                &mut primary,
                &mut settings,
                &mut generation,
                tick_context(70.0, Some(1)),
                &primary_probe.snapshot(),
                || Ok(standby_probe.clone()),
            ));
            {
                let mut s = standby_probe.state_mut();
                s.snapshot.loading = false;
                s.snapshot.duration = 120.0;
            }
            primary_probe.state_mut().snapshot.position = 114.5;
            assert!(!crossfade.tick_with(
                &mut primary,
                &mut settings,
                &mut generation,
                tick_context(70.0, Some(1)),
                &primary_probe.snapshot(),
                || Err("unexpected standby start".into()),
            ));
            primary_probe.state_mut().snapshot.position = 117.0;
            assert!(!crossfade.tick_with(
                &mut primary,
                &mut settings,
                &mut generation,
                tick_context(70.0, Some(1)),
                &primary_probe.snapshot(),
                || Err("unexpected standby start".into()),
            ));
            assert!(matches!(crossfade.phase, Phase::Mixing { .. }));
            assert!(!standby_probe.state().snapshot.paused);
            assert!(primary_probe.state().gain_db < -1.0);

            // Both clear-queue and system media-controls Stop route Stop and mark playback inactive.
            route_command(
                primary.as_ref(),
                &mut crossfade,
                &settings,
                PlaybackCommand::Stop,
            )
            .unwrap();
            if clear_queue {
                settings.queue = Queue::default();
            }

            assert!(primary_probe.state().stopped);
            assert!(primary_probe.state().snapshot.paused);
            assert!(standby_probe.state().stopped);
            assert!(standby_probe.state().snapshot.paused);
            assert!(standby_probe.state().retired);
            assert!(crossfade.standby.is_none());
            assert_eq!(crossfade.phase, Phase::Idle);
            assert_eq!(primary_probe.state().gain_db, 0.0);
            assert_eq!(crossfade.primary_gain_db(), 0.0);
        }
    }
}
