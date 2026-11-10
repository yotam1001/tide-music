//! Spotify feature requests are explicit UI actions with bounded in-flight work.
#[derive(Clone, Debug)]
pub enum Action {
    Search { query: String, offset: u32 },
    Home,
    Artist { uri: String },
    Radio { uri: String },
    Save { uri: String, saved: bool },
    Contains { uri: String },
    TrackInfo { uri: String },
    Collection { uri: String, offset: u32 },
    Playlists { offset: u32 },
    PlaylistAdd { playlist: String, uri: String },
    PlaylistRemove { playlist: String, uri: String },
}
