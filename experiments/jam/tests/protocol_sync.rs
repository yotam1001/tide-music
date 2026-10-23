use tide_jam_lab::{client::Client, protocol::*, relay::Store, sync::*};

fn timeline() -> Timeline {
    Timeline {
        revision: 1,
        play_id: 1,
        queue: vec![VideoId::parse("abcdefghijk").unwrap()],
        index: 0,
        position_ms: 1_000,
        paused: false,
        loading: false,
    }
}
#[test]
fn maximum_queue_and_counters_fit_the_existing_transport_limit() {
    let mut timeline = timeline();
    timeline.queue = vec![VideoId::parse("abcdefghijk").unwrap(); MAX_QUEUE];
    timeline.index = MAX_QUEUE - 1;
    timeline.revision = u64::MAX;
    timeline.play_id = u64::MAX;
    timeline.position_ms = MAX_POSITION_MS;
    timeline.validate().unwrap();
    let view = View {
        timeline: Some(timeline),
        age_ms: 7_200_000,
        members: 8,
    };
    view.validate().unwrap();
    assert!(serde_json::to_vec(&view).unwrap().len() < MAX_BODY);
}
fn view() -> View {
    View {
        timeline: Some(timeline()),
        age_ms: 100,
        members: 1,
    }
}
fn ready() -> Local {
    Local {
        generation: 1,
        duration_ms: 30_000,
        ready: true,
        paused: true,
        ..Local::default()
    }
}

#[test]
fn joining_an_existing_player_never_reuses_its_load_generation() {
    let mut follower = Follower::starting_after(41);
    let actions = follower.decide(&view(), 0, &ready(), 0).unwrap();
    assert!(matches!(
        actions.first(),
        Some(Action::Load { generation: 42, .. })
    ));
    let mut exhausted = Follower::starting_after(u64::MAX);
    assert!(exhausted.decide(&view(), 0, &ready(), 0).is_err());
}

#[test]
fn rejected_views_cannot_replace_the_displayed_queue_or_track_metadata() {
    let mut follower = Follower::default();
    let mut current = view();
    assert!(follower.accepts_timeline(&current, 0));
    follower.decide(&current, 0, &ready(), 0).unwrap();
    current.timeline.as_mut().unwrap().revision = 2;
    current.timeline.as_mut().unwrap().queue = vec![VideoId::parse("second00001").unwrap()];
    assert!(!follower.accepts_timeline(&current, 2500));
    assert!(!follower
        .decide(&current, 2500, &ready(), 100)
        .unwrap()
        .iter()
        .any(|action| matches!(action, Action::Load { .. })));
    current.age_ms = STALE_MS;
    assert!(!follower.accepts_timeline(&current, 0));
    current.age_ms = 0;
    assert!(follower.accepts_timeline(&current, 0));
    follower.decide(&current, 0, &ready(), 200).unwrap();
    assert!(!follower.accepts_timeline(&view(), 0));
    follower.disconnect();
    assert!(!follower.accepts_timeline(&current, 0));
}

#[test]
fn wire_cannot_supply_urls_paths_cookie_fields_or_non_ascii_ids() {
    for value in [
        "file:///abc",
        "https://x/y",
        "../secret!!",
        "a&x=1!!!!!!",
        "ébcdefghijk",
        "abcdefghij\n",
        "short",
    ] {
        assert!(VideoId::parse(value).is_err());
        assert!(serde_json::from_value::<VideoId>(serde_json::json!(value)).is_err());
    }
    let video = VideoId::parse("Abcd_12-xyz").unwrap();
    assert_eq!(
        video.playback_url(),
        "https://www.youtube.com/watch?v=Abcd_12-xyz"
    );
    let mut value = serde_json::to_value(timeline()).unwrap();
    value["cookie"] = serde_json::json!("unexpected field");
    assert!(serde_json::from_value::<Timeline>(value).is_err());
}
#[test]
fn client_requires_canonical_https_or_literal_loopback_http_endpoints() {
    for address in [
        "https://example.com:443",
        "https://EXAMPLE.com",
        "https://@example.com",
        "https://example.com:0",
        "https://example.com/./",
        "https://example.com/a/../",
        " https://example.com",
        "https://example.com\n",
        "https://example.com\\",
        "https://example.com/%2f",
        "https://example.com?",
        "https://example.com/#",
        "http://2130706433:8000",
        "http://127.1:8000",
        "http://0x7f000001:8000",
        "http://127.0.0.1:08000",
        "http://127.0.0.1:0",
        "http://@127.0.0.1:8000",
        "http://192.168.1.5:8000",
        "http://localhost:8000",
        "http://127.0.0.1:8000/path",
        "http://user@127.0.0.1:8000",
        "http://127.0.0.1:8000/?secret=1",
        "http://127.0.0.1:8000/#fragment",
        "file:///tmp/relay",
    ] {
        assert!(Client::new(address).is_err());
    }
    assert!(Client::new("http://127.0.0.1:8012").is_ok());
    assert!(Client::new("https://jam.example.test").is_ok());
    assert!(Client::new("https://jam.example.test/").is_ok());
    assert!(Client::new("https://jam.example.test:8443").is_ok());
    assert!(Client::new("https://127.0.0.1:8443").is_ok());
    assert!(Client::new("https://jam.example.test")
        .unwrap()
        .create()
        .is_err());
}
#[test]
fn both_sender_and_receiver_validate_bounded_timelines() {
    let mut t = timeline();
    t.queue = vec![t.queue[0].clone(); MAX_QUEUE + 1];
    assert!(t.validate().is_err());
    t = timeline();
    t.index = 1;
    assert!(t.validate().is_err());
    t = timeline();
    t.position_ms = MAX_POSITION_MS + 1;
    assert!(t.validate().is_err());
    t = timeline();
    t.play_id = 0;
    assert!(t.validate().is_err());
    let mut v = view();
    v.members = 9;
    assert!(v.validate().is_err());
    v = view();
    v.age_ms = u64::MAX;
    assert!(v.validate().is_err());
}
#[test]
fn capabilities_are_random_redacted_and_not_interchangeable() {
    let mut store = Store::default();
    let room = store.create(0).unwrap();
    assert!(!room.host.matches(&room.invite));
    assert!(!format!("{room:?}").contains(room.host.expose()));
    let guest = store.join(&room.room, &room.invite, 1).unwrap();
    assert!(store
        .publish(&room.room, &guest.member, timeline(), 2)
        .is_err());
    assert!(store
        .publish(&room.room, &room.invite, timeline(), 2)
        .is_err());
    assert!(store.view(&room.room, &room.invite, 2).is_err());
    store
        .publish(&room.room, &room.host, timeline(), 2)
        .unwrap();
    let state = store.view(&room.room, &guest.member, 102).unwrap();
    let wire = serde_json::to_string(&state).unwrap();
    assert!(!wire.contains(room.host.expose()));
    assert!(!wire.contains(room.invite.expose()));
    assert!(!wire.contains(guest.member.expose()));
    assert!(store.close(&room.room, &guest.member, 103).is_err());
    store.leave(&room.room, &guest.member, 104).unwrap();
    assert!(store.view(&room.room, &guest.member, 105).is_err());
}
#[test]
fn room_expiry_member_expiry_and_replay_checks_are_authoritative() {
    let mut store = Store::default();
    let room = store.create(0).unwrap();
    let guest = store.join(&room.room, &room.invite, 0).unwrap();
    store
        .publish(&room.room, &room.host, timeline(), 0)
        .unwrap();
    assert!(store
        .publish(&room.room, &room.host, timeline(), 200)
        .is_err());
    assert!(store.view(&room.room, &guest.member, 30_000).is_err());
    let guest = store.join(&room.room, &room.invite, 30_001).unwrap();
    assert!(store.view(&room.room, &guest.member, 30_002).is_ok());
    assert!(store
        .publish(&room.room, &room.host, timeline(), 7_200_000)
        .is_err());
    assert!(store.join(&room.room, &room.invite, 7_200_001).is_err());
}
#[test]
fn room_and_member_allocations_are_bounded() {
    let mut store = Store::default();
    let room = store.create(0).unwrap();
    for _ in 0..8 {
        store.join(&room.room, &room.invite, 0).unwrap();
    }
    assert!(store.join(&room.room, &room.invite, 0).is_err());
    for index in 1..32 {
        store.create((index / 16) * 60_000).unwrap();
    }
    assert!(store.create(60_001).is_err());
}
#[test]
fn follower_waits_for_ready_generation_then_catches_up_and_preserves_pause() {
    let mut follower = Follower::default();
    let v = view();
    assert!(matches!(
        &follower.decide(&v, 100, &Local::default(), 0).unwrap()[0],
        Action::Load { generation: 1, .. }
    ));
    assert!(follower
        .decide(&v, 100, &Local::default(), 200)
        .unwrap()
        .is_empty());
    assert_eq!(
        follower.decide(&v, 100, &ready(), 500).unwrap(),
        vec![Action::Seek(1150), Action::Pause(false)]
    );
    let mut paused = view();
    paused.timeline.as_mut().unwrap().revision = 2;
    paused.timeline.as_mut().unwrap().paused = true;
    paused.timeline.as_mut().unwrap().position_ms = 12_000;
    let local = Local {
        paused: false,
        ..ready()
    };
    assert_eq!(
        follower.decide(&paused, 100, &local, 4000).unwrap(),
        vec![Action::Seek(12_000), Action::Pause(true)]
    );
}
#[test]
fn buffering_pause_intent_is_not_lost_and_corrections_are_rate_limited() {
    let mut f = Follower::default();
    f.decide(&view(), 0, &Local::default(), 0).unwrap();
    f.decide(&view(), 0, &ready(), 100).unwrap();
    let local = Local {
        paused: false,
        position_ms: 8000,
        ..ready()
    };
    assert!(f.decide(&view(), 0, &local, 3099).unwrap().is_empty());
    assert!(f
        .decide(&view(), 0, &local, 3100)
        .unwrap()
        .contains(&Action::Seek(1100)));
    let mut paused = view();
    paused.timeline.as_mut().unwrap().paused = true;
    let buffering = Local {
        ready: false,
        ..local
    };
    assert_eq!(
        f.decide(&paused, 0, &buffering, 3200).unwrap(),
        vec![Action::Pause(true)]
    );
}
#[test]
fn same_video_new_play_id_reloads_and_disconnect_never_resurrects() {
    let mut f = Follower::default();
    f.decide(&view(), 0, &Local::default(), 0).unwrap();
    let mut v = view();
    let t = v.timeline.as_mut().unwrap();
    t.revision = 2;
    t.play_id = 2;
    assert!(matches!(
        &f.decide(&v, 0, &ready(), 1000).unwrap()[0],
        Action::Load { generation: 2, .. }
    ));
    assert_eq!(f.disconnect(), vec![Action::Pause(true)]);
    assert!(f.decide(&v, 0, &ready(), 2000).unwrap().is_empty());
}
#[test]
fn stale_hosts_network_loss_and_slow_replies_pause_without_loading_more() {
    let mut f = Follower::default();
    f.decide(&view(), 0, &Local::default(), 0).unwrap();
    let local = Local {
        paused: false,
        ..ready()
    };
    assert!(f.network_failure(&local, 2999).is_empty());
    assert_eq!(f.network_failure(&local, 3000), vec![Action::Pause(true)]);
    let mut stale = view();
    stale.age_ms = STALE_MS;
    assert_eq!(
        f.decide(&stale, 0, &local, 6000).unwrap(),
        vec![Action::Pause(true)]
    );
    assert_eq!(
        f.decide(&view(), 2001, &local, 7000).unwrap(),
        vec![Action::Pause(true)]
    );
}

#[test]
fn skip_schema_rejects_commands_urls_paths_and_unrelated_plays() {
    for input in [
        r#"{"play_id":1,"video":"https://example.test/evil.exe"}"#,
        r#"{"play_id":1,"video":"../evil.exe"}"#,
        r#"{"play_id":1,"video":"abcdefghijk","command":"exec"}"#,
    ] {
        assert!(serde_json::from_str::<Skip>(input).is_err());
    }
    let mut skip = Skip {
        play_id: 1,
        video: VideoId::parse("abcdefghijk").unwrap(),
    };
    assert!(skip.matches(&timeline()));
    skip.play_id = 2;
    assert!(!skip.matches(&timeline()));
    skip.play_id = 0;
    assert!(!skip.matches(&timeline()));
}
