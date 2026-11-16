//! Shared application state and the engine that drives it. This fuses
//! src/app/LyricsStatusApp.ts, src/PlaybackStateUpdater.ts and the Discord
//! status side of src/StatusChanger.ts into one orchestrator.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::datastore::{now_ms, DataStore, NewHistoryItem};
use crate::debug;
use crate::http::Http;
use crate::lyrics::{FetchCtx, LyricsFetcher};
use crate::playback::PlaybackState;
use crate::settings::AllSettings;
use crate::spotify::Spotify;
use crate::status::StatusChanger;

static PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r" \(.+\)").unwrap());

// Loop cadences. The Spotify poll matches the original 1.5s; the status loop is
// 100ms (vs the original 60fps) — the renderer interpolates between ticks, so
// this is visually identical while cutting IPC traffic ~6x.
const POLL_INTERVAL_MS: u64 = 1500;
const STATUS_INTERVAL_MS: u64 = 100;

#[derive(Clone)]
pub struct AppState {
    pub http: Arc<Http>,
    pub settings: Arc<Mutex<AllSettings>>,
    pub datastore: Arc<Mutex<DataStore>>,
    pub spotify: Arc<Spotify>,
    pub playback: Arc<Mutex<PlaybackState>>,
    pub fetcher: Arc<tokio::sync::Mutex<LyricsFetcher>>,
    pub status: Arc<Mutex<StatusChanger>>,

    pub settings_path: PathBuf,

    running: Arc<AtomicBool>,
    rate_limited_until: Arc<AtomicI64>,
    last_status_text: Arc<Mutex<String>>,
    app: Arc<OnceLock<AppHandle>>,
    tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl AppState {
    pub fn new() -> AppState {
        let base = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("LyricsStatus");
        let _ = std::fs::create_dir_all(&base);
        debug::init(base.clone());

        let settings_path = base.join("settings.json");
        let data_path = base.join("appdata.json");
        let cache_dir = base.join("cache");

        debug::write(&format!("Settings path initialized: {}", settings_path.display()));

        let settings = Arc::new(Mutex::new(AllSettings::load(&settings_path)));
        // Persist defaults on first run, mirroring Settings.load().
        if !settings_path.exists() {
            settings.lock().unwrap().save(&settings_path);
        }

        let http = Arc::new(Http::new());
        let datastore = Arc::new(Mutex::new(DataStore::new(data_path)));
        let spotify = Arc::new(Spotify::new(http.clone(), settings.clone()));
        let playback = Arc::new(Mutex::new(PlaybackState::new()));
        let fetcher = Arc::new(tokio::sync::Mutex::new(LyricsFetcher::new(cache_dir)));
        let status = Arc::new(Mutex::new(StatusChanger::new()));

        AppState {
            http,
            settings,
            datastore,
            spotify,
            playback,
            fetcher,
            status,
            settings_path,
            running: Arc::new(AtomicBool::new(false)),
            rate_limited_until: Arc::new(AtomicI64::new(0)),
            last_status_text: Arc::new(Mutex::new(String::new())),
            app: Arc::new(OnceLock::new()),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn set_app(&self, app: AppHandle) {
        self.http.set_app(app.clone());
        let _ = self.app.set(app);
    }

    fn emit(&self, event: &str, payload: Value) {
        if let Some(app) = self.app.get() {
            let _ = app.emit(event, payload);
        }
    }

    pub fn settings_snapshot(&self) -> AllSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    // -- Lifecycle ----------------------------------------------------------

    pub async fn start(&self) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }

        let spotify_ready = if !self.spotify.token().is_empty() && !self.spotify.is_token_expired() {
            true
        } else {
            self.spotify.refresh().await
        };
        let original_ready = self.capture_original_status().await;

        if !spotify_ready || !original_ready {
            self.running.store(false, Ordering::SeqCst);
            return;
        }

        self.running.store(true, Ordering::SeqCst);

        // Poll loop (playback state + history).
        let poll_state = self.clone();
        let poll = tokio::spawn(async move {
            loop {
                if !poll_state.running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
                if !poll_state.running.load(Ordering::SeqCst) {
                    break;
                }
                poll_state.poll_once().await;
            }
        });

        // Status loop (lyric timing + Discord status).
        let status_state = self.clone();
        let status = tokio::spawn(async move {
            let mut last_update = now_ms();
            loop {
                if !status_state.running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(STATUS_INTERVAL_MS)).await;
                if !status_state.running.load(Ordering::SeqCst) {
                    break;
                }
                last_update = status_state.status_tick(last_update).await;
            }
        });

        let mut tasks = self.tasks.lock().unwrap();
        tasks.push(poll);
        tasks.push(status);
    }

    pub async fn stop(&self) {
        if !self.running.load(Ordering::SeqCst) {
            return;
        }
        self.running.store(false, Ordering::SeqCst);

        let handles: Vec<_> = self.tasks.lock().unwrap().drain(..).collect();
        for h in handles {
            h.abort();
        }

        self.restore_original_status(true).await;
        self.datastore.lock().unwrap().force_save();
    }

    pub async fn toggle(&self) {
        if self.running.load(Ordering::SeqCst) {
            self.stop().await;
        } else {
            self.start().await;
        }
    }

    pub async fn clear_discord_status(&self) {
        self.playback.lock().unwrap().is_playing = false;
        self.restore_original_status(false).await;
    }

    // -- Poll loop body (LyricsStatusApp updateInterval + PlaybackStateUpdater)

    async fn poll_once(&self) {
        let (prev_song_id, prev_has_lyrics, was_playing) = {
            let pb = self.playback.lock().unwrap();
            (pb.song_id.clone(), pb.has_lyrics, pb.is_playing)
        };

        self.update_playback(true).await;

        let (song_id, is_playing, song_name, song_author, has_lyrics) = {
            let pb = self.playback.lock().unwrap();
            (
                pb.song_id.clone(),
                pb.is_playing,
                pb.song_name.clone(),
                pb.song_author.clone(),
                pb.has_lyrics,
            )
        };

        if was_playing && !is_playing {
            self.restore_original_status(false).await;
        }

        if prev_song_id != song_id && !song_id.is_empty() {
            self.status.lock().unwrap().song_changed();
            if let Some(song_info) = self.current_song() {
                let (uri, art) = {
                    let pb = self.playback.lock().unwrap();
                    (pb.song_uri.clone(), pb.album_art.clone())
                };
                self.datastore.lock().unwrap().add_to_history(NewHistoryItem {
                    name: song_name.clone(),
                    artist: song_author.clone(),
                    timestamp: now_ms(),
                    had_lyrics: has_lyrics,
                    spotify_uri: if uri.is_empty() { None } else { Some(uri) },
                    album_art: if art.is_empty() { None } else { Some(art) },
                });
                self.emit("song:changed", song_info);
            }
        } else if prev_song_id == song_id && prev_has_lyrics != has_lyrics {
            self.datastore
                .lock()
                .unwrap()
                .update_song_lyrics(&song_name, &song_author, has_lyrics);
        }
    }

    /// Port of PlaybackStateUpdater.update. `allow_retry` guards the single
    /// post-refresh recursion.
    async fn update_playback(&self, allow_retry: bool) {
        if now_ms() < self.rate_limited_until.load(Ordering::SeqCst) {
            return;
        }
        let round_trip_start = now_ms();
        let token = self.spotify.token();

        let resp = self
            .http
            .client
            .get("https://api.spotify.com/v1/me/player")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .timeout(Duration::from_millis(15_000))
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                debug::write(&format!("PlaybackStateUpdater: Error - {e}"));
                self.playback.lock().unwrap().spotify_connected = false;
                return;
            }
        };

        let code = resp.status().as_u16();

        if code == 204 {
            debug::write("PlaybackStateUpdater: No active playback (204)");
            let mut pb = self.playback.lock().unwrap();
            pb.is_playing = false;
            pb.spotify_connected = true;
            return;
        }

        if code == 429 {
            let wait = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(5);
            self.rate_limited_until
                .store(now_ms() + wait * 1000, Ordering::SeqCst);
            debug::write(&format!("PlaybackStateUpdater: Rate limited (429), waiting {wait}s"));
            self.playback.lock().unwrap().spotify_connected = true;
            return;
        }

        if code == 401 || code == 400 {
            debug::write(&format!("Spotify API returned {code}, attempting token refresh..."));
            let refreshed = self.spotify.refresh().await;
            if !refreshed {
                debug::write("Discord-linked Spotify token refresh failed");
                self.playback.lock().unwrap().spotify_connected = false;
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            if allow_retry {
                Box::pin(self.update_playback(false)).await;
            }
            return;
        }

        if code != 200 {
            return;
        }

        let text = match resp.text().await {
            Ok(t) => t,
            Err(_) => return,
        };
        if text.is_empty() {
            debug::write("PlaybackStateUpdater: Empty response body");
            self.playback.lock().unwrap().is_playing = false;
            return;
        }

        let json: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return,
        };
        let item = json.get("item");
        if item.is_none() || item == Some(&Value::Null) {
            debug::write("PlaybackStateUpdater: No active track (item is null)");
            self.playback.lock().unwrap().is_playing = false;
            return;
        }
        let item = item.unwrap();

        let progress = json.get("progress_ms").and_then(|v| v.as_i64()).unwrap_or(0)
            + (now_ms() - round_trip_start);
        let is_playing = json.get("is_playing").and_then(|v| v.as_bool()).unwrap_or(false);
        let item_id = item
            .get("id")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();

        let changed;
        let (name, author);
        {
            let mut pb = self.playback.lock().unwrap();
            pb.song_progress = progress;
            pb.is_playing = is_playing;
            pb.spotify_connected = true;

            debug::write(&format!(
                "PlaybackStateUpdater: {} - {}",
                item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                if is_playing { "playing" } else { "paused" }
            ));

            changed = pb.song_id != item_id;
            if changed {
                let raw_name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                pb.song_name = PAREN.replace(raw_name, "").to_string();
                pb.song_author = item
                    .get("artists")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|a| a.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                pb.old_song_id = pb.song_id.clone();
                pb.song_id = item_id.clone();
                pb.song_uri = item.get("uri").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let images = item
                    .get("album")
                    .and_then(|a| a.get("images"))
                    .and_then(|i| i.as_array())
                    .cloned()
                    .unwrap_or_default();
                let art = images
                    .iter()
                    .find(|img| img.get("height").and_then(|h| h.as_i64()) == Some(300))
                    .and_then(|img| img.get("url").and_then(|u| u.as_str()))
                    .or_else(|| images.first().and_then(|img| img.get("url").and_then(|u| u.as_str())))
                    .unwrap_or("")
                    .to_string();
                pb.album_art = art;
                pb.song_album = item
                    .get("album")
                    .and_then(|a| a.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                pb.song_duration = item.get("duration_ms").and_then(|v| v.as_i64()).unwrap_or(0);
            }
            name = pb.song_name.clone();
            author = pb.song_author.clone();
        }

        if changed {
            let lyrics = self.fetch_lyrics(&name, &author).await;
            let mut pb = self.playback.lock().unwrap();
            let has = lyrics
                .as_ref()
                .map(|l| !l.lines.is_empty())
                .unwrap_or(false);
            pb.lyrics = lyrics;
            pb.current_line = None;
            pb.has_lyrics = has;
        }

        // Safety refetch when the fetcher's last target drifted from the song.
        let drifted = {
            let fetcher = self.fetcher.lock().await;
            fetcher.last_fetched_for != format!("{name}{author}")
        };
        if drifted {
            let lyrics = self.fetch_lyrics(&name, &author).await;
            let mut pb = self.playback.lock().unwrap();
            let has = lyrics
                .as_ref()
                .map(|l| !l.lines.is_empty())
                .unwrap_or(false);
            pb.has_lyrics = has;
            pb.lyrics = lyrics;
        }
    }

    async fn fetch_lyrics(&self, name: &str, author: &str) -> Option<crate::lyrics::SongLyrics> {
        let spotify_token = self.spotify.token();
        let cookies = self.spotify.cookies();
        let ctx = FetchCtx {
            http: self.http.as_ref(),
            spotify_token: &spotify_token,
            cookies: &cookies,
        };
        let mut fetcher = self.fetcher.lock().await;
        fetcher.fetch_lyrics(&ctx, name, author).await
    }

    // -- Status loop body ---------------------------------------------------

    async fn status_tick(&self, last_update: i64) -> i64 {
        let now = now_ms();
        let settings = self.settings_snapshot();

        // Advance progress and run the lyric decision while holding playback.
        let to_send = {
            let mut pb = self.playback.lock().unwrap();
            if pb.is_playing {
                pb.song_progress += now - last_update;
            }

            let current_status = {
                let st = self.status.lock().unwrap();
                st.current_status_text(&pb, &settings)
            };
            if !current_status.is_empty() {
                let mut last = self.last_status_text.lock().unwrap();
                if current_status != *last {
                    self.datastore.lock().unwrap().increment_status_changes();
                    *last = current_status;
                }
            }

            let decision = self.status.lock().unwrap().decide(&mut *pb, &settings);

            if pb.ended() {
                self.status.lock().unwrap().song_changed();
            }
            decision
        };

        if let Some(text) = to_send {
            self.send_status(&text).await;
        }

        // Emit the lyric view for the renderer.
        let lyrics_info = self.current_lyrics();
        self.emit("lyrics:updated", lyrics_info);

        now
    }

    // -- Discord status PATCH / capture / restore ---------------------------

    async fn send_status(&self, text: &str) {
        let token = self.settings.lock().unwrap().credentials.token.clone();
        let expires = chrono::Utc::now() + chrono::Duration::milliseconds(600_000);
        let body = json!({
            "custom_status": {
                "text": text,
                "expires_at": expires.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            }
        });
        let start = now_ms();
        let resp = self
            .http
            .discord_request(
                reqwest::Method::PATCH,
                "https://discord.com/api/v10/users/@me/settings",
                Some(&token),
                Some("application/json"),
                Some(body.to_string()),
                "discord-status",
                3,
                15_000,
            )
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                self.status.lock().unwrap().record_sent(now_ms() - start);
                self.emit("status:sent", json!({ "text": text }));
            }
            Ok(r) => {
                debug::write(&format!("[Discord API] Error {}", r.status().as_u16()));
            }
            Err(e) => {
                debug::write(&format!("[Discord API] Status update failed: {e}"));
            }
        }
    }

    async fn capture_original_status(&self) -> bool {
        {
            let st = self.status.lock().unwrap();
            if st.original_status_captured {
                return true;
            }
        }
        let token = self.settings.lock().unwrap().credentials.token.clone();
        if token.is_empty() {
            return false;
        }
        let resp = self
            .http
            .discord_request(
                reqwest::Method::GET,
                "https://discord.com/api/v10/users/@me/settings",
                Some(&token),
                None,
                None,
                "discord-status",
                3,
                15_000,
            )
            .await;
        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            _ => return false,
        };
        let json = match resp.json::<Value>().await {
            Ok(v) => v,
            Err(_) => return false,
        };
        let custom = json.get("custom_status").cloned();
        let mut st = self.status.lock().unwrap();
        st.original_custom_status = match custom {
            Some(Value::Null) | None => None,
            Some(v) => Some(v),
        };
        st.original_status_captured = true;
        true
    }

    async fn restore_original_status(&self, reset_capture: bool) {
        let (captured, active) = {
            let st = self.status.lock().unwrap();
            (st.original_status_captured, st.lyrics_status_active)
        };
        if !captured || !active {
            if reset_capture {
                self.status.lock().unwrap().original_status_captured = false;
            }
            return;
        }

        let original = {
            let mut st = self.status.lock().unwrap();
            st.lyrics_status_active = false;
            st.last_sent_status_text.clear();
            st.original_custom_status.clone()
        };
        let token = self.settings.lock().unwrap().credentials.token.clone();
        let body = json!({ "custom_status": original });

        let resp = self
            .http
            .discord_request(
                reqwest::Method::PATCH,
                "https://discord.com/api/v10/users/@me/settings",
                Some(&token),
                Some("application/json"),
                Some(body.to_string()),
                "discord-status",
                3,
                15_000,
            )
            .await;

        if !matches!(resp, Ok(ref r) if r.status().is_success()) {
            self.status.lock().unwrap().lyrics_status_active = true;
        }
        if reset_capture {
            self.status.lock().unwrap().original_status_captured = false;
        }
    }

    // -- Renderer-facing snapshots -----------------------------------------

    pub fn current_song(&self) -> Option<Value> {
        let pb = self.playback.lock().unwrap();
        if pb.song_id.is_empty() {
            return None;
        }
        Some(json!({
            "name": pb.song_name,
            "artist": pb.song_author,
            "progress": pb.song_progress,
            "duration": pb.song_duration,
            "album": pb.song_album,
            "albumArt": pb.album_art,
            "spotifyUri": pb.song_uri,
            "id": pb.song_id,
        }))
    }

    pub fn current_lyrics(&self) -> Value {
        let pb = self.playback.lock().unwrap();
        let lyrics = match &pb.lyrics {
            Some(l) => l,
            None => {
                return json!({
                    "currentLine": "",
                    "nextLine": "",
                    "source": "None",
                    "hasLyrics": false,
                    "lines": [],
                    "currentLineIndex": -1,
                    "progress": pb.song_progress,
                    "isPlaying": pb.is_playing,
                });
            }
        };

        let current_time = pb.current_line.as_ref().map(|l| l.time);
        let current_index = current_time
            .and_then(|t| lyrics.lines.iter().position(|l| l.time == t))
            .map(|i| i as i64)
            .unwrap_or(-1);
        let next_line = if current_index >= 0 && (current_index as usize) < lyrics.lines.len() - 1 {
            lyrics.lines[current_index as usize + 1].text.clone()
        } else {
            String::new()
        };

        let source = {
            // last_fetched_from lives on the fetcher; read non-blocking.
            self.fetcher
                .try_lock()
                .map(|f| f.last_fetched_from.clone())
                .unwrap_or_else(|_| "…".to_string())
        };

        let lines: Vec<Value> = lyrics
            .lines
            .iter()
            .map(|l| {
                json!({
                    "time": l.time,
                    "text": l.text,
                    "endTime": l.end_time,
                    "words": l.words,
                })
            })
            .collect();

        json!({
            "currentLine": pb.current_line.as_ref().map(|l| l.text.clone()).unwrap_or_default(),
            "nextLine": next_line,
            "source": source,
            "hasLyrics": pb.has_lyrics,
            "lines": lines,
            "currentLineIndex": current_index,
            "progress": pb.song_progress,
            "isPlaying": pb.is_playing,
        })
    }

    pub fn app_status(&self) -> Value {
        let is_running = self.running.load(Ordering::SeqCst);
        let is_playing = self.playback.lock().unwrap().is_playing;
        let cached = self
            .fetcher
            .try_lock()
            .map(|f| f.cached_songs_count())
            .unwrap_or(0);
        let stats = self.datastore.lock().unwrap().get_stats();
        json!({
            "isRunning": is_running,
            "isPlaying": is_playing,
            "currentSong": self.current_song(),
            "currentLyrics": self.current_lyrics(),
            "cachedSongs": cached,
            "stats": stats,
        })
    }
}
