use crate::auth::{self, Tokens};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub struct SavedLibrary {
    pub client_id: String,
    pub refresh_token: String,
    pub account_id: String,
}

impl Drop for SavedLibrary {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.refresh_token.zeroize();
    }
}

pub struct WebApi {
    pub client_id: String,
    pub tokens: Tokens,
    pub account_id: String,
    pub name: String,
    pub premium: Option<bool>,
    http: reqwest::Client,
    retry_at: Instant,
    quota_exhausted: bool,
    pub grant_dirty: bool,
}

impl WebApi {
    pub async fn feature(&mut self, action: crate::library::Action) -> Result<Value, String> {
        use crate::library::Action;
        match action {
            Action::TrackInfo { uri } => {
                let id = entity_id(&uri, "track")?;
                let track = self.get(&format!("/tracks/{id}")).await?;
                Ok(json!({"kind":"track_info","track":track}))
            }
            Action::Search { query, offset } => {
                let query = query.trim();
                if query.is_empty() || query.len() > 300 {
                    return Err("Enter a search of 1–300 characters.".into());
                }
                let parameters = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("q", query)
                    .append_pair("type", "track")
                    .append_pair("limit", "10")
                    .append_pair("offset", &offset.min(1000).to_string())
                    .finish();
                let page = self.get(&format!("/search?{parameters}")).await?;
                Ok(
                    json!({"kind":"search","query":query,"offset":offset,"tracks":page["tracks"]["items"],"has_more":!page["tracks"]["next"].is_null()}),
                )
            }
            Action::Home => {
                if let Ok(page) = self
                    .get("/me/top/tracks?limit=50&time_range=short_term")
                    .await
                {
                    if page["items"]
                        .as_array()
                        .is_some_and(|items| !items.is_empty())
                    {
                        return Ok(
                            json!({"kind":"home","tracks":page["items"],"note":"Recently in your rotation"}),
                        );
                    }
                }
                let page = self.get("/me/tracks?limit=50").await?;
                let tracks: Vec<_> = page["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.get("track").or_else(|| entry.get("item")))
                    .cloned()
                    .collect();
                Ok(json!({"kind":"home","tracks":tracks,"note":"From your liked songs"}))
            }
            Action::Artist { uri } => {
                let id = entity_id(&uri, "artist")?;
                let artist = self.get(&format!("/artists/{id}")).await?;
                let tracks = self.artist_tracks(&id).await?;
                Ok(
                    json!({"kind":"artist","uri":uri,"title":artist["name"],"tracks":tracks,"note":"Tracks from recent releases"}),
                )
            }
            Action::Radio { uri } => {
                let id = entity_id(&uri, "track")?;
                let track = self.get(&format!("/tracks/{id}")).await?;
                let artist = track["artists"][0]["uri"]
                    .as_str()
                    .ok_or("Spotify did not return an artist for this song.")?;
                let artist_id = entity_id(artist, "artist")?;
                let mut tracks = self.artist_tracks(&artist_id).await?;
                tracks.retain(|t| t["uri"].as_str() != Some(&uri));
                Ok(
                    json!({"kind":"radio","tracks":tracks,"seed":uri,"title":"Artist discovery","note":"More from this artist's recent releases. Spotify personalized radio is not exposed to Grove."}),
                )
            }
            Action::Save { uri, saved } => {
                entity_id(&uri, "track")?;
                let parameters = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("uris", &uri)
                    .finish();
                self.mutate(
                    if saved {
                        reqwest::Method::PUT
                    } else {
                        reqwest::Method::DELETE
                    },
                    &format!("/me/library?{parameters}"),
                    None,
                )
                .await?;
                Ok(json!({"kind":"saved","uri":uri,"saved":saved}))
            }
            Action::Contains { uri } => {
                entity_id(&uri, "track")?;
                let parameters = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("uris", &uri)
                    .finish();
                let result = self
                    .get(&format!("/me/library/contains?{parameters}"))
                    .await?;
                Ok(json!({"kind":"saved","uri":uri,"saved":result[0]}))
            }
            Action::Collection { uri, offset } => {
                let path = if uri == "liked" {
                    format!("/me/tracks?limit=50&offset={}", offset.min(100_000))
                } else {
                    format!(
                        "/playlists/{}/items?limit=50&offset={}",
                        entity_id(&uri, "playlist")?,
                        offset.min(100_000)
                    )
                };
                let page = self.get(&path).await?;
                Ok(
                    json!({"kind":"collection_page","uri":uri,"offset":offset,"items":page["items"],"has_more":!page["next"].is_null()}),
                )
            }
            Action::Playlists { offset } => {
                let page = self
                    .get(&format!(
                        "/me/playlists?limit=50&offset={}",
                        offset.min(100_000)
                    ))
                    .await?;
                Ok(
                    json!({"kind":"playlists_page","offset":offset,"items":page["items"],"has_more":!page["next"].is_null(),"account":self.account_id}),
                )
            }
            Action::PlaylistAdd { playlist, uri } => {
                entity_id(&uri, "track")?;
                let id = entity_id(&playlist, "playlist")?;
                self.require_owned_playlist(&id).await?;
                self.mutate(
                    reqwest::Method::POST,
                    &format!("/playlists/{id}/items"),
                    Some(json!({"uris":[uri]})),
                )
                .await?;
                Ok(
                    json!({"kind":"playlist_changed","uri":playlist,"message":"Track added to your playlist."}),
                )
            }
            Action::PlaylistRemove { playlist, uri } => {
                entity_id(&uri, "track")?;
                let id = entity_id(&playlist, "playlist")?;
                let snapshot = self.require_owned_playlist(&id).await?;
                self.mutate(
                    reqwest::Method::DELETE,
                    &format!("/playlists/{id}/items"),
                    Some(json!({"items":[{"uri":uri}],"snapshot_id":snapshot})),
                )
                .await?;
                Ok(
                    json!({"kind":"playlist_changed","uri":playlist,"message":"Song removed from your playlist. Spotify removes matching copies."}),
                )
            }
        }
    }

    async fn require_owned_playlist(&mut self, id: &str) -> Result<String, String> {
        let playlist = self.get(&format!("/playlists/{id}")).await?;
        if playlist["owner"]["id"].as_str() != Some(&self.account_id) {
            return Err("Choose a playlist owned by your Spotify account.".into());
        }
        playlist["snapshot_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or("Spotify did not return the playlist revision. Refresh before editing.".into())
    }
    async fn mutate(
        &mut self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        self.ensure_token().await?;
        if Instant::now() < self.retry_at {
            return Err(throttle_message(
                self.quota_exhausted,
                self.retry_at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .saturating_add(1),
            ));
        }
        let mut request = self
            .http
            .request(method, format!("https://api.spotify.com/v1{path}"))
            .bearer_auth(&self.tokens.access);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "The change could not be confirmed. Refresh before retrying.")?;
        match response.status().as_u16() {
            200..=299 => Ok(Value::Null),
            401 | 403 => Err("Spotify rejected this action. Sign in again to approve library and playlist editing, or check this app's access.".into()),
            429 => Err(self.throttled(response).await),
            _ => Err("Spotify did not confirm the change. Refresh before retrying.".into()),
        }
    }
    pub async fn verify(
        client_id: String,
        tokens: Tokens,
        expected: Option<&str>,
        waiting: impl Fn(u64),
    ) -> Result<Self, String> {
        let mut api = Self {
            client_id,
            tokens,
            account_id: String::new(),
            name: String::new(),
            premium: None,
            http: auth::http_client()?,
            retry_at: Instant::now(),
            quota_exhausted: false,
            grant_dirty: false,
        };
        let mut attempts = 0;
        let mut waited = Duration::ZERO;
        let profile = loop {
            match api.get("/me").await {
                Ok(profile) => break profile,
                Err(error) => {
                    let delay = api.retry_at.saturating_duration_since(Instant::now());
                    if api.quota_exhausted || !verification_retry(attempts, waited, delay) {
                        return Err(error);
                    }
                    waiting(delay.as_secs().saturating_add(1));
                    tokio::time::sleep(delay).await;
                    attempts += 1;
                    waited += delay;
                }
            }
        };
        let id = profile["id"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("Spotify did not return an account ID.")?;
        if expected.is_some_and(|expected| expected != id) {
            return Err(
                "Saved approval belongs to another Spotify account. Sign out and try again.".into(),
            );
        }
        api.account_id = id.to_owned();
        api.name = profile["display_name"].as_str().unwrap_or(id).to_owned();
        // Development-mode profiles omit product. Defer unknown eligibility to
        // the streaming session instead of incorrectly rejecting every account.
        api.premium = profile["product"].as_str().map(|p| p == "premium");
        Ok(api)
    }

    pub fn saved(&self) -> SavedLibrary {
        SavedLibrary {
            client_id: self.client_id.clone(),
            refresh_token: self.tokens.refresh.clone(),
            account_id: self.account_id.clone(),
        }
    }

    async fn artist_tracks(&mut self, id: &str) -> Result<Vec<Value>, String> {
        let albums = self
            .get(&format!(
                "/artists/{id}/albums?include_groups=album,single&limit=3"
            ))
            .await?;
        let mut tracks = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for album in albums["items"].as_array().into_iter().flatten().take(3) {
            let Some(uri) = album["uri"].as_str() else {
                continue;
            };
            let album_id = entity_id(uri, "album")?;
            let page = self
                .get(&format!("/albums/{album_id}/tracks?limit=50"))
                .await?;
            for track in page["items"].as_array().into_iter().flatten() {
                let Some(uri) = track["uri"].as_str() else {
                    continue;
                };
                if !seen.insert(uri.to_owned()) {
                    continue;
                }
                let mut track = track.clone();
                track["album"] = album.clone();
                tracks.push(track);
                if tracks.len() >= 50 {
                    return Ok(tracks);
                }
            }
        }
        Ok(tracks)
    }

    pub async fn ensure_token(&mut self) -> Result<(), String> {
        if self.tokens.expires <= Instant::now() + Duration::from_secs(60) {
            self.tokens = auth::refresh(&self.client_id, &self.tokens.refresh).await?;
            // Persist only on the orchestrator after checking the task generation.
            // A cancelled refresh must never recreate a signed-out credential.
            self.grant_dirty = true;
        }
        Ok(())
    }

    pub async fn get(&mut self, path: &str) -> Result<Value, String> {
        self.ensure_token().await?;
        if Instant::now() < self.retry_at {
            return Err(throttle_message(
                self.quota_exhausted,
                self.retry_at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .saturating_add(1),
            ));
        }
        let response = self
            .http
            .get(format!("https://api.spotify.com/v1{path}"))
            .bearer_auth(&self.tokens.access)
            .send()
            .await
            .map_err(|_| "Could not reach Spotify. Check your connection and retry.".to_string())?;
        if response.status().as_u16() == 429 {
            return Err(self.throttled(response).await);
        }
        self.quota_exhausted = false;
        if response.status().as_u16() == 401 {
            return Err("Spotify rejected library access. Sign in again.".into());
        }
        if response.status().as_u16() == 403 {
            return Err("Spotify denied this content. Sign in again for updated permissions, or check that this Spotify app can access this feature.".into());
        }
        if !response.status().is_success() {
            return Err(format!(
                "Spotify could not load this content (HTTP {}).",
                response.status().as_u16()
            ));
        }
        response
            .json()
            .await
            .map_err(|_| "Spotify returned an unreadable response.".into())
    }

    async fn throttled(&mut self, response: reqwest::Response) -> String {
        let seconds = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30)
            .clamp(1, 3600);
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        self.quota_exhausted = body["error"]["reason"].as_str() == Some("QUOTA_EXCEEDED");
        self.retry_at = Instant::now() + Duration::from_secs(seconds);
        throttle_message(self.quota_exhausted, seconds)
    }

    pub async fn playlists(&mut self) -> Result<Value, String> {
        let mut page = self.get("/me/playlists?limit=50").await?;
        page["account"] = json!(self.account_id);
        Ok(page)
    }

    pub async fn queue(&mut self) -> Result<Value, String> {
        let result = self.get("/me/player/queue").await?;
        let queue: Vec<_> = result["queue"]
            .as_array()
            .into_iter()
            .flatten()
            .take(50)
            .map(|item| json!({"item":entity(item)}))
            .collect();
        Ok(json!({"type":"queue_changed","upcoming":queue}))
    }

    pub async fn enqueue(&mut self, uri: &str, device: &str) -> Result<(), String> {
        self.ensure_token().await?;
        if Instant::now() < self.retry_at {
            return Err(throttle_message(
                self.quota_exhausted,
                self.retry_at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .saturating_add(1),
            ));
        }
        let response = self
            .http
            .post("https://api.spotify.com/v1/me/player/queue")
            .query(&[("uri", uri), ("device_id", device)])
            .bearer_auth(&self.tokens.access)
            .send()
            .await
            .map_err(|_| "Could not add the track to Spotify’s queue.".to_string())?;
        if response.status().as_u16() == 429 {
            return Err(self.throttled(response).await);
        }
        if !response.status().is_success() {
            return Err(
                "Spotify refused the queue update. Make Grove the active device and retry.".into(),
            );
        }
        Ok(())
    }
}

pub fn entity(item: &Value) -> Value {
    let creators: Vec<_> = item["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|artist| json!({"entity":{"uri":artist["uri"],"decorations":{"identity":{"name":artist["name"]}}}}))
        .collect();
    json!({"uri":item["uri"],"artwork_url":item["album"]["images"][0]["url"],"decorations":{"identity":{"name":item["name"]},"creators":creators,"playback":{"duration_ms":item["duration_ms"]}}})
}

fn throttle_message(quota: bool, seconds: u64) -> String {
    if quota {
        "Spotify's app quota is exhausted. A short retry will not fix this. Try later or use a dedicated Spotify app in Account → Connection options.".into()
    } else {
        format!("Spotify's shared app is busy. Try again in {seconds} seconds.")
    }
}

fn verification_retry(attempts: u8, waited: Duration, delay: Duration) -> bool {
    attempts < 3
        && !delay.is_zero()
        && delay <= Duration::from_secs(30)
        && waited + delay <= Duration::from_secs(60)
}

fn entity_id(uri: &str, kind: &str) -> Result<String, String> {
    let id = uri
        .strip_prefix(&format!("spotify:{kind}:"))
        .filter(|id| id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()));
    id.map(str::to_owned)
        .ok_or_else(|| format!("Choose a valid Spotify {kind}."))
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn quota_exhaustion_is_not_presented_as_a_short_cooldown() {
        assert!(throttle_message(true, 8).contains("quota is exhausted"));
        assert!(!throttle_message(true, 8).contains("8 seconds"));
        assert!(throttle_message(false, 8).contains("8 seconds"));
    }
    #[test]
    fn profile_retries_keep_short_server_cooldowns_bounded() {
        assert!(verification_retry(
            0,
            Duration::ZERO,
            Duration::from_secs(13)
        ));
        assert!(verification_retry(
            2,
            Duration::from_secs(26),
            Duration::from_secs(13)
        ));
        assert!(!verification_retry(
            3,
            Duration::from_secs(39),
            Duration::from_secs(13)
        ));
        assert!(!verification_retry(0, Duration::ZERO, Duration::ZERO));
        assert!(!verification_retry(
            0,
            Duration::ZERO,
            Duration::from_secs(3600)
        ));
        assert!(!verification_retry(
            2,
            Duration::from_secs(45),
            Duration::from_secs(20)
        ));
    }
}
