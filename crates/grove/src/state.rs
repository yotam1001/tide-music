use serde_json::Value;
use std::time::Instant;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub duration: f32,
    #[serde(default)]
    pub artist_uri: String,
    #[serde(default)]
    pub artwork_url: String,
}

impl Track {
    pub fn from_entity(entity: &Value) -> Self {
        if entity["uri"].as_str().unwrap_or_default().is_empty()
            && entity["decorations"]["identity"]["name"].as_str().is_none()
        {
            return Self::default();
        }
        let decorations = &entity["decorations"];
        let artist = decorations["creators"]
            .as_array()
            .map(|creators| {
                creators
                    .iter()
                    .filter_map(|c| c["entity"]["decorations"]["identity"]["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        Self {
            artist_uri: decorations["creators"][0]["entity"]["uri"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            artwork_url: entity["artwork_url"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            uri: entity["uri"].as_str().unwrap_or_default().to_owned(),
            title: decorations["identity"]["name"]
                .as_str()
                .unwrap_or("Untitled track")
                .to_owned(),
            artist,
            duration: decorations["playback"]["duration_ms"]
                .as_f64()
                .unwrap_or_default()
                .max(0.0) as f32,
        }
    }
}

pub struct State {
    pub play_request_id: Option<u64>,
    pub connected: bool,
    pub authenticated: bool,
    pub active: bool,
    pub playing: bool,
    pub loading: bool,
    pub track: Track,
    pub context: String,
    pub queue: Vec<Track>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: bool,
    pub actions: Value,
    position: f32,
    speed: f32,
    anchor: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            play_request_id: None,
            connected: false,
            authenticated: false,
            active: false,
            playing: false,
            loading: false,
            track: Track::default(),
            context: String::new(),
            queue: vec![],
            volume: 35.0,
            shuffle: false,
            repeat: false,
            actions: Value::Null,
            position: 0.0,
            speed: 0.0,
            anchor: Instant::now(),
        }
    }
}

impl State {
    pub fn position(&self) -> f32 {
        if !self.playing || !self.active {
            return self.position.min(self.track.duration);
        }
        (self.position + self.anchor.elapsed().as_millis() as f32 * self.speed)
            .clamp(0.0, self.track.duration)
    }

    pub fn allows(&self, action: &str) -> bool {
        self.authenticated && self.active && self.actions.get(action).is_some()
    }

    fn set_position(&mut self, position: &Value) {
        self.position = position["position_ms"]
            .as_f64()
            .unwrap_or_default()
            .max(0.0) as f32;
        self.speed = position["speed"].as_f64().unwrap_or(0.0).clamp(0.0, 4.0) as f32;
        self.anchor = Instant::now();
    }

    pub fn apply(&mut self, event: &Value) {
        match event["type"].as_str().unwrap_or_default() {
            "loading" => {
                self.position = self.position();
                self.playing = false;
                self.speed = 0.0;
                self.loading = true;
                self.anchor = Instant::now();
            }
            "auth_state" => {
                self.authenticated = event["logged_in"].as_bool().unwrap_or(false);
                self.active = event["is_active"].as_bool().unwrap_or(false);
                if !self.active {
                    self.queue.clear();
                }
                if !self.authenticated {
                    let connected = self.connected;
                    *self = Self::default();
                    self.connected = connected;
                }
            }
            "playback_state" => {
                self.loading = event["loading"].as_bool().unwrap_or(false);
                self.play_request_id = event["play_request_id"].as_u64();
                self.track = Track::from_entity(&event["item"]);
                self.playing = event["status"] == "playing";
                self.active = event["is_active"].as_bool().unwrap_or(false);
                if !self.active {
                    self.queue.clear();
                }
                self.volume = event["volume"].as_f64().unwrap_or(35.0).clamp(0.0, 100.0) as f32;
                self.context = event["context"]["decorations"]["identity"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                self.actions = event["available_actions"].clone();
                self.set_position(&event["position"]);
                self.options(&event["options"]);
            }
            "track_changed" => {
                self.track = Track::from_entity(&event["item"]);
                self.position = 0.0;
                self.anchor = Instant::now();
                self.actions = Value::Null; // refresh action availability for the new item
            }
            "playback_changed" => {
                self.position = self.position();
                self.anchor = Instant::now();
                self.playing = event["status"] == "playing";
            }
            "position_sync" => self.set_position(&event["position"]),
            "volume_changed" => {
                self.volume = event["volume"].as_f64().unwrap_or(35.0).clamp(0.0, 100.0) as f32;
            }
            "device_changed" => {
                self.position = self.position();
                self.anchor = Instant::now();
                self.active = event["is_active"].as_bool().unwrap_or(false);
            }
            "context_changed" => {
                self.context = event["context"]["decorations"]["identity"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
            }
            "options_changed" => self.options(&event["options"]),
            "queue_changed" => {
                self.queue = event["upcoming"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .take(50)
                            .map(|entry| Track::from_entity(&entry["item"]))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            _ => {}
        }
    }

    fn options(&mut self, options: &Value) {
        if let Some(shuffle) = options["shuffle"].as_bool() {
            self.shuffle = shuffle;
        }
        if let Some(repeat) = options["repeat"].as_str() {
            self.repeat = matches!(repeat, "context" | "track");
        }
        if let Some(repeat) = options["repeat_context"].as_bool() {
            self.repeat = repeat;
        }
    }
}

pub fn clock(milliseconds: f32) -> String {
    let seconds = (milliseconds.max(0.0) / 1000.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn loading_freezes_clock_and_partial_option_events_preserve_other_controls() {
        let mut s = State::default();
        s.apply(&json!({"type":"auth_state","logged_in":true,"is_active":true}));
        s.apply(&json!({"type":"playback_state","status":"playing","is_active":true,"item":{"uri":"spotify:track:example","decorations":{"playback":{"duration_ms":200000}}},"position":{"position_ms":42000,"speed":1},"options":{"shuffle":true,"repeat_context":true}}));
        s.apply(&json!({"type":"options_changed","options":{"shuffle":false}}));
        assert!(!s.shuffle);
        assert!(s.repeat);
        s.apply(&json!({"type":"loading"}));
        assert!(s.loading);
        assert!(!s.playing);
        let frozen = s.position();
        assert!(frozen >= 42000.0);
        assert_eq!(s.position(), frozen);
        s.apply(&json!({"type":"options_changed","options":{"repeat_context":false}}));
        assert!(!s.repeat);
        assert!(!s.shuffle);
        s.apply(&json!({"type":"playback_state","loading":false,"status":"playing","is_active":true,"position":{"position_ms":0,"speed":1}}));
        assert!(!s.loading);
        assert!(s.playing);
    }

    #[test]
    fn follows_official_entity_shape_and_real_position() {
        let mut state = State::default();
        state.apply(&json!({"type":"auth_state","logged_in":true,"is_active":true}));
        state.apply(&json!({"type":"playback_state","status":"paused","is_active":true,"item":{"uri":"spotify:track:example","decorations":{"identity":{"name":"Test song"},"creators":[{"entity":{"decorations":{"identity":{"name":"Test artist"}}}}],"playback":{"duration_ms":210000}}},"position":{"position_ms":45000,"speed":0},"available_actions":{"play":{},"seek":{}}}));
        assert_eq!(state.track.title, "Test song");
        assert_eq!(state.track.artist, "Test artist");
        assert_eq!(clock(state.position()), "0:45");
        assert!(state.allows("seek"));
        assert!(!state.allows("skip_next"));
        state.apply(
            &json!({"type":"track_changed","item":{"decorations":{"identity":{"name":"Next"}}}}),
        );
        assert_eq!(state.position(), 0.0);
        assert_eq!(state.track.duration, 0.0);
        assert!(!state.allows("seek"));
        state.apply(&json!({"type":"auth_state","logged_in":false}));
        assert!(state.track.uri.is_empty());
        assert!(!state.authenticated);
    }

    #[test]
    fn volume_only_updates_preserve_the_playback_clock_and_identity() {
        let mut state = State::default();
        state.apply(&json!({"type":"playback_state","play_request_id":7,"status":"playing","is_active":true,
            "item":{"uri":"spotify:track:example","decorations":{"identity":{"name":"Track"},"playback":{"duration_ms":200000}}},
            "position":{"position_ms":45000,"speed":1}}));
        let anchor = state.anchor;
        state.apply(&json!({"type":"volume_changed","volume":55}));
        assert_eq!(state.anchor, anchor);
        assert_eq!(state.position, 45000.0);
        assert_eq!(state.play_request_id, Some(7));
        assert_eq!(state.track.title, "Track");
        assert_eq!(state.volume, 55.0);
        assert!(state.playing);
    }

    #[test]
    fn first_play_request_zero_is_distinct_from_no_playback() {
        let mut state = State::default();
        assert_eq!(state.play_request_id, None);
        state.apply(&json!({"type":"playback_state","play_request_id":0,"status":"playing"}));
        assert_eq!(state.play_request_id, Some(0));
        state.apply(&json!({"type":"playback_state","play_request_id":1,"status":"playing"}));
        assert_eq!(state.play_request_id, Some(1));
    }

    #[test]
    fn queue_events_and_device_loss() {
        let mut state = State::default();
        state.apply(&json!({"type":"queue_changed","upcoming":[{"item":{"uri":"spotify:track:example","decorations":{"identity":{"name":"Next song"}}}}]}));
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue[0].title, "Next song");
        state.apply(&json!({"type":"queue_changed","upcoming":[]}));
        assert!(state.queue.is_empty());
        state.authenticated = true;
        state.active = true;
        state.actions = json!({"play":{}});
        assert!(state.allows("play"));
        state.apply(&json!({"type":"device_changed","is_active":false}));
        assert!(!state.allows("play"));
    }
}
