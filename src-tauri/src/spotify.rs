//! Port of src/SpotifyService.ts. The access token comes from the Spotify
//! account linked in the user's Discord connections. Endpoint results are
//! mapped to the exact shapes the renderer expects.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use crate::datastore::now_ms;
use crate::debug;
use crate::http::Http;
use crate::settings::AllSettings;

const TOKEN_EXPIRY_SKEW_MS: i64 = 60_000;
const MAX_REFRESH_ATTEMPTS: u32 = 3;

pub struct Spotify {
    http: Arc<Http>,
    settings: Arc<Mutex<AllSettings>>,
    token: Mutex<String>,
    expires_at: AtomicI64,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl Spotify {
    pub fn new(http: Arc<Http>, settings: Arc<Mutex<AllSettings>>) -> Spotify {
        Spotify {
            http,
            settings,
            token: Mutex::new(String::new()),
            expires_at: AtomicI64::new(0),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn token(&self) -> String {
        self.token.lock().unwrap().clone()
    }

    fn set_token(&self, token: &str) {
        *self.token.lock().unwrap() = token.to_string();
        let lifetime = 3600_i64 * 1000;
        self.expires_at
            .store(now_ms() + (lifetime - TOKEN_EXPIRY_SKEW_MS).max(30_000), Ordering::SeqCst);
    }

    fn clear_token(&self) {
        self.token.lock().unwrap().clear();
        self.expires_at.store(0, Ordering::SeqCst);
    }

    pub fn is_token_expired(&self) -> bool {
        now_ms() >= self.expires_at.load(Ordering::SeqCst)
    }

    fn discord_token(&self) -> String {
        self.settings.lock().unwrap().credentials.token.clone()
    }

    pub fn cookies(&self) -> String {
        self.settings.lock().unwrap().credentials.cookies.clone()
    }

    /// Fetch the Spotify access token exposed by the linked Discord account.
    pub async fn refresh(&self) -> bool {
        let _guard = self.refresh_lock.lock().await;
        self.perform_refresh().await
    }

    async fn perform_refresh(&self) -> bool {
        let discord_token = self.discord_token();
        if discord_token.is_empty() {
            debug::write("Spotify connection refresh skipped: Discord token is missing");
            return false;
        }

        let connections = match self
            .fetch_discord("https://discord.com/api/v10/users/@me/connections", &discord_token)
            .await
        {
            Some(v) => v,
            None => return false,
        };

        let spotify = connections
            .as_array()
            .and_then(|arr| arr.iter().find(|c| c.get("type").and_then(|t| t.as_str()) == Some("spotify")));
        let spotify = match spotify {
            Some(s) => s,
            None => {
                self.clear_token();
                debug::write("No Spotify account is linked in Discord Connections");
                return false;
            }
        };

        let mut access_token = spotify
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if access_token.is_none() {
            let id = spotify.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let url = format!(
                "https://discord.com/api/v10/users/@me/connections/spotify/{}/access-token",
                urlencoding::encode(id)
            );
            match self.fetch_discord(&url, &discord_token).await {
                Some(v) => {
                    access_token = v.get("access_token").and_then(|x| x.as_str()).map(|s| s.to_string());
                }
                None => {
                    self.clear_token();
                    return false;
                }
            }
        }

        match access_token {
            Some(t) if !t.is_empty() => {
                self.set_token(&t);
                debug::write("Spotify token refreshed from the linked Discord account");
                true
            }
            _ => {
                self.clear_token();
                debug::write("Discord did not return an access token for the linked Spotify account");
                false
            }
        }
    }

    async fn fetch_discord(&self, url: &str, discord_token: &str) -> Option<Value> {
        for attempt in 0..MAX_REFRESH_ATTEMPTS {
            match self
                .http
                .discord_request(
                    reqwest::Method::GET,
                    url,
                    Some(discord_token),
                    None,
                    None,
                    "spotify-connection",
                    MAX_REFRESH_ATTEMPTS,
                    15_000,
                )
                .await
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json::<Value>().await.ok();
                    }
                    let status = resp.status().as_u16();
                    debug::write(&format!("Discord connection request failed: {status}"));
                    let recoverable = status == 429 || status >= 500;
                    if !recoverable || attempt == MAX_REFRESH_ATTEMPTS - 1 {
                        return None;
                    }
                    let backoff = 2_u64.pow(attempt) * 1000;
                    tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                }
                Err(e) => {
                    debug::write(&format!("Discord connection error: {e}"));
                    if attempt == MAX_REFRESH_ATTEMPTS - 1 {
                        return None;
                    }
                    let backoff = 2_u64.pow(attempt) * 1000;
                    tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                }
            }
        }
        None
    }

    async fn ensure_token(&self) -> bool {
        if self.token().is_empty() || self.is_token_expired() {
            if !self.refresh().await {
                debug::write("ensureToken: Token refresh failed");
                return false;
            }
        }
        true
    }

    // --- Low-level request helpers -----------------------------------------

    async fn get_json(&self, url: &str) -> Option<Value> {
        if !self.ensure_token().await {
            return None;
        }
        let resp = self
            .http
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token()))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    }

    /// PUT/POST/DELETE with no body; returns true on 2xx.
    async fn simple(&self, method: reqwest::Method, url: &str) -> bool {
        if !self.ensure_token().await {
            return false;
        }
        match self
            .http
            .client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", self.token()))
            .send()
            .await
        {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }

    async fn put_json(&self, url: &str, body: Value) -> Option<reqwest::Response> {
        if !self.ensure_token().await {
            return None;
        }
        self.http
            .client
            .put(url)
            .header("Authorization", format!("Bearer {}", self.token()))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .ok()
    }

    // --- Playback ----------------------------------------------------------

    pub async fn play_track(&self, track_uri: &str) -> bool {
        if !self.ensure_token().await {
            return false;
        }
        let body = json!({ "uris": [track_uri] });
        let resp = self.put_json("https://api.spotify.com/v1/me/player/play", body.clone()).await;
        if let Some(r) = resp {
            if r.status().as_u16() == 204 {
                return true;
            }
            if r.status().as_u16() == 404 {
                // No active device: pick one and retry.
                if let Some(devices) = self.get_json("https://api.spotify.com/v1/me/player/devices").await {
                    if let Some(arr) = devices.get("devices").and_then(|v| v.as_array()) {
                        if !arr.is_empty() {
                            let device = arr
                                .iter()
                                .find(|d| d.get("is_active").and_then(|v| v.as_bool()) == Some(true))
                                .unwrap_or(&arr[0]);
                            if let Some(id) = device.get("id").and_then(|v| v.as_str()) {
                                let retry_url = format!(
                                    "https://api.spotify.com/v1/me/player/play?device_id={id}"
                                );
                                if let Some(retry) = self.put_json(&retry_url, body).await {
                                    return retry.status().as_u16() == 204;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub async fn search_track(&self, name: &str, artist: &str) -> Option<String> {
        let query = urlencoding::encode(&format!("track:{name} artist:{artist}")).into_owned();
        let url = format!("https://api.spotify.com/v1/search?q={query}&type=track&limit=1");
        let data = self.get_json(&url).await?;
        data.get("tracks")
            .and_then(|t| t.get("items"))
            .and_then(|i| i.as_array())
            .and_then(|a| a.first())
            .and_then(|t| t.get("uri"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub async fn pause(&self) -> bool {
        self.simple(reqwest::Method::PUT, "https://api.spotify.com/v1/me/player/pause").await
    }
    pub async fn resume(&self) -> bool {
        self.simple(reqwest::Method::PUT, "https://api.spotify.com/v1/me/player/play").await
    }
    pub async fn next(&self) -> bool {
        self.simple(reqwest::Method::POST, "https://api.spotify.com/v1/me/player/next").await
    }
    pub async fn previous(&self) -> bool {
        self.simple(reqwest::Method::POST, "https://api.spotify.com/v1/me/player/previous").await
    }

    pub async fn seek(&self, position_ms: i64) -> bool {
        let url = format!("https://api.spotify.com/v1/me/player/seek?position_ms={position_ms}");
        self.simple(reqwest::Method::PUT, &url).await
    }

    pub async fn set_volume(&self, volume_percent: i64) -> bool {
        let v = volume_percent.clamp(0, 100);
        let url = format!("https://api.spotify.com/v1/me/player/volume?volume_percent={v}");
        self.simple(reqwest::Method::PUT, &url).await
    }

    pub async fn transfer_playback(&self, device_id: &str, play: bool) -> bool {
        let body = json!({ "device_ids": [device_id], "play": play });
        match self.put_json("https://api.spotify.com/v1/me/player", body).await {
            Some(r) => r.status().is_success(),
            None => false,
        }
    }

    pub async fn save_track(&self, track_id: &str) -> bool {
        let url = format!("https://api.spotify.com/v1/me/tracks?ids={track_id}");
        self.simple(reqwest::Method::PUT, &url).await
    }
    pub async fn remove_track(&self, track_id: &str) -> bool {
        let url = format!("https://api.spotify.com/v1/me/tracks?ids={track_id}");
        self.simple(reqwest::Method::DELETE, &url).await
    }
    pub async fn is_track_saved(&self, track_id: &str) -> bool {
        let url = format!("https://api.spotify.com/v1/me/tracks/contains?ids={track_id}");
        match self.get_json(&url).await {
            Some(v) => v.as_array().and_then(|a| a.first()).and_then(|x| x.as_bool()) == Some(true),
            None => false,
        }
    }

    // --- Profile / library / browse ---------------------------------------

    pub async fn user_profile(&self) -> Option<Value> {
        let data = self.get_json("https://api.spotify.com/v1/me").await?;
        Some(json!({
            "name": data.get("display_name").cloned().unwrap_or(Value::Null),
            "id": data.get("id").cloned().unwrap_or(Value::Null),
            "image": data.get("images").and_then(|i| i.as_array()).and_then(|a| a.first())
                .and_then(|x| x.get("url")).cloned().unwrap_or(Value::Null),
            "followers": data.get("followers").and_then(|f| f.get("total")).cloned().unwrap_or(Value::Null),
            "product": data.get("product").cloned().unwrap_or(Value::Null),
            "country": data.get("country").cloned().unwrap_or(Value::Null),
        }))
    }

    pub async fn playlists(&self) -> Option<Value> {
        let data = self.get_json("https://api.spotify.com/v1/me/playlists?limit=50").await?;
        data.get("items").cloned()
    }

    pub async fn playlist_tracks(&self, playlist_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/playlists/{playlist_id}/tracks?limit=100");
        let data = self.get_json(&url).await?;
        let items = data.get("items").and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .filter_map(|item| item.get("track").filter(|t| !t.is_null()))
            .map(|t| {
                json!({
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "album": t.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
                    "albumArt": first_image(t.get("album")),
                    "duration": t.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn liked_songs(&self) -> Option<Value> {
        let count = self.get_json("https://api.spotify.com/v1/me/tracks?limit=1").await?;
        let total = count.get("total").cloned().unwrap_or(json!(0));
        let data = self.get_json("https://api.spotify.com/v1/me/tracks?limit=50").await?;
        let items = data.get("items").and_then(|i| i.as_array())?;
        let tracks: Vec<Value> = items
            .iter()
            .filter_map(|item| item.get("track"))
            .map(|t| {
                json!({
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "album": t.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
                    "albumArt": first_image(t.get("album")),
                    "duration": t.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(json!({ "total": total, "tracks": tracks }))
    }

    pub async fn devices(&self) -> Option<Value> {
        let data = self.get_json("https://api.spotify.com/v1/me/player/devices").await?;
        let arr = data.get("devices").and_then(|d| d.as_array())?;
        let mapped: Vec<Value> = arr
            .iter()
            .map(|d| {
                json!({
                    "id": d.get("id").cloned().unwrap_or(Value::Null),
                    "name": d.get("name").cloned().unwrap_or(Value::Null),
                    "type": d.get("type").cloned().unwrap_or(Value::Null),
                    "isActive": d.get("is_active").cloned().unwrap_or(json!(false)),
                    "volumePercent": d.get("volume_percent").cloned().unwrap_or(json!(0)),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn recently_played(&self, limit: i64) -> Option<Value> {
        let limit = limit.clamp(1, 50);
        let url = format!("https://api.spotify.com/v1/me/player/recently-played?limit={limit}");
        let data = self.get_json(&url).await?;
        let items = data.get("items").and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .filter_map(|item| {
                let t = item.get("track")?;
                Some(json!({
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "album": t.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
                    "albumArt": first_image(t.get("album")),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                    "playedAt": item.get("played_at").cloned().unwrap_or(Value::Null),
                }))
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn audio_features(&self, track_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/audio-features/{track_id}");
        let d = self.get_json(&url).await?;
        Some(json!({
            "tempo": d.get("tempo").cloned().unwrap_or(Value::Null),
            "energy": d.get("energy").cloned().unwrap_or(Value::Null),
            "danceability": d.get("danceability").cloned().unwrap_or(Value::Null),
            "valence": d.get("valence").cloned().unwrap_or(Value::Null),
            "acousticness": d.get("acousticness").cloned().unwrap_or(Value::Null),
            "instrumentalness": d.get("instrumentalness").cloned().unwrap_or(Value::Null),
            "loudness": d.get("loudness").cloned().unwrap_or(Value::Null),
            "key": d.get("key").cloned().unwrap_or(Value::Null),
            "mode": d.get("mode").cloned().unwrap_or(Value::Null),
            "timeSignature": d.get("time_signature").cloned().unwrap_or(Value::Null),
        }))
    }

    pub async fn search_tracks(&self, query: &str, limit: i64) -> Option<Value> {
        let limit = limit.clamp(1, 50);
        let url = format!(
            "https://api.spotify.com/v1/search?q={}&type=track&limit={limit}",
            urlencoding::encode(query)
        );
        let data = self.get_json(&url).await?;
        let items = data.get("tracks").and_then(|t| t.get("items")).and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .map(|t| {
                json!({
                    "id": t.get("id").cloned().unwrap_or(Value::Null),
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "album": t.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
                    "albumArt": first_image(t.get("album")),
                    "duration": t.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                    "popularity": t.get("popularity").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn search_artists(&self, query: &str, limit: i64) -> Option<Value> {
        let limit = limit.clamp(1, 50);
        let url = format!(
            "https://api.spotify.com/v1/search?q={}&type=artist&limit={limit}",
            urlencoding::encode(query)
        );
        let data = self.get_json(&url).await?;
        let items = data.get("artists").and_then(|t| t.get("items")).and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items.iter().map(map_artist).collect();
        Some(Value::Array(mapped))
    }

    pub async fn artist(&self, artist_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/artists/{artist_id}");
        let d = self.get_json(&url).await?;
        Some(map_artist(&d))
    }

    pub async fn artist_top_tracks(&self, artist_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/artists/{artist_id}/top-tracks?market=US");
        let data = self.get_json(&url).await?;
        let items = data.get("tracks").and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .map(|t| {
                json!({
                    "id": t.get("id").cloned().unwrap_or(Value::Null),
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "album": t.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
                    "albumArt": first_image(t.get("album")),
                    "duration": t.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                    "popularity": t.get("popularity").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn album(&self, album_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/albums/{album_id}");
        let d = self.get_json(&url).await?;
        Some(json!({
            "id": d.get("id").cloned().unwrap_or(Value::Null),
            "name": d.get("name").cloned().unwrap_or(Value::Null),
            "artist": join_artists(&d),
            "image": first_image(Some(&d)),
            "releaseDate": d.get("release_date").cloned().unwrap_or(Value::Null),
            "totalTracks": d.get("total_tracks").cloned().unwrap_or(Value::Null),
        }))
    }

    pub async fn album_tracks(&self, album_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/albums/{album_id}/tracks?limit=50");
        let data = self.get_json(&url).await?;
        let items = data.get("items").and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .map(|t| {
                json!({
                    "id": t.get("id").cloned().unwrap_or(Value::Null),
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "duration": t.get("duration_ms").cloned().unwrap_or(Value::Null),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                    "trackNumber": t.get("track_number").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn track(&self, track_id: &str) -> Option<Value> {
        let url = format!("https://api.spotify.com/v1/tracks/{track_id}");
        let d = self.get_json(&url).await?;
        Some(json!({
            "id": d.get("id").cloned().unwrap_or(Value::Null),
            "name": d.get("name").cloned().unwrap_or(Value::Null),
            "artist": join_artists(&d),
            "album": d.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
            "albumArt": first_image(d.get("album")),
            "duration": d.get("duration_ms").cloned().unwrap_or(Value::Null),
            "uri": d.get("uri").cloned().unwrap_or(Value::Null),
            "popularity": d.get("popularity").cloned().unwrap_or(Value::Null),
            "previewUrl": d.get("preview_url").cloned().unwrap_or(Value::Null),
        }))
    }

    pub async fn recommendations(
        &self,
        seed_tracks: Option<Vec<String>>,
        seed_artists: Option<Vec<String>>,
        limit: i64,
    ) -> Option<Value> {
        let mut seeds: Vec<String> = Vec::new();
        let track_count = seed_tracks.as_ref().map(|v| v.len()).unwrap_or(0);
        if let Some(tracks) = &seed_tracks {
            for id in tracks.iter().take(5) {
                seeds.push(format!("seed_tracks={id}"));
            }
        }
        if let Some(artists) = &seed_artists {
            let remaining = 5usize.saturating_sub(track_count);
            for id in artists.iter().take(remaining) {
                seeds.push(format!("seed_artists={id}"));
            }
        }
        if seeds.is_empty() {
            return None;
        }
        let limit = limit.clamp(1, 100);
        let url = format!(
            "https://api.spotify.com/v1/recommendations?{}&limit={limit}",
            seeds.join("&")
        );
        let data = self.get_json(&url).await?;
        let items = data.get("tracks").and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .map(|t| {
                json!({
                    "id": t.get("id").cloned().unwrap_or(Value::Null),
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(t),
                    "album": t.get("album").and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null),
                    "albumArt": first_image(t.get("album")),
                    "uri": t.get("uri").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn available_genres(&self) -> Option<Value> {
        let data = self
            .get_json("https://api.spotify.com/v1/recommendations/available-genre-seeds")
            .await?;
        data.get("genres").cloned()
    }

    pub async fn new_releases(&self, limit: i64) -> Option<Value> {
        let limit = limit.clamp(1, 50);
        let url = format!("https://api.spotify.com/v1/browse/new-releases?limit={limit}");
        let data = self.get_json(&url).await?;
        let items = data.get("albums").and_then(|a| a.get("items")).and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .map(|a| {
                json!({
                    "id": a.get("id").cloned().unwrap_or(Value::Null),
                    "name": a.get("name").cloned().unwrap_or(Value::Null),
                    "artist": join_artists(a),
                    "image": first_image(Some(a)),
                    "releaseDate": a.get("release_date").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }

    pub async fn featured_playlists(&self, limit: i64) -> Option<Value> {
        let limit = limit.clamp(1, 50);
        let url = format!("https://api.spotify.com/v1/browse/featured-playlists?limit={limit}");
        let data = self.get_json(&url).await?;
        let items = data.get("playlists").and_then(|p| p.get("items")).and_then(|i| i.as_array())?;
        let mapped: Vec<Value> = items
            .iter()
            .map(|p| {
                json!({
                    "id": p.get("id").cloned().unwrap_or(Value::Null),
                    "name": p.get("name").cloned().unwrap_or(Value::Null),
                    "description": p.get("description").cloned().unwrap_or(Value::Null),
                    "image": first_image(Some(p)),
                    "owner": p.get("owner").and_then(|o| o.get("display_name")).cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Some(Value::Array(mapped))
    }
}

fn join_artists(obj: &Value) -> Value {
    let names: Vec<String> = obj
        .get("artists")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Value::String(names.join(", "))
}

/// First image URL from an object carrying an `images` array (album/artist/playlist).
fn first_image(obj: Option<&Value>) -> Value {
    obj.and_then(|o| o.get("images"))
        .and_then(|i| i.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.get("url"))
        .cloned()
        .unwrap_or(Value::String(String::new()))
}

fn map_artist(a: &Value) -> Value {
    let genres: Vec<Value> = a
        .get("genres")
        .and_then(|g| g.as_array())
        .cloned()
        .unwrap_or_default();
    json!({
        "id": a.get("id").cloned().unwrap_or(Value::Null),
        "name": a.get("name").cloned().unwrap_or(Value::Null),
        "image": first_image(Some(a)),
        "genres": genres,
        "followers": a.get("followers").and_then(|f| f.get("total")).cloned().unwrap_or(Value::Null),
        "popularity": a.get("popularity").cloned().unwrap_or(Value::Null),
    })
}
