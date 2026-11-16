//! Port of src/app/DataStore.ts — persistent song history and usage stats.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongHistoryItem {
    pub name: String,
    pub artist: String,
    pub timestamp: i64,
    pub had_lyrics: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotify_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_art: Option<String>,
    #[serde(default = "one")]
    pub play_count: i64,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone)]
pub struct NewHistoryItem {
    pub name: String,
    pub artist: String,
    pub timestamp: i64,
    pub had_lyrics: bool,
    pub spotify_uri: Option<String>,
    pub album_art: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStats {
    #[serde(default)]
    pub songs_today: i64,
    #[serde(default)]
    pub total_songs_played: i64,
    #[serde(default)]
    pub status_changes: i64,
    #[serde(default)]
    pub lyrics_found_count: i64,
    #[serde(default = "today")]
    pub last_reset_date: String,
}

impl Default for AppStats {
    fn default() -> Self {
        AppStats {
            songs_today: 0,
            total_songs_played: 0,
            status_changes: 0,
            lyrics_found_count: 0,
            last_reset_date: today(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedData {
    #[serde(default)]
    pub history: Vec<SongHistoryItem>,
    #[serde(default)]
    pub stats: AppStats,
    #[serde(default = "now_ms", rename = "lastUpdated")]
    pub last_updated: i64,
}

impl Default for PersistedData {
    fn default() -> Self {
        PersistedData {
            history: Vec::new(),
            stats: AppStats::default(),
            last_updated: now_ms(),
        }
    }
}

pub struct DataStore {
    data: PersistedData,
    path: PathBuf,
}

impl DataStore {
    pub fn new(path: PathBuf) -> DataStore {
        let mut store = DataStore {
            data: DataStore::load(&path),
            path,
        };
        store.check_daily_reset();
        store
    }

    fn load(path: &PathBuf) -> PersistedData {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(parsed) = serde_json::from_str::<PersistedData>(&text) {
                return parsed;
            }
        }
        PersistedData::default()
    }

    pub fn save(&mut self) {
        self.data.last_updated = now_ms();
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("json.tmp");
        if let Ok(text) = serde_json::to_string_pretty(&self.data) {
            if std::fs::write(&tmp, text).is_ok() {
                if std::fs::rename(&tmp, &self.path).is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
            }
        }
    }

    fn check_daily_reset(&mut self) {
        let today = today();
        if self.data.stats.last_reset_date != today {
            self.data.stats.songs_today = 0;
            self.data.stats.last_reset_date = today;
            self.save();
        }
    }

    pub fn add_to_history(&mut self, item: NewHistoryItem) {
        let existing = self
            .data
            .history
            .iter()
            .position(|h| h.name == item.name && h.artist == item.artist);

        if let Some(idx) = existing {
            let mut entry = self.data.history.remove(idx);
            entry.play_count += 1;
            entry.timestamp = item.timestamp;
            entry.had_lyrics = item.had_lyrics;
            if item.album_art.is_some() {
                entry.album_art = item.album_art.clone();
            }
            if item.spotify_uri.is_some() {
                entry.spotify_uri = item.spotify_uri.clone();
            }
            self.data.history.insert(0, entry);
        } else {
            self.data.history.insert(
                0,
                SongHistoryItem {
                    name: item.name.clone(),
                    artist: item.artist.clone(),
                    timestamp: item.timestamp,
                    had_lyrics: item.had_lyrics,
                    spotify_uri: item.spotify_uri.clone(),
                    album_art: item.album_art.clone(),
                    play_count: 1,
                },
            );
        }

        if self.data.history.len() > 100 {
            self.data.history.truncate(100);
        }

        self.data.stats.songs_today += 1;
        self.data.stats.total_songs_played += 1;
        if item.had_lyrics {
            self.data.stats.lyrics_found_count += 1;
        }

        self.save();
    }

    pub fn get_history(&self) -> Value {
        serde_json::to_value(&self.data.history).unwrap_or(Value::Array(vec![]))
    }

    pub fn clear_history(&mut self) {
        self.data.history.clear();
        self.save();
    }

    pub fn update_song_lyrics(&mut self, name: &str, artist: &str, had_lyrics: bool) {
        if let Some(entry) = self
            .data
            .history
            .iter_mut()
            .find(|h| h.name == name && h.artist == artist)
        {
            entry.had_lyrics = had_lyrics;
            self.save();
        }
    }

    pub fn get_stats(&mut self) -> Value {
        self.check_daily_reset();
        serde_json::to_value(&self.data.stats).unwrap_or(Value::Null)
    }

    pub fn increment_status_changes(&mut self) {
        self.data.stats.status_changes += 1;
        self.save();
    }

    pub fn force_save(&mut self) {
        self.save();
    }
}
