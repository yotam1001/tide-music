//! Opt-in native callback review. Uses loopback rooms, never Spotify credentials.
use crate::{jam_transport as jam, GroveWindow};
use slint::{ComponentHandle, Model};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Review {
    timer: slint::Timer,
    peer: tokio::sync::mpsc::UnboundedSender<jam::Request>,
    task: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Review {
    fn drop(&mut self) {
        self.timer.stop();
        let _ = self.peer.send(jam::Request::Shutdown);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}

pub fn start(ui: &GroveWindow, output: PathBuf) -> Result<Review, std::io::Error> {
    std::fs::create_dir_all(&output)?;
    let (peer, events, task) = jam::worker();
    let sender = peer.clone();
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    let mut stage = 0;
    let mut since = Instant::now();
    let mut peer_invite = String::new();
    let mut peer_joined = false;
    ui.set_page(4);
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(150), move || {
        let Some(ui) = weak.upgrade() else { return; };
        assert!(since.elapsed() < Duration::from_secs(12), "Native review timed out at stage {stage}");
        while let Ok(event) = events.try_recv() {
            match event {
                jam::Event::Joined {invite, ..} => { peer_invite = invite; peer_joined = true; },
                jam::Event::Error(error) => panic!("Native review peer: {error}"),
                _ => {},
            }
        }
        let advanced = match stage {
            0 => {
                ui.set_jam_name("Native review host".into());
                ui.set_jam_address("8.8.8.8".into());
                ui.invoke_jam_create();
                assert!(ui.get_jam_busy());
                true
            },
            1 if !ui.get_jam_busy() => {
                assert!(!ui.get_jam_connected());
                assert!(ui.get_jam_message().contains("private LAN"));
                capture(&ui, &output, "invalid-host");
                ui.set_jam_address("127.0.0.1".into());
                ui.invoke_jam_create();
                ui.invoke_jam_create(); // Repeated activation is ignored while busy.
                true
            },
            2 if ui.get_jam_connected() => {
                assert!(ui.get_jam_host());
                sender.send(jam::Request::Join {name: "Review guest".into(), invite: ui.get_jam_invite().to_string()}).unwrap();
                ui.set_jam_track_link("https://example.com/not-spotify".into());
                ui.invoke_jam_add();
                assert!(ui.get_jam_message().contains("individual Spotify"));
                true
            },
            3 if peer_joined && ui.get_jam_members().contains("Review guest") => {
                ui.set_jam_track_link("spotify:track:2JRo0gjbX4GrCqBYdRohoo".into());
                ui.set_jam_track_title("Host contribution".into());
                ui.invoke_jam_add();
                true
            },
            4 if ui.get_jam_tracks().row_count() == 1 => {
                assert!(ui.get_jam_message().is_empty(), "A successful action retained an old validation error");
                assert!(ui.get_jam_tracks().row_data(0).unwrap().removable);
                sender.send(jam::Request::Add {uri: "spotify:track:3JRo0gjbX4GrCqBYdRohoo".into(), title: "Guest contribution".into()}).unwrap();
                true
            },
            5 if ui.get_jam_tracks().row_count() == 2 => {
                capture(&ui, &output, "host-with-guest");
                ui.invoke_jam_remove(ui.get_jam_tracks().row_data(0).unwrap().id);
                true
            },
            6 if ui.get_jam_tracks().row_count() == 1 => {
                assert_eq!(ui.get_jam_tracks().row_data(0).unwrap().title, "Guest contribution");
                sender.send(jam::Request::Leave).unwrap();
                true
            },
            7 if !ui.get_jam_members().contains("Review guest") => {
                assert_eq!(ui.get_jam_tracks().row_count(), 1, "Leaving must preserve contributions");
                ui.invoke_jam_leave();
                true
            },
            8 if !ui.get_jam_connected() => {
                assert_eq!(ui.get_jam_tracks().row_count(), 0);
                assert!(ui.get_jam_invite().is_empty());
                peer_joined = false;
                sender.send(jam::Request::Host {name: "Review remote host".into(), address: "127.0.0.1".into()}).unwrap();
                true
            },
            9 if peer_joined && !peer_invite.is_empty() => {
                ui.set_jam_name("Native review listener".into());
                ui.set_jam_join_invite(peer_invite.clone().into());
                ui.invoke_jam_join();
                true
            },
            10 if ui.get_jam_connected() => {
                assert!(!ui.get_jam_host());
                assert!(ui.get_jam_following());
                ui.invoke_play_link();
                assert!(ui.get_message().contains("Stop following"));
                ui.set_message("".into());
                sender.send(jam::Request::Add {uri: "spotify:track:2JRo0gjbX4GrCqBYdRohoo".into(), title: "Remote host contribution".into()}).unwrap();
                true
            },
            11 if ui.get_jam_tracks().row_count() == 1 => {
                let row = ui.get_jam_tracks().row_data(0).unwrap();
                assert!(!row.removable);
                ui.invoke_jam_remove(row.id); // Server must also enforce disabled actions.
                true
            },
            12 if ui.get_jam_message().contains("only their own") => {
                ui.window().set_size(slint::LogicalSize::new(700.0,540.0));
                true
            },
            13 => {
                capture(&ui, &output, "guest-permission-error");
                sender.send(jam::Request::Leave).unwrap();
                true
            },
            14 if !ui.get_jam_connected() => {
                assert_eq!(ui.get_jam_tracks().row_count(), 0);
                assert!(ui.get_jam_sync_status().contains("Not in a jam"));
                true
            },
            15 => {
                capture(&ui, &output, "host-disconnected");
                eprintln!("PASS: native callbacks, validation, busy guard, host/join, shared queue, ownership, leave and host disconnect");
                slint::quit_event_loop().unwrap();
                true
            },
            _ => false,
        };
        if advanced { eprintln!("Native review stage {stage} passed"); stage += 1; since = Instant::now(); }
    });
    Ok(Review {
        timer,
        peer,
        task: Some(task),
    })
}

fn capture(ui: &GroveWindow, output: &std::path::Path, name: &str) {
    let pixels = ui.window().take_snapshot().expect("Native snapshot");
    image::RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.as_bytes().to_vec())
        .unwrap()
        .save(output.join(format!("{name}.png")))
        .unwrap();
}
