use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Rating {
    Like,
    Dislike,
    Indifferent,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FeedbackRequest {
    Rating {
        video_id: String,
    },
    Rate {
        video_id: String,
        rating: Rating,
    },
    PlaylistAdd {
        video_id: String,
        playlist_id: String,
    },
    PlaylistRemove {
        video_id: String,
        playlist_id: String,
        entry_id: String,
    },
    History {
        video_id: String,
    },
}

pub fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

impl FeedbackRequest {
    pub fn video_id(&self) -> &str {
        match self {
            Self::Rating { video_id }
            | Self::Rate { video_id, .. }
            | Self::PlaylistAdd { video_id, .. }
            | Self::PlaylistRemove { video_id, .. }
            | Self::History { video_id } => video_id,
        }
    }
    pub fn playlist_id(&self) -> Option<&str> {
        match self {
            Self::PlaylistAdd { playlist_id, .. } | Self::PlaylistRemove { playlist_id, .. } => {
                Some(playlist_id)
            }
            _ => None,
        }
    }
    pub fn valid(&self) -> bool {
        self.video_id().len() == 11
            && valid_identifier(self.video_id(), 11)
            && self
                .playlist_id()
                .is_none_or(|id| valid_identifier(id, 160))
            && match self {
                Self::PlaylistRemove { entry_id, .. } => valid_identifier(entry_id, 256),
                _ => true,
            }
    }
    pub fn accepts(&self, result: &FeedbackResult) -> bool {
        if !self.valid()
            || self.video_id() != result.video_id
            || self.playlist_id() != result.playlist_id.as_deref()
        {
            return false;
        }
        match self {
            Self::Rating { .. } => result.operation == FeedbackOperation::Rating,
            Self::Rate { rating, .. } => {
                result.operation == FeedbackOperation::Rate && result.rating == Some(*rating)
            }
            Self::PlaylistAdd { .. } => result.operation == FeedbackOperation::PlaylistAdd,
            Self::PlaylistRemove { entry_id, .. } => {
                result.operation == FeedbackOperation::PlaylistRemove
                    && result.entry_id.as_deref() == Some(entry_id.as_str())
            }
            Self::History { .. } => result.operation == FeedbackOperation::History,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackOperation {
    Rating,
    Rate,
    PlaylistAdd,
    PlaylistRemove,
    History,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackResult {
    pub operation: FeedbackOperation,
    pub video_id: String,
    pub playlist_id: Option<String>,
    pub rating: Option<Rating>,
    #[serde(default)]
    pub entry_id: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistDetails {
    pub id: String,
    pub editable: bool,
    pub entries: Vec<Option<String>>,
}
impl PlaylistDetails {
    pub fn valid_for(&self, id: &str, tracks: usize) -> bool {
        let mut seen = std::collections::HashSet::new();
        self.id == id
            && valid_identifier(id, 160)
            && tracks <= tide_core::MAX_QUEUE_TRACKS
            && self.entries.len() == tracks
            && self
                .entries
                .iter()
                .flatten()
                .all(|entry| valid_identifier(entry, 256) && seen.insert(entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn results_are_bound_to_the_exact_requested_operation_and_track() {
        let request = FeedbackRequest::Rate {
            video_id: "K4DyBUG242c".into(),
            rating: Rating::Like,
        };
        let mut result = FeedbackResult {
            operation: FeedbackOperation::Rate,
            video_id: "K4DyBUG242c".into(),
            playlist_id: None,
            rating: Some(Rating::Like),
            entry_id: None,
        };
        assert!(request.accepts(&result));
        result.video_id = "00000000001".into();
        assert!(!request.accepts(&result));
        result.video_id = request.video_id().into();
        result.operation = FeedbackOperation::History;
        assert!(!request.accepts(&result));
        result.operation = FeedbackOperation::Rate;
        result.rating = Some(Rating::Dislike);
        assert!(!request.accepts(&result));
        assert!(serde_json::from_str::<FeedbackResult>(r#"{"operation":"rate","video_id":"K4DyBUG242c","rating":"LIKE","cookies":"unexpected"}"#).is_err());
    }
    #[test]
    fn playlist_metadata_cannot_authorize_a_different_or_misaligned_list() {
        let mut details = PlaylistDetails {
            id: "PLfixture".into(),
            editable: true,
            entries: vec![Some("entry-one".into()), Some("entry-two".into())],
        };
        assert!(details.valid_for("PLfixture", 2));
        assert!(!details.valid_for("PLother", 2));
        assert!(!details.valid_for("PLfixture", 1));
        details.entries[1] = Some("../file".into());
        assert!(!details.valid_for("PLfixture", 2));
        details.entries[1] = details.entries[0].clone();
        assert!(
            !details.valid_for("PLfixture", 2),
            "Ambiguous entry IDs must not authorize removal"
        );
        assert!(!FeedbackRequest::History {
            video_id: "https://example.test".into()
        }
        .valid());
    }
    #[test]
    fn removing_a_duplicate_song_requires_the_exact_entry_acknowledgement() {
        let request = FeedbackRequest::PlaylistRemove {
            video_id: "00000000001".into(),
            playlist_id: "PLfixture".into(),
            entry_id: "entry-two".into(),
        };
        let mut result = FeedbackResult {
            operation: FeedbackOperation::PlaylistRemove,
            video_id: request.video_id().into(),
            playlist_id: Some("PLfixture".into()),
            rating: None,
            entry_id: Some("entry-one".into()),
        };
        assert!(!request.accepts(&result));
        result.entry_id = None;
        assert!(!request.accepts(&result));
        result.entry_id = Some("entry-two".into());
        assert!(request.accepts(&result));
    }
}
