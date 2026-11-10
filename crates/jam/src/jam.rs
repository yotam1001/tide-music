//! Host-authoritative jam state. Only metadata is shared; no credentials or audio.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_MEMBERS: usize = 16;
pub const MAX_TRACKS: usize = 100;

pub fn id() -> String {
    oauth2::CsrfToken::new_random().secret().clone()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub host: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub uri: String,
    pub title: String,
    pub member_id: String,
    pub member_name: String,
    pub delivery: Delivery,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Waiting,
    Sending,
    Unconfirmed,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub room_id: String,
    pub revision: u64,
    pub members: Vec<Member>,
    pub queue: Vec<Entry>,
    pub closed: bool,
    pub playback: Option<Playback>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Playback {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub position_ms: u64,
    pub playing: bool,
    pub sampled_at_ms: u64,
}

pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct Room {
    pub snapshot: Snapshot,
    operations: HashSet<(String, String)>,
}

impl Room {
    pub fn new(name: &str) -> Result<Self, String> {
        Ok(Self {
            snapshot: Snapshot {
                room_id: id(),
                revision: 1,
                members: vec![Member {
                    id: id(),
                    name: clean_name(name)?,
                    host: true,
                }],
                queue: vec![],
                closed: false,
                playback: None,
            },
            operations: HashSet::new(),
        })
    }
    pub fn host_id(&self) -> &str {
        &self.snapshot.members[0].id
    }
    pub fn join(&mut self, name: &str) -> Result<String, String> {
        if self.snapshot.closed {
            return Err("This jam has ended.".into());
        }
        if self.snapshot.members.len() >= MAX_MEMBERS {
            return Err("This jam is full (16 people).".into());
        }
        let member = Member {
            id: id(),
            name: clean_name(name)?,
            host: false,
        };
        let id = member.id.clone();
        self.snapshot.members.push(member);
        self.snapshot.revision += 1;
        Ok(id)
    }
    pub fn leave(&mut self, member: &str) {
        if member == self.host_id() {
            return;
        }
        let before = self.snapshot.members.len();
        self.snapshot.members.retain(|m| m.id != member);
        if self.snapshot.members.len() != before {
            self.snapshot.revision += 1;
        }
    }
    fn member(&self, id: &str) -> Result<&Member, String> {
        if self.snapshot.closed {
            return Err("This jam has ended.".into());
        }
        self.snapshot
            .members
            .iter()
            .find(|m| m.id == id)
            .ok_or("Join the jam first.".into())
    }
    pub fn add(
        &mut self,
        member: &str,
        operation: &str,
        input: &str,
        title: &str,
    ) -> Result<(), String> {
        let person = self.member(member)?.clone();
        if self
            .operations
            .contains(&(member.to_owned(), operation.to_owned()))
        {
            return Ok(());
        }
        if operation.is_empty() || operation.len() > 128 {
            return Err("Invalid command ID.".into());
        }
        if self.operations.len() >= 4096 {
            return Err("This jam reached its command limit. Start a new room.".into());
        }
        if self.snapshot.queue.len() >= MAX_TRACKS {
            return Err("The shared queue is full (100 tracks).".into());
        }
        if !person.host
            && self
                .snapshot
                .queue
                .iter()
                .filter(|e| e.member_id == member)
                .count()
                >= 20
        {
            return Err("You can have up to 20 tracks waiting in this jam.".into());
        }
        let uri = crate::track_uri(input)?;
        if !uri.starts_with("spotify:track:") {
            return Err("Add an individual Spotify track to the jam.".into());
        }
        let title = title.trim();
        if title.chars().count() > 120 || title.chars().any(char::is_control) {
            return Err(
                "Track names must be at most 120 characters without control characters.".into(),
            );
        }
        self.snapshot.queue.push(Entry {
            id: id(),
            uri,
            title: if title.is_empty() {
                "Spotify track".into()
            } else {
                title.into()
            },
            member_id: member.into(),
            member_name: person.name,
            delivery: Delivery::Waiting,
        });
        self.operations.insert((member.into(), operation.into()));
        self.snapshot.revision += 1;
        Ok(())
    }
    pub fn remove(&mut self, member: &str, entry: &str) -> Result<(), String> {
        let host = self.member(member)?.host;
        let index = self
            .snapshot
            .queue
            .iter()
            .position(|e| e.id == entry)
            .ok_or("That track is no longer in the shared queue.")?;
        let item = &self.snapshot.queue[index];
        if !host && item.member_id != member {
            return Err("Guests can remove only their own tracks.".into());
        }
        if item.delivery == Delivery::Sending {
            return Err("Wait for Spotify to confirm this track.".into());
        }
        self.snapshot.queue.remove(index);
        self.snapshot.revision += 1;
        Ok(())
    }
    pub fn stage_next(&mut self) -> Result<Entry, String> {
        if self.snapshot.closed {
            return Err("This jam has ended.".into());
        }
        if self
            .snapshot
            .queue
            .iter()
            .any(|e| e.delivery == Delivery::Sending)
        {
            return Err("A track is already being sent to Spotify.".into());
        }
        let first = self
            .snapshot
            .queue
            .first_mut()
            .ok_or("Add a track to the shared queue first.")?;
        if first.delivery == Delivery::Unconfirmed {
            return Err("Check Spotify’s queue, then remove the unconfirmed entry. It will not be retried automatically.".into());
        }
        first.delivery = Delivery::Sending;
        let entry = first.clone();
        self.snapshot.revision += 1;
        Ok(entry)
    }
    pub fn complete(&mut self, entry: &str, success: bool) {
        let Some(index) = self
            .snapshot
            .queue
            .iter()
            .position(|e| e.id == entry && e.delivery == Delivery::Sending)
        else {
            return;
        };
        if success {
            self.snapshot.queue.remove(index);
        } else {
            self.snapshot.queue[index].delivery = Delivery::Unconfirmed;
        }
        self.snapshot.revision += 1;
    }
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        return Err("Enter a display name of 1–32 characters.".into());
    }
    Ok(name.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const TRACK: &str = "spotify:track:2JRo0gjbX4GrCqBYdRohoo";
    #[test]
    fn shared_queue_serializes_contributions_and_retry_is_idempotent() {
        let mut room = Room::new("Host").unwrap();
        let guest = room.join("Guest").unwrap();
        room.add(&guest, "one", TRACK, "First").unwrap();
        room.add(&guest, "one", TRACK, "First").unwrap();
        let host = room.host_id().to_owned();
        room.add(&host, "two", TRACK, "Second").unwrap();
        assert_eq!(room.snapshot.queue.len(), 2);
        assert_eq!(room.snapshot.queue[0].title, "First");
        assert_eq!(room.snapshot.revision, 4);
    }
    #[test]
    fn guest_ownership_and_delivery_confirmation_are_enforced() {
        let mut room = Room::new("Host").unwrap();
        let a = room.join("A").unwrap();
        let b = room.join("B").unwrap();
        room.add(&a, "one", TRACK, "Track").unwrap();
        let entry = room.snapshot.queue[0].id.clone();
        assert!(room.remove(&b, &entry).is_err());
        room.stage_next().unwrap();
        assert!(room.remove(&a, &entry).is_err());
        assert!(room.stage_next().is_err());
        room.complete(&entry, false);
        assert_eq!(room.snapshot.queue.len(), 1);
        assert!(room.stage_next().is_err());
        let host = room.host_id().to_owned();
        room.remove(&host, &entry).unwrap();
        room.add(&b, "two", TRACK, "Next").unwrap();
        let staged = room.stage_next().unwrap();
        room.complete("stale-entry", true);
        assert_eq!(room.snapshot.queue.len(), 1);
        room.complete(&staged.id, true);
        assert!(room.snapshot.queue.is_empty());
    }
    #[test]
    fn bounded_members_and_guest_contributions() {
        let mut room = Room::new("Host").unwrap();
        let guest = room.join("Guest").unwrap();
        for n in 0..20 {
            room.add(&guest, &format!("op{n}"), TRACK, "").unwrap();
        }
        assert!(room.add(&guest, "overflow", TRACK, "").is_err());
        while room.snapshot.members.len() < MAX_MEMBERS {
            room.join("Guest").unwrap();
        }
        assert!(room.join("Overflow").is_err());
        assert!(Room::new("\n").is_err());
        assert!(room
            .add(&guest, "bad", "spotify:album:2JRo0gjbX4GrCqBYdRohoo", "")
            .is_err());
    }
}
