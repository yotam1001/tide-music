//! Local files use Tide's owned native backend, never Spotify URLs or grants.
use crate::{
    features::Features,
    spotify::{Request, RequestSender},
    state::State,
    GroveWindow,
};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};
use tide_core::PlaybackCommand as Command;
#[derive(Default)]
pub struct Local {
    engine: Option<tide_playback::PlaybackHandle>,
    source: String,
    title: String,
    generation: u64,
    active: bool,
    picking: bool,
    pending: Option<(String, f32, Instant)>,
}
impl Local {
    pub fn active(&self) -> bool {
        self.active || self.pending.is_some()
    }
    pub fn playing_file(&self) -> bool {
        self.active
    }
    fn load(&mut self, ui: &GroveWindow, source: &str, volume: f32) -> Result<(), String> {
        let source = local_path(source)?;
        if self.engine.is_none() {
            self.engine = Some(tide_playback::start()?);
        }
        self.generation = self.generation.wrapping_add(1);
        let engine = self.engine.as_ref().unwrap();
        engine.send(Command::SetVolume(volume.clamp(0.0, 100.0) as f64))?;
        engine.send(Command::Load {
            source: source.clone(),
            generation: self.generation,
        })?;
        self.title = Path::new(&source)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.source = source;
        self.active = true;
        self.pending = None;
        ui.set_local_active(true);
        ui.set_message("Loading local audio…".into());
        self.refresh(ui);
        Ok(())
    }
    pub fn stop(&mut self, ui: &GroveWindow, state: &State) {
        if let Some(engine) = &self.engine {
            let _ = engine.send(Command::Stop);
        }
        self.active = false;
        self.pending = None;
        ui.set_local_active(false);
        ui.set_message("".into());
        crate::refresh(ui, state);
    }
    pub fn transport(&mut self, ui: &GroveWindow, action: &str) -> bool {
        if self.pending.is_some() {
            return true;
        }
        if !self.active {
            return false;
        }
        let Some(engine) = &self.engine else {
            return true;
        };
        let snapshot = engine.snapshot();
        let result = match action {
            "play" if snapshot.ended || snapshot.error.is_some() => {
                let source = self.source.clone();
                self.load(ui, &source, snapshot.volume as f32)
            }
            "play" => engine.send(Command::SetPaused(false)),
            "pause" => engine.send(Command::SetPaused(true)),
            _ => Ok(()),
        };
        if let Err(e) = result {
            ui.set_message(e.into());
        }
        true
    }
    pub fn seek(&self, ui: &GroveWindow, ms: f32) -> bool {
        if self.pending.is_some() {
            return true;
        }
        if !self.active {
            return false;
        }
        if let Some(engine) = &self.engine {
            if let Err(e) = engine.send(Command::Seek(ms.max(0.0) as f64 / 1000.0)) {
                ui.set_message(e.into());
            }
        }
        true
    }
    pub fn volume(&mut self, ui: &GroveWindow, value: f32) -> bool {
        if let Some((_, volume, _)) = self.pending.as_mut() {
            *volume = value.clamp(0.0, 100.0);
            return true;
        }
        if !self.active {
            return false;
        }
        if let Some(engine) = &self.engine {
            if let Err(e) = engine.send(Command::SetVolume(value.clamp(0.0, 100.0) as f64)) {
                ui.set_message(e.into());
            }
        }
        true
    }
    fn refresh(&self, ui: &GroveWindow) {
        if !self.active {
            return;
        }
        let Some(engine) = &self.engine else {
            return;
        };
        let s = engine.snapshot();
        if s.generation != self.generation {
            return;
        }
        ui.set_track_title(
            s.title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&self.title)
                .into(),
        );
        ui.set_artist("Local audio file".into());
        ui.set_playing(!s.paused && !s.loading && !s.ended && s.error.is_none());
        ui.set_can_play(true);
        ui.set_can_pause(true);
        ui.set_can_seek(!s.loading && s.duration > 0.0);
        ui.set_can_next(false);
        ui.set_can_previous(false);
        ui.set_can_shuffle(false);
        ui.set_can_repeat(false);
        ui.set_position((s.position * 1000.0) as f32);
        ui.set_duration((s.duration * 1000.0) as f32);
        ui.set_elapsed(crate::state::clock((s.position * 1000.0) as f32).into());
        ui.set_total(crate::state::clock((s.duration * 1000.0) as f32).into());
        ui.set_volume(s.volume as f32);
        ui.set_status(
            if s.loading {
                "Loading local audio"
            } else if s.error.is_some() {
                "Local audio needs retry"
            } else if s.ended {
                "Local audio finished"
            } else if s.paused {
                "Local audio · paused"
            } else {
                "Playing local audio"
            }
            .into(),
        );
        if let Some(error) = s.error {
            ui.set_message(error.into());
        } else if !s.loading && ui.get_message().starts_with("Loading local audio") {
            ui.set_message("".into());
        }
    }
    fn tick(&mut self, ui: &GroveWindow, state: &State) {
        if let Some((source, volume, since)) = self.pending.clone() {
            if ui.get_jam_connected() {
                self.stop(ui, state);
                ui.set_message("Leave the jam before opening local audio.".into());
                return;
            }
            if !state.playing && !state.loading {
                self.pending = None;
                if let Err(e) = self.load(ui, &source, volume) {
                    self.stop(ui, state);
                    ui.set_message(e.into());
                }
            } else if since.elapsed() > Duration::from_secs(5) {
                self.stop(ui, state);
                ui.set_message(
                    "Spotify did not confirm pause. Pause Spotify and open the local file again."
                        .into(),
                );
            }
        }
        self.refresh(ui);
    }
}
pub fn install(
    ui: &GroveWindow,
    spotify: RequestSender,
    state: Rc<RefCell<State>>,
    prefs: Rc<RefCell<Features>>,
    offline: bool,
) -> (Rc<RefCell<Local>>, slint::Timer) {
    let local = Rc::new(RefCell::new(Local::default()));
    let weak = ui.as_weak();
    let current = local.clone();
    let playback = state.clone();
    ui.on_open_local(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if offline || current.borrow().picking || current.borrow().pending.is_some() {
            return;
        }
        if ui.get_jam_connected() {
            ui.set_message("Leave the jam before opening a local audio file.".into());
            return;
        }
        current.borrow_mut().picking = true;
        let weak = ui.as_weak();
        let local = current.clone();
        let spotify = spotify.clone();
        let prefs = prefs.clone();
        let playback = playback.clone();
        let _ = slint::spawn_local(async move {
            let file = rfd::AsyncFileDialog::new()
                .set_title("Open audio file in Grove")
                .add_filter(
                    "Audio",
                    &[
                        "mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "aiff", "wma",
                    ],
                )
                .pick_file()
                .await;
            local.borrow_mut().picking = false;
            let (Some(ui), Some(file)) = (weak.upgrade(), file) else {
                return;
            };
            if ui.get_jam_connected() {
                ui.set_message("Leave the jam before opening a local audio file.".into());
                return;
            }
            let source = match local_path(&file.path().to_string_lossy()) {
                Ok(source) => source,
                Err(e) => {
                    ui.set_message(e.into());
                    return;
                }
            };
            let volume = prefs.borrow().prefs.volume;
            if playback.borrow().playing || playback.borrow().loading {
                ui.set_track_title(
                    Path::new(&source)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                        .into(),
                );
                ui.set_artist("Local audio file".into());
                ui.set_status("Preparing local audio".into());
                ui.set_can_shuffle(false);
                ui.set_can_repeat(false);
                local.borrow_mut().pending = Some((source, volume, Instant::now()));
                ui.set_local_active(true);
                ui.set_playing(false);
                ui.set_can_play(false);
                ui.set_can_pause(false);
                ui.set_can_seek(false);
                ui.set_can_next(false);
                ui.set_can_previous(false);
                ui.set_message("Pausing Spotify before opening local audio…".into());
                let _ = spotify.send(Request::Command(serde_json::json!({"command":"pause"})));
            } else if let Err(e) = local.borrow_mut().load(&ui, &source, volume) {
                ui.set_message(e.into());
            }
        });
    });
    let weak = ui.as_weak();
    let current = local.clone();
    let playback = state.clone();
    ui.on_stop_local(move || {
        if let Some(ui) = weak.upgrade() {
            current.borrow_mut().stop(&ui, &state.borrow());
        }
    });
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let current = local.clone();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            if let Some(ui) = weak.upgrade() {
                current.borrow_mut().tick(&ui, &playback.borrow());
            }
        },
    );
    (local, timer)
}
fn local_path(input: &str) -> Result<String, String> {
    // Reject network URLs before the shared backend (which also supports web media).
    if !Path::new(input).is_absolute()
        || input.starts_with("\\\\")
        || input.starts_with("//")
        || input.contains("://")
    {
        return Err("Choose an existing local audio file on this computer.".into());
    }
    tide_playback::validate_playback_source(input)
}
/// Explicit muted native probe: owned silence, no picker, provider calls or preferences.
pub fn review(
    ui: &GroveWindow,
    local: Rc<RefCell<Local>>,
    state: Rc<RefCell<State>>,
    output: std::path::PathBuf,
) -> Result<slint::Timer, String> {
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let source = output.join("grove-silent-review.wav");
    let size = 8u32 * 16000 * 2;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(size + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&32000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&size.to_le_bytes());
    wav.resize(44 + size as usize, 0);
    std::fs::write(&source, wav).map_err(|e| e.to_string())?;
    state.borrow_mut().playing = true;
    local.borrow_mut().pending = Some((source.to_string_lossy().to_string(), 0.0, Instant::now()));
    ui.set_local_active(true);
    ui.set_page(8);
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let mut stage = 0;
    let mut since = Instant::now();
    timer.start(slint::TimerMode::Repeated,Duration::from_millis(150),move||{
        let Some(ui)=weak.upgrade()else{return;};assert!(since.elapsed()<Duration::from_secs(20),"Local review timed out at {stage}");
        let snapshot=local.borrow().engine.as_ref().map(|e|e.snapshot()).unwrap_or_default();assert!(snapshot.error.is_none(),"Local playback error: {:?}",snapshot.error);
        let capture=|name:&str|{let p=ui.window().take_snapshot().unwrap();image::RgbaImage::from_raw(p.width(),p.height(),p.as_bytes().to_vec()).unwrap().save(output.join(format!("{name}.png"))).unwrap();};
        let advanced=match stage {
            0=>{assert!(local.borrow().engine.is_none(),"File audio began before Spotify pause confirmation");state.borrow_mut().playing=false;true},
            1 if !snapshot.loading&&!snapshot.paused&&snapshot.position>0.15=>{assert!(ui.get_local_active());assert!(!ui.get_authenticated());assert!(ui.get_can_seek());assert_eq!(snapshot.volume,0.0);ui.invoke_jam_create();assert!(ui.get_jam_message().contains("Return to Spotify"));assert!(!ui.get_jam_busy());ui.invoke_jam_join();assert!(!ui.get_jam_busy());ui.set_jam_message("".into());capture("local-playing");ui.invoke_transport("pause".into());true},
            2 if snapshot.paused=>{ui.invoke_seek(2500.0);true},
            3 if snapshot.paused&&(2.3..2.8).contains(&snapshot.position)=>{ui.invoke_set_volume(0.0);ui.invoke_transport("play".into());true},
            4 if !snapshot.paused&&snapshot.position>2.8=>{capture("local-seek-confirmed");ui.invoke_switch_mini();true},
            5=>{assert!(ui.get_mini());capture("local-mini");ui.invoke_transport("pause".into());true},
            6 if snapshot.paused=>{ui.invoke_stop_local();assert!(!ui.get_local_active());ui.invoke_switch_mini();true},
            7=>{assert_eq!(snapshot.duration,0.0);assert!(snapshot.paused);capture("local-returned");eprintln!("PASS: pause-confirmed handoff, muted native local file, no Spotify login, jam guards, duration/position, pause, seek, resume, volume, mini and return");slint::quit_event_loop().unwrap();true},
            _=>false
        };if advanced{eprintln!("Local review stage {stage} passed");stage+=1;since=Instant::now();}
    });
    Ok(timer)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_mode_never_accepts_network_or_provider_sources() {
        for s in [
            "https://example.com/song.mp3",
            "spotify:track:aaaaaaaaaaaaaaaaaaaaaa",
            "file:///C:/song.mp3",
            "\\\\server\\song.mp3",
            "song.mp3",
        ] {
            assert!(local_path(s).is_err());
        }
    }
}
