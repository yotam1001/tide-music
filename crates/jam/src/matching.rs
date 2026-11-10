//! Explicit, session-only recording pairs. A new room never inherits old choices.
use std::collections::HashMap;

#[derive(Default)]
pub struct Pairings {
    room: String,
    pairs: HashMap<String, String>,
}
impl Pairings {
    pub fn set_room(&mut self, room: &str) {
        if self.room != room {
            self.room = room.into();
            self.pairs.clear();
        }
    }
    pub fn get(&self, remote: &str) -> Option<&str> {
        self.pairs.get(remote).map(String::as_str)
    }
    pub fn remember(&mut self, remote: &str, local: &str) -> Result<(), String> {
        let remote = crate::track_uri(remote)?;
        let local = crate::track_uri(local)?;
        if self.room.is_empty() {
            return Err("Join a jam first.".into());
        }
        if self.pairs.len() >= 100 && !self.pairs.contains_key(&remote) {
            self.pairs.clear();
        }
        self.pairs.insert(remote, local);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choices_are_bounded_validated_and_room_scoped() {
        let mut pairs = Pairings::default();
        let youtube = "youtube:abcdefghijk";
        let spotify = "spotify:track:aaaaaaaaaaaaaaaaaaaaaa";
        assert!(pairs.remember(youtube, spotify).is_err());
        pairs.set_room("first");
        pairs.remember(youtube, spotify).unwrap();
        assert_eq!(pairs.get(youtube), Some(spotify));
        assert!(pairs.remember(youtube, "file:///song.mp3").is_err());
        pairs.set_room("second");
        assert!(pairs.get(youtube).is_none());
        for n in 0..150 {
            pairs
                .remember(&format!("youtube:{n:011}"), spotify)
                .unwrap();
        }
        assert!(pairs.pairs.len() <= 100);
    }
}
