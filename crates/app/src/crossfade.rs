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

#[derive(Default)]
pub struct Crossfade {
    phase: Phase,
    standby: Option<PlaybackHandle>,
    /// Gain last sent to the main engine; 0 when not fading.
    primary_db: f64,
    standby_db: f64,
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

impl Crossfade {
    /// The main engine's current crossfade gain, for audio settings changes.
    pub fn primary_gain_db(&self) -> f64 {
        self.primary_db
    }

    fn set_primary(s: &mut State, gain_db: f64) {
        let options = audio_settings::options(&s.settings, gain_db);
        if let Some(engine) = s.engine.as_ref() {
            let _ = engine.configure_audio(options);
        }
        s.crossfade.primary_db = gain_db;
    }

    fn set_standby(s: &mut State, gain_db: f64) {
        let options = audio_settings::options(&s.settings, gain_db);
        if let Some(engine) = s.crossfade.standby.as_ref() {
            let _ = engine.configure_audio(options);
        }
        s.crossfade.standby_db = gain_db;
    }

    /// Drop the standby engine off the UI thread; shutdown joins its worker.
    fn retire(engine: PlaybackHandle) {
        let _ = std::thread::Builder::new()
            .name("tide-crossfade-retire".into())
            .spawn(move || drop(engine));
    }

    /// Abandon a pending or running crossfade and restore the main engine.
    pub fn cancel(s: &mut State) {
        if s.crossfade.phase == Phase::Idle && s.crossfade.standby.is_none() {
            return;
        }
        s.crossfade.phase = Phase::Idle;
        if let Some(standby) = s.crossfade.standby.take() {
            Self::retire(standby);
        }
        if s.crossfade.primary_db != 0.0 {
            Self::set_primary(s, 0.0);
        }
    }
}

fn start_standby(s: &State) -> Result<PlaybackHandle, String> {
    let lease = s.cookie_lease.as_ref().map(|l| l.path().to_path_buf());
    let index = s
        .saved_connection
        .as_ref()
        .map_or(0, |connection| connection.account_index);
    if s.connection_probe {
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
    let seconds = f64::from(s.settings.crossfade_seconds);
    // The outgoing song reaching its end completes a running mix: at EOF the
    // engine reports itself paused, which must not read as the user pausing.
    if let Phase::Mixing {
        index, generation, ..
    } = s.crossfade.phase.clone()
    {
        let finished = p.generation == s.generation
            && (p.ended || p.duration - p.position <= 0.15)
            && s.crossfade
                .standby
                .as_ref()
                .is_some_and(|engine| engine.snapshot().error.is_none());
        if finished && Some(index) == next_queue_index(s) {
            trace(|| "outgoing song ended; standby takes over".into());
            take_over(s, u, index, generation);
            return true;
        }
    }
    let next = next_queue_index(s).filter(|&index| Some(index) != s.settings.queue.current);
    let next_source = next.and_then(|index| {
        s.settings
            .queue
            .tracks
            .get(index)
            .map(|track| track.source.clone())
    });
    let eligible = seconds > 0.0
        && !s.settings.bit_perfect
        && local_controls_allowed(s)
        && !u.get_jam_active()
        && s.active_playback
        && s.sleep_timer != Some(SleepTimer::EndOfSong)
        && s.engine.is_some()
        && p.generation == s.generation
        && !p.paused
        && !p.loading
        && p.error.is_none()
        && p.duration.is_finite()
        && p.duration >= 2.0 * seconds + PRELOAD_SECONDS
        && next_source.is_some();
    if !eligible {
        if s.crossfade.phase != Phase::Idle {
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
        Crossfade::cancel(s);
        return false;
    }
    let (Some(index), Some(source)) = (next, next_source) else {
        return false;
    };
    let remaining = p.duration - p.position;
    match s.crossfade.phase.clone() {
        Phase::Idle => {
            if remaining > seconds + PRELOAD_SECONDS || remaining <= seconds + 1.0 {
                return false;
            }
            let standby = match s.crossfade.standby.take() {
                Some(engine) => engine,
                None => match start_standby(s) {
                    Ok(engine) => engine,
                    Err(_) => return false,
                },
            };
            let generation = s.generation.wrapping_add(1);
            s.crossfade.standby = Some(standby);
            Crossfade::set_standby(s, SILENT_GAIN_DB);
            let engine = s.crossfade.standby.as_ref().unwrap();
            let volume = f64::from(u.get_volume());
            let loaded = engine
                .send(PlaybackCommand::SetVolume(volume.clamp(0.0, 100.0)))
                .and_then(|()| engine.load_at(source.clone(), generation, 0.0))
                .and_then(|()| engine.send(PlaybackCommand::SetPaused(true)));
            if loaded.is_err() {
                Crossfade::cancel(s);
                return false;
            }
            trace(|| format!("preloading next song at {remaining:.1}s left"));
            s.crossfade.phase = Phase::Preloading {
                index,
                source,
                generation,
            };
            false
        }
        Phase::Preloading {
            index: planned,
            source: planned_source,
            generation,
        } => {
            // The queue, a seek or a skip changed what plays next.
            if planned != index
                || planned_source != source
                || remaining > seconds + PRELOAD_SECONDS + 2.0
            {
                Crossfade::cancel(s);
                return false;
            }
            let Some(standby) = s.crossfade.standby.as_ref().map(|e| e.snapshot()) else {
                Crossfade::cancel(s);
                return false;
            };
            if standby.error.is_some() {
                Crossfade::cancel(s);
                return false;
            }
            let ready =
                standby.generation == generation && !standby.loading && standby.duration > 0.0;
            if remaining <= seconds && ready {
                let engine = s.crossfade.standby.as_ref().unwrap();
                if engine.send(PlaybackCommand::SetPaused(false)).is_err() {
                    Crossfade::cancel(s);
                    return false;
                }
                trace(|| format!("mixing from {remaining:.1}s left"));
                s.crossfade.phase = Phase::Mixing {
                    index,
                    source,
                    generation,
                };
            } else if remaining <= 1.0 {
                trace(|| {
                    format!(
                        "standby not ready in time (loading={} duration={})",
                        standby.loading, standby.duration
                    )
                });
                // Not ready in time: let the ordinary transition happen.
                Crossfade::cancel(s);
            }
            false
        }
        Phase::Mixing {
            index: planned,
            source: planned_source,
            generation,
        } => {
            if planned != index || planned_source != source || remaining > seconds + 1.0 {
                Crossfade::cancel(s);
                return false;
            }
            let standby = s.crossfade.standby.as_ref().map(|e| e.snapshot());
            if standby
                .as_ref()
                .is_none_or(|snapshot| snapshot.error.is_some())
            {
                Crossfade::cancel(s);
                return false;
            }
            let t = 1.0 - (remaining / seconds);
            if t >= 1.0 || p.ended || remaining <= 0.15 {
                take_over(s, u, index, generation);
                return true;
            }
            let (outgoing, incoming) = gains(t);
            if (outgoing - s.crossfade.primary_db).abs() >= GAIN_STEP_DB {
                Crossfade::set_primary(s, outgoing);
            }
            if (incoming - s.crossfade.standby_db).abs() >= GAIN_STEP_DB {
                Crossfade::set_standby(s, incoming);
            }
            false
        }
    }
}

/// Promote the standby engine: it already plays the next song at full gain.
fn take_over(s: &mut State, u: &AppWindow, index: usize, generation: u64) {
    let Some(standby) = s.crossfade.standby.take() else {
        return;
    };
    s.crossfade.phase = Phase::Idle;
    if let Some(old) = s.engine.replace(standby) {
        Crossfade::retire(old);
    }
    s.crossfade.primary_db = s.crossfade.standby_db;
    Crossfade::set_primary(s, 0.0);
    s.generation = generation;
    s.settings.queue.select(index);
    s.settings.playback_bookmark = None;
    s.active_playback = true;
    s.load_requested_at = None;
    refresh(u, s);
    u.set_paused(false);
    save(&s.settings);
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
}
