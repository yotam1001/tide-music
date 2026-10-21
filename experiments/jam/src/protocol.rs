use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const MAX_QUEUE: usize = 100;
pub const MAX_BODY: usize = 32 * 1024;
pub const MAX_REQUESTS: usize = 32;

/// Guests may propose a video, never a URL, account token or playback command.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enqueue {
    pub video: VideoId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueRequest {
    pub id: u64,
    pub video: VideoId,
}
pub const MAX_POSITION_MS: u64 = 24 * 60 * 60 * 1000;
pub const STALE_MS: u64 = 6_000;

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(transparent)]
pub struct VideoId(String);

impl VideoId {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.len() != 11
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("Jam accepts only YouTube video IDs, never URLs or files");
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn playback_url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.0)
    }
}
impl<'de> Deserialize<'de> for VideoId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

/// Capability strings are never included in Debug/error output or CLI arguments.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);
impl Secret {
    pub fn random() -> Result<Self, &'static str> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        getrandom::fill(bytes.as_mut()).map_err(|_| "Secure randomness unavailable")?;
        Self::parse(&hex(bytes.as_ref()))
    }
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if !valid_hex(value, 64) {
            return Err("Invalid room capability");
        }
        Ok(Self(Zeroizing::new(value.into())))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn matches(&self, other: &Self) -> bool {
        self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[room capability redacted]")
    }
}
impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}
impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        Self::parse(&value).map_err(de::Error::custom)
    }
}

pub fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn room_id() -> Result<String, &'static str> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Secure randomness unavailable")?;
    Ok(hex(&bytes))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub revision: u64,
    pub play_id: u64,
    pub queue: Vec<VideoId>,
    pub index: usize,
    pub position_ms: u64,
    pub paused: bool,
    pub loading: bool,
}
impl Timeline {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.queue.is_empty()
            || self.queue.len() > MAX_QUEUE
            || self.index >= self.queue.len()
            || self.position_ms > MAX_POSITION_MS
            || self.revision == 0
            || self.play_id == 0
        {
            return Err("Invalid Jam timeline");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub timeline: Option<Timeline>,
    pub age_ms: u64,
    pub members: usize,
}
impl View {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.members > 8 || self.age_ms > 2 * 60 * 60 * 1000 {
            return Err("Invalid Jam room state");
        }
        if let Some(timeline) = &self.timeline {
            timeline.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Created {
    pub room: String,
    pub host: Secret,
    pub invite: Secret,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Joined {
    pub member: Secret,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invite {
    pub endpoint: String,
    pub room: String,
    pub invite: Secret,
}

/// Text-only invitation. Contains only the guest capability, never host/operator
/// authority or account credentials. Bounded before decoding or allocating JSON.
impl Invite {
    pub fn to_code(&self) -> Result<Zeroizing<String>, &'static str> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let endpoint =
            crate::endpoint::Endpoint::parse(&self.endpoint).map_err(|_| "Invalid invite relay")?;
        if !valid_hex(&self.room, 32)
            || endpoint.as_str() != self.endpoint
            || self.endpoint.len() > 512
        {
            return Err("Invalid Jam invite");
        }
        let bytes = Zeroizing::new(serde_json::to_vec(self).map_err(|_| "Invalid Jam invite")?);
        let result = Zeroizing::new(format!("tide-jam:v1.{}", URL_SAFE_NO_PAD.encode(&*bytes)));
        if result.len() > 2048 {
            return Err("Jam invite is too long");
        }
        Ok(result)
    }
    pub fn from_code(code: &str) -> Result<Self, &'static str> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        if code.len() > 2048 {
            return Err("Jam invite is too long");
        }
        let encoded = code
            .trim()
            .strip_prefix("tide-jam:v1.")
            .ok_or("Paste a Tide Jam invite code")?;
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| "Invalid Jam invite code")?,
        );
        let invite: Self = serde_json::from_slice(&bytes).map_err(|_| "Invalid Jam invite code")?;
        // Reuse validation on both paths and require a canonical transport URL.
        invite.to_code()?;
        Ok(invite)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub open: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostView {
    pub members: Vec<u32>,
    pub open: bool,
}
impl HostView {
    pub fn validate(&self) -> Result<(), &'static str> {
        let ids: std::collections::HashSet<_> = self.members.iter().copied().collect();
        if self.members.len() > 8
            || ids.len() != self.members.len()
            || self
                .members
                .iter()
                .any(|id| *id == 0 || *id > i32::MAX as u32)
        {
            return Err("Invalid Jam participants");
        }
        Ok(())
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotatedInvite {
    pub invite: Secret,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListenerId {
    pub id: u32,
}
