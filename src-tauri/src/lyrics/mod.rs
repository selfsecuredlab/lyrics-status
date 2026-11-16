//! Port of src/Sources/BaseSource.ts and src/LyricsFetcher.ts.

pub mod sources;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::http::Http;
use sources::Source;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsWord {
    pub start_time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsLine {
    pub time: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<LyricsWord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SongLyrics {
    pub lines: Vec<LyricsLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedSongLyrics {
    #[serde(default)]
    pub lines: Vec<LyricsLine>,
    pub app_name: String,
}

/// Context the sources need for an outbound request.
pub struct FetchCtx<'a> {
    pub http: &'a Http,
    pub spotify_token: &'a str,
    pub cookies: &'a str,
}

pub struct LyricsFetcher {
    sources: Vec<Source>,
    pub last_fetched_from: String,
    pub last_fetched_for: String,
    cache_dir: PathBuf,
}

impl LyricsFetcher {
    pub fn new(cache_dir: PathBuf) -> LyricsFetcher {
        LyricsFetcher {
            sources: vec![
                Source::Spotify,
                Source::LrcLib,
                Source::NetEase,
                Source::QqMusic,
            ],
            last_fetched_from: "Not fetched".to_string(),
            last_fetched_for: String::new(),
            cache_dir,
        }
    }

    fn cache_path(&self, name: &str, artist: &str) -> PathBuf {
        // Match TS cache key `${name}-${artist}.json`; sanitize path separators
        // so arbitrary track titles cannot escape the cache directory.
        let key = format!("{name}-{artist}.json");
        let safe: String = key
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                other => other,
            })
            .collect();
        self.cache_dir.join(safe)
    }

    fn fetch_cached(&self, name: &str, artist: &str) -> Option<CachedSongLyrics> {
        let path = self.cache_path(name, artist);
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<CachedSongLyrics>(&text).ok()
    }

    fn cache_lyrics(&self, name: &str, artist: &str, lyrics: &SongLyrics, app_name: &str) {
        let _ = std::fs::create_dir_all(&self.cache_dir);
        let cached = CachedSongLyrics {
            lines: lyrics.lines.clone(),
            app_name: app_name.to_string(),
        };
        if let Ok(text) = serde_json::to_string(&cached) {
            let _ = std::fs::write(self.cache_path(name, artist), text);
        }
    }

    pub fn cached_songs_count(&self) -> usize {
        std::fs::read_dir(&self.cache_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|x| x == "json")
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    pub async fn fetch_lyrics(
        &mut self,
        ctx: &FetchCtx<'_>,
        name: &str,
        artist: &str,
    ) -> Option<SongLyrics> {
        self.last_fetched_from = "Not fetched".to_string();

        let cache = self.fetch_cached(name, artist);
        let cache_has_spotify_words = cache
            .as_ref()
            .map(|c| c.app_name == "Spotify" && cached_has_word_timings(c))
            .unwrap_or(false);

        let mut result: Option<SongLyrics> = None;

        for source in &self.sources {
            let source_name = source.app_name();
            self.last_fetched_for = format!("{name}{artist}");

            // Cache hit for this source.
            if let Some(ref c) = cache {
                if c.app_name == source_name {
                    if source_name == "Spotify" && cache_has_spotify_words {
                        result = Some(normalize_lyrics(cached_to_lyrics(c)));
                        self.last_fetched_from = format!("Cache ({})", c.app_name);
                        break;
                    }
                    if source_name != "Spotify" {
                        let normalized = normalize_lyrics(cached_to_lyrics(c));
                        result = Some(if source_name == "LrcLib" {
                            strip_word_timings(normalized)
                        } else {
                            normalized
                        });
                        self.last_fetched_from = format!("Cache ({})", c.app_name);
                        break;
                    }
                }
            }

            match source.get_lyrics(ctx, name, artist).await {
                Ok(raw) => {
                    let fetched = normalize_lyrics(raw);
                    if source_name == "Spotify" {
                        if !has_word_timings(&fetched) {
                            crate::debug::write(
                                "Spotify lyrics missing word timings, falling back to other sources",
                            );
                            continue;
                        }
                        result = Some(fetched);
                    } else if source_name == "LrcLib" {
                        result = Some(strip_word_timings(fetched));
                    } else {
                        result = Some(fetched);
                    }
                    self.last_fetched_from = source_name.to_string();
                    if let Some(ref r) = result {
                        self.cache_lyrics(name, artist, r, source_name);
                    }
                }
                Err(e) => {
                    crate::debug::write(&format!("Lyrics fetch error from {source_name}: {e}"));
                }
            }

            if result.is_some() {
                break;
            }
        }

        if result.is_none() {
            if let Some(c) = cache {
                if c.app_name == "Spotify" && !cached_has_word_timings(&c) {
                    return None;
                }
                self.last_fetched_from = format!("Cache ({})", c.app_name);
                let normalized = normalize_lyrics(cached_to_lyrics(&c));
                result = Some(if c.app_name == "LrcLib" {
                    strip_word_timings(normalized)
                } else {
                    normalized
                });
            }
        }

        result
    }
}

fn cached_to_lyrics(c: &CachedSongLyrics) -> SongLyrics {
    SongLyrics {
        lines: c.lines.clone(),
    }
}

fn has_word_timings(lyrics: &SongLyrics) -> bool {
    lyrics
        .lines
        .iter()
        .any(|l| l.words.as_ref().map(|w| !w.is_empty()).unwrap_or(false))
}

fn cached_has_word_timings(c: &CachedSongLyrics) -> bool {
    c.lines
        .iter()
        .any(|l| l.words.as_ref().map(|w| !w.is_empty()).unwrap_or(false))
}

fn strip_word_timings(mut lyrics: SongLyrics) -> SongLyrics {
    for line in &mut lyrics.lines {
        line.words = None;
    }
    lyrics
}

/// Port of LyricsFetcher.normalizeLyrics — sort by time, fill missing line and
/// word end times from the following entry.
pub fn normalize_lyrics(mut lyrics: SongLyrics) -> SongLyrics {
    lyrics.lines.sort_by_key(|l| l.time);

    let times: Vec<i64> = lyrics.lines.iter().map(|l| l.time).collect();
    let next_times: Vec<Option<i64>> = (0..lyrics.lines.len())
        .map(|i| times.get(i + 1).copied())
        .collect();

    for (i, line) in lyrics.lines.iter_mut().enumerate() {
        let needs_end = line.end_time.map(|e| e <= line.time).unwrap_or(true);
        if needs_end {
            if let Some(nt) = next_times[i] {
                if nt > line.time {
                    line.end_time = Some(nt);
                }
            }
        }

        if let Some(words) = line.words.as_mut() {
            if !words.is_empty() {
                words.retain(|w| !w.text.is_empty());
                words.sort_by_key(|w| w.start_time);

                let starts: Vec<i64> = words.iter().map(|w| w.start_time).collect();
                let line_end = line.end_time;
                for w in 0..words.len() {
                    let word = &words[w];
                    let needs = word.end_time.map(|e| e <= word.start_time).unwrap_or(true);
                    if needs {
                        let next_start = starts.get(w + 1).copied();
                        if let Some(ns) = next_start {
                            if ns > word.start_time {
                                words[w].end_time = Some(ns);
                                continue;
                            }
                        }
                        if let Some(le) = line_end {
                            if le > word.start_time {
                                words[w].end_time = Some(le);
                            }
                        }
                    }
                }
            }
        }
    }

    lyrics
}
