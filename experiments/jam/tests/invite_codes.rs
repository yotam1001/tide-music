use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use tide_jam_lab::protocol::*;
fn invite() -> Invite {
    Invite {
        endpoint: "https://jam.example.test".into(),
        room: "a".repeat(32),
        invite: Secret::parse(&"b".repeat(64)).unwrap(),
    }
}
#[test]
fn text_invites_roundtrip_only_guest_fields_and_accept_paste_whitespace() {
    let original = invite();
    let code = original.to_code().unwrap();
    assert!(code.starts_with("tide-jam:v1."));
    let decoded = Invite::from_code(&format!("  {}\n", code.as_str())).unwrap();
    assert_eq!(decoded.endpoint, original.endpoint);
    assert_eq!(decoded.room, original.room);
    assert!(decoded.invite.matches(&original.invite));
    let bytes = URL_SAFE_NO_PAD
        .decode(code.strip_prefix("tide-jam:v1.").unwrap())
        .unwrap();
    let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 3);
    assert!(!format!("{decoded:?}").contains(decoded.invite.expose()));
}
#[test]
fn pasted_invites_reject_versions_urls_payload_expansion_and_extra_credentials() {
    for code in [
        "",
        "https://example.test/",
        "tide-jam:v2.e30",
        "tide-jam:v1.!!!!",
        "tide-jam:v1.e30",
        &"x".repeat(2049),
    ] {
        assert!(Invite::from_code(code).is_err());
    }
    for field in ["host", "operator", "cookie", "authorization", "stream_url"] {
        let mut wire = serde_json::to_value(invite()).unwrap();
        wire[field] = "must-never-be-accepted".into();
        let code = format!(
            "tide-jam:v1.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&wire).unwrap())
        );
        assert!(Invite::from_code(&code).is_err());
    }
}
#[test]
fn invitations_cannot_smuggle_plaintext_remote_or_credential_urls() {
    for endpoint in [
        "http://example.test",
        "https://user:password@example.test",
        "https://example.test/path",
        "https://example.test?cookie=never",
        "https://example.test#secret",
        "https://EXAMPLE.test",
    ] {
        let mut wire = serde_json::to_value(invite()).unwrap();
        wire["endpoint"] = endpoint.into();
        let code = format!(
            "tide-jam:v1.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&wire).unwrap())
        );
        assert!(Invite::from_code(&code).is_err());
    }
    let mut local = invite();
    local.endpoint = "http://127.0.0.1:8765".into();
    assert!(Invite::from_code(&local.to_code().unwrap()).is_ok());
}

#[test]
fn host_rosters_are_bounded_unique_and_contain_no_member_capabilities() {
    let good = HostView {
        members: vec![1, 2],
        open: true,
    };
    good.validate().unwrap();
    for members in [vec![0], vec![1, 1], vec![u32::MAX], (1..=9).collect()] {
        assert!(HostView {
            members,
            open: true
        }
        .validate()
        .is_err());
    }
    assert!(serde_json::from_str::<HostView>(
        r#"{"members":[1],"open":true,"member":"not-allowed"}"#
    )
    .is_err());
}
