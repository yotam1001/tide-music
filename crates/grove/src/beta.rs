//! Beta support data contains fixed fields only: no tokens, account IDs, paths or music history.
use crate::{credentials, state::State, GroveWindow};
use serde_json::{json, Value};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub fn command_line() -> Result<bool, Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("Grove {}", env!("CARGO_PKG_VERSION"));
            Ok(true)
        }
        Some("--export-icon") => {
            std::fs::write(
                args.get(2).ok_or("--export-icon needs an output file")?,
                include_bytes!(env!("GROVE_BUILT_ICON")),
            )?;
            Ok(true)
        }
        Some("--beta-status") => {
            let status = json!({"version":env!("CARGO_PKG_VERSION"),"library_saved":credentials::load::<crate::web_api::SavedLibrary>(credentials::LIBRARY)?.is_some(),"playback_saved":credentials::load::<librespot_core::authentication::Credentials>(credentials::PLAYBACK)?.is_some()});
            std::fs::write(
                args.get(2).ok_or("--beta-status needs an output file")?,
                serde_json::to_vec_pretty(&status)?,
            )?;
            Ok(true)
        }
        Some("--beta-library-check") => {
            let path = args
                .get(2)
                .ok_or("--beta-library-check needs an output file")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let result=runtime.block_on(async {
                let saved=credentials::load::<crate::web_api::SavedLibrary>(credentials::LIBRARY)?.ok_or("Connect Spotify in Grove first.")?;
                let tokens=crate::auth::refresh(&saved.client_id,&saved.refresh_token).await?;
                let mut api=crate::web_api::WebApi::verify(saved.client_id.clone(),tokens,Some(&saved.account_id),|_|{}).await?;
                let mut report=json!({"version":env!("CARGO_PKG_VERSION"),"profile_verified":true,"playlists_read":false,"liked_songs_read":false,"search_read":false,"premium_eligibility_known":api.premium.is_some(),"premium_eligible":api.premium});
                for (key,action) in [("playlists_read",crate::library::Action::Playlists{offset:0}),("liked_songs_read",crate::library::Action::Collection{uri:"liked".into(),offset:0}),("search_read",crate::library::Action::Search{query:"Daft Punk Get Lucky".into(),offset:0})] {
                    match api.feature(action).await { Ok(_)=>report[key]=json!(true),Err(error)=>{report[format!("{key}_error")]=json!(error);} }
                }
                Ok::<_,String>(report)
            });
            let report=result.unwrap_or_else(|error|json!({"version":env!("CARGO_PKG_VERSION"),"profile_verified":false,"error":error}));
            std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
fn support(ui: &GroveWindow, state: &State) -> Value {
    json!({"app":"Grove","version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "library_connected":ui.get_library_ready(),"playback_connected":state.authenticated,"active":state.active,"loading":state.loading,
        "local_file_mode":ui.get_local_active(),"in_jam":ui.get_jam_connected(),"jam_host":ui.get_jam_host(),
        "media_keys":ui.get_media_keys(),"light_theme":ui.get_light_mode(),"compact":ui.get_compact_mode()})
}

pub fn install(ui: &GroveWindow, state: Rc<RefCell<State>>, offline: bool) {
    let weak = ui.as_weak();
    let current = state;
    let picking = Rc::new(std::cell::Cell::new(false));
    ui.on_save_support(move || {
        let Some(ui)=weak.upgrade() else { return; };
        if offline || picking.replace(true) { return; }
        let data=support(&ui,&current.borrow());let weak=ui.as_weak();let picking=picking.clone();
        let _=slint::spawn_local(async move {
            let file=rfd::AsyncFileDialog::new().set_title("Save Grove support info").set_file_name("grove-support.json").add_filter("Support info", &["json"]).save_file().await;
            picking.set(false);
            if let (Some(ui),Some(file))=(weak.upgrade(),file) {
                ui.set_message(match std::fs::write(file.path(),serde_json::to_vec_pretty(&data).unwrap()) {
                    Ok(())=>"Support info saved. It contains no account details, tokens, song names or file paths.".into(),
                    Err(_)=>"Couldn't save support info. Choose another folder.".into(),
                });
            }
        });
    });
}

#[derive(Default)]
pub struct Live {
    library: bool,
    playback: bool,
    progress: bool,
    pause: bool,
    seek: bool,
    resume: bool,
    volume: bool,
    sample: Option<(String, f32, bool, f32, Instant)>,
    streamed: Duration,
}
impl Live {
    fn observe(&mut self, state: &State, library: bool, now: Instant) {
        self.library |= library;
        self.playback |= state.authenticated;
        if state.authenticated
            && state.active
            && !state.loading
            && state.track.uri.starts_with("spotify:track:")
        {
            let position = state.position();
            if let Some((uri, old, playing, volume, then)) = &self.sample {
                if uri == &state.track.uri {
                    let elapsed = now.saturating_duration_since(*then);
                    if state.playing
                        && *playing
                        && position > *old
                        && elapsed < Duration::from_secs(2)
                    {
                        self.streamed += elapsed;
                    }
                    self.progress |= self.streamed >= Duration::from_secs(8);
                    self.pause |= self.progress && !state.playing;
                    self.resume |= self.pause && !*playing && state.playing;
                    self.seek |= self.progress
                        && (position - *old).abs() > elapsed.as_millis() as f32 + 2000.0;
                    self.volume |= (state.volume - *volume).abs() > 2.0;
                } else {
                    self.streamed = Duration::ZERO;
                }
            }
            self.sample = Some((
                state.track.uri.clone(),
                position,
                state.playing,
                state.volume,
                now,
            ));
        }
    }
    fn report(&self) -> Value {
        json!({"version":env!("CARGO_PKG_VERSION"),"library_connected":self.library,"playback_connected":self.playback,
            "stream_progress_observed":self.progress,"pause_observed":self.pause,"seek_observed":self.seek,"resume_observed":self.resume,"volume_observed":self.volume,
            "audible_audio_requires_human_check":true,"playlist_writes_not_exercised":true,"two_device_jams_not_exercised":true})
    }
}
pub fn live_check(
    ui: &GroveWindow,
    state: Rc<RefCell<State>>,
    offline: bool,
) -> Result<Option<slint::Timer>, Box<dyn std::error::Error>> {
    let Some(path) = std::env::args()
        .skip_while(|a| a != "--beta-live-check")
        .nth(1)
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    if offline {
        return Err("Live checks cannot run with offline review modes.".into());
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&Live::default().report())?)?;
    ui.set_page(2);
    ui.set_message("Beta check: connect Spotify and enable playback. Play a song for 10 seconds, pause, seek, resume and change volume.".into());
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    let mut live = Live::default();
    let mut last = Value::Null;
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(250),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            live.observe(&state.borrow(), ui.get_library_ready(), Instant::now());
            let report = live.report();
            if last != report {
                if std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).is_err() {
                    ui.set_message("Couldn't save the beta check report.".into());
                }
                last = report;
            }
        },
    );
    Ok(Some(timer))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_report_does_not_claim_audio_or_include_private_metadata() {
        let live = Live::default();
        let report = live.report();
        assert_eq!(report["stream_progress_observed"], false);
        assert_eq!(report["audible_audio_requires_human_check"], true);
        let text = report.to_string();
        for forbidden in ["refresh_token", "account_id", "spotify:track", "C:\\Users"] {
            assert!(!text.contains(forbidden));
        }
    }
}
