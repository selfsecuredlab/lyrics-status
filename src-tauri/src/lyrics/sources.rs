//! Port of the four lyric providers in src/Sources/*. Modeled as an enum
//! instead of class inheritance so dispatch stays allocation-free.

use base64::Engine;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use super::{FetchCtx, LyricsLine, LyricsWord, SongLyrics};

// [mm:ss.xx] with 1-3 digit hundredths. Capture groups: 1=mm, 3=ss, 4=frac.
static LRC_TAG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[(\d\d):((\d\d)\.(\d\d?\d?))]").unwrap());
// LrcLib line + word tags (scaled by digit count).
static LRC_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[(\d{2}):(\d{2})(?:\.(\d{1,3}))?]").unwrap());
static LRC_WORD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<(\d{2}):(\d{2})(?:\.(\d{1,3}))?>").unwrap());
static WS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

#[derive(Clone, Copy)]
pub enum Source {
    Spotify,
    LrcLib,
    NetEase,
    QqMusic,
}

impl Source {
    pub fn app_name(&self) -> &'static str {
        match self {
            Source::Spotify => "Spotify",
            Source::LrcLib => "LrcLib",
            Source::NetEase => "NetEase Music",
            Source::QqMusic => "QQMusic",
        }
    }

    pub async fn get_lyrics(
        &self,
        ctx: &FetchCtx<'_>,
        name: &str,
        artist: &str,
    ) -> Result<SongLyrics, String> {
        match self {
            Source::Spotify => spotify_lyrics(ctx).await,
            Source::LrcLib => lrclib_lyrics(ctx, name, artist).await,
            Source::NetEase => netease_lyrics(ctx, name, artist).await,
            Source::QqMusic => qq_lyrics(ctx, name, artist).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Spotify (open.spotify.com color-lyrics)
// ---------------------------------------------------------------------------

async fn spotify_request(ctx: &FetchCtx<'_>, url: &str) -> Result<Value, String> {
    let resp = ctx
        .http
        .client
        .get(url)
        .header("accept", "application/json")
        .header("accept-language", "ru")
        .header("app-platform", "WebPlayer")
        .header("authorization", format!("Bearer {}", ctx.spotify_token))
        .header("spotify-app-version", "1.2.40.176.g6d58cb73")
        .header("Cookie", ctx.cookies)
        .header("referer", "https://open.spotify.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

async fn spotify_lyrics(ctx: &FetchCtx<'_>) -> Result<SongLyrics, String> {
    let player = spotify_request(ctx, "https://api.spotify.com/v1/me/player").await?;
    let id = player
        .get("item")
        .and_then(|i| i.get("id"))
        .ok_or("Song not found")?;
    let song_id = match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let url = format!(
        "https://spclient.wg.spotify.com/color-lyrics/v2/track/{song_id}?format=json&vocalRemoval=false&market=from_token"
    );
    let json = spotify_request(ctx, &url).await?;
    let lyrics = json.get("lyrics").ok_or("Lyrics not found")?;

    let show_upsell = lyrics
        .get("showUpsell")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sync_type = lyrics
        .get("syncType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if show_upsell || sync_type == "UNSYNCED" {
        return Err("Lyrics not found".to_string());
    }

    let empty = vec![];
    let lines = lyrics
        .get("lines")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    let mut out = SongLyrics::default();
    for line in lines {
        let line_start = line
            .get("startTimeMs")
            .map(num_from_value)
            .unwrap_or(0);
        let line_end = line.get("endTimeMs").map(num_from_value);
        let words = parse_spotify_words(line, line_start);
        out.lines.push(LyricsLine {
            time: line_start,
            text: line
                .get("words")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            end_time: line_end,
            words,
        });
    }
    Ok(out)
}

fn parse_spotify_words(line: &Value, line_start: i64) -> Option<Vec<LyricsWord>> {
    let syllables = line.get("syllables").and_then(|v| v.as_array())?;
    if syllables.is_empty() {
        return None;
    }
    let mut words = Vec::new();
    for syl in syllables {
        let text = syl
            .get("text")
            .or_else(|| syl.get("word"))
            .map(value_to_trimmed_string)
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let raw_start = syl
            .get("startTimeMs")
            .or_else(|| syl.get("startTime"))
            .map(num_from_value);
        let raw_start = match raw_start {
            Some(s) => s,
            None => continue,
        };
        let raw_end = syl
            .get("endTimeMs")
            .or_else(|| syl.get("endTime"))
            .map(num_from_value);

        let start_time = if raw_start < line_start - 1000 && line_start > 0 {
            line_start + raw_start
        } else {
            raw_start
        };
        let end_time = raw_end.map(|e| {
            if e < line_start - 1000 && line_start > 0 {
                line_start + e
            } else {
                e
            }
        });
        words.push(LyricsWord {
            start_time,
            end_time,
            text,
        });
    }
    if words.is_empty() {
        None
    } else {
        Some(words)
    }
}

// ---------------------------------------------------------------------------
// LrcLib
// ---------------------------------------------------------------------------

async fn lrclib_lyrics(
    ctx: &FetchCtx<'_>,
    name: &str,
    artist: &str,
) -> Result<SongLyrics, String> {
    let url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        urlencoding::encode(name),
        urlencoding::encode(artist)
    );
    let resp = ctx.http.client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Request failed: {}", resp.status().as_u16()));
    }
    let json = resp.json::<Value>().await.map_err(|e| e.to_string())?;
    let synced = json
        .get("syncedLyrics")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if synced.trim().is_empty() {
        return Err("No synced lyrics found".to_string());
    }
    Ok(parse_lrclib(synced))
}

fn scaled_ms(frac: Option<&str>) -> i64 {
    match frac {
        Some(f) if !f.is_empty() => {
            let raw: i64 = f.parse().unwrap_or(0);
            match f.len() {
                3 => raw,
                2 => raw * 10,
                _ => raw * 100,
            }
        }
        _ => 0,
    }
}

fn parse_lrclib(lyrics: &str) -> SongLyrics {
    let mut out = SongLyrics::default();
    for line in lyrics.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let mut timestamps = Vec::new();
        for cap in LRC_LINE.captures_iter(line) {
            let min: i64 = cap.get(1).unwrap().as_str().parse().unwrap_or(0);
            let sec: i64 = cap.get(2).unwrap().as_str().parse().unwrap_or(0);
            let ms = scaled_ms(cap.get(3).map(|m| m.as_str()));
            timestamps.push((min * 60 + sec) * 1000 + ms);
        }

        let line_with_word_tags = LRC_LINE.replace_all(line, "").trim().to_string();
        let words = parse_lrclib_words(&line_with_word_tags);
        let text = WS
            .replace_all(&LRC_WORD.replace_all(&line_with_word_tags, ""), " ")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }

        let times = if timestamps.is_empty() {
            vec![0]
        } else {
            timestamps
        };
        for time in times {
            out.lines.push(LyricsLine {
                time,
                text: text.clone(),
                end_time: None,
                words: words.clone(),
            });
        }
    }
    out.lines.sort_by_key(|l| l.time);
    out
}

fn parse_lrclib_words(line: &str) -> Option<Vec<LyricsWord>> {
    let matches: Vec<regex::Captures> = LRC_WORD.captures_iter(line).collect();
    if matches.is_empty() {
        return None;
    }
    let mut words = Vec::new();
    for i in 0..matches.len() {
        let cap = &matches[i];
        let min: i64 = cap.get(1).unwrap().as_str().parse().unwrap_or(0);
        let sec: i64 = cap.get(2).unwrap().as_str().parse().unwrap_or(0);
        let ms = scaled_ms(cap.get(3).map(|m| m.as_str()));
        let start_time = (min * 60 + sec) * 1000 + ms;

        let m0 = cap.get(0).unwrap();
        let start_index = m0.end();
        let end_index = if i + 1 < matches.len() {
            matches[i + 1].get(0).unwrap().start()
        } else {
            line.len()
        };
        let slice = line.get(start_index..end_index).unwrap_or("");
        let word_text = LRC_WORD.replace_all(slice, "").trim().to_string();
        if !word_text.is_empty() {
            words.push(LyricsWord {
                start_time,
                end_time: None,
                text: word_text,
            });
        }
    }
    if words.is_empty() {
        None
    } else {
        Some(words)
    }
}

// ---------------------------------------------------------------------------
// NetEase Music
// ---------------------------------------------------------------------------

async fn netease_request(ctx: &FetchCtx<'_>, url: &str) -> Result<Value, String> {
    let resp = ctx
        .http
        .client
        .post(url)
        .header("Referer", "https://music.163.com")
        .header("Cookie", "appver=2.0.2")
        .header("X-Real-IP", "202.96.0.0")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

async fn netease_lyrics(
    ctx: &FetchCtx<'_>,
    name: &str,
    artist: &str,
) -> Result<SongLyrics, String> {
    let search_url = format!(
        "https://music.163.com/api/search/get?s={}&type=1&offset=0&sub=false&limit=5",
        urlencoding::encode(&format!("{name}-{artist}"))
    );
    let search = netease_request(ctx, &search_url).await?;
    let song_count = search
        .get("result")
        .and_then(|r| r.get("songCount"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if song_count <= 0 {
        return Err("Song not found".to_string());
    }
    let song_id = search
        .get("result")
        .and_then(|r| r.get("songs"))
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_i64())
        .ok_or("Song not found")?;

    let lyric_url =
        format!("https://music.163.com/api/song/lyric?tv=-1&kv=-1&lv=-1&os=pc&id={song_id}");
    let json = netease_request(ctx, &lyric_url).await?;
    let lyric = json
        .get("lrc")
        .and_then(|l| l.get("lyric"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if lyric.is_empty() {
        return Err("Lyrics not found".to_string());
    }
    Ok(parse_unscaled_lrc(lyric))
}

/// LRC parse used by NetEase and QQ: hundredths taken verbatim (no digit-count
/// scaling), multiple timestamps per line supported.
fn parse_unscaled_lrc(lyrics: &str) -> SongLyrics {
    let mut out = SongLyrics::default();
    for raw_line in lyrics.split('\n') {
        if raw_line.is_empty() {
            continue;
        }
        let mut line = raw_line.to_string();
        let mut timestamps = Vec::new();
        while let Some(cap) = LRC_TAG.captures(&line) {
            let m: i64 = cap.get(1).unwrap().as_str().parse().unwrap_or(0);
            let s: i64 = cap.get(3).unwrap().as_str().parse().unwrap_or(0);
            let ms: i64 = cap.get(4).unwrap().as_str().parse().unwrap_or(0);
            let whole = cap.get(0).unwrap();
            let (start, end) = (whole.start(), whole.end());
            line.replace_range(start..end, "");
            timestamps.push((60 * m + s) * 1000 + ms);
        }
        for ts in timestamps {
            out.lines.push(LyricsLine {
                time: ts,
                text: line.clone(),
                end_time: None,
                words: None,
            });
        }
    }
    out.lines.sort_by_key(|l| l.time);
    out
}

// ---------------------------------------------------------------------------
// QQ Music
// ---------------------------------------------------------------------------

async fn qq_request(ctx: &FetchCtx<'_>, url: &str) -> Result<Value, String> {
    let resp = ctx
        .http
        .client
        .get(url)
        .header("Referer", "http://y.qq.com/portal/player.html")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    parse_jsonp(&text)
}

async fn qq_lyrics(
    ctx: &FetchCtx<'_>,
    name: &str,
    artist: &str,
) -> Result<SongLyrics, String> {
    let search_url = format!(
        "https://c.y.qq.com/splcloud/fcgi-bin/smartbox_new.fcg?inCharset=utf-8&outCharset=utf-8&key={}",
        urlencoding::encode(&format!("{name}-{artist}"))
    );
    let search = qq_request(ctx, &search_url).await?;
    let count = search.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    if count <= 0 {
        return Err("Song not found".to_string());
    }
    let mid = search
        .get("data")
        .and_then(|d| d.get("song"))
        .and_then(|s| s.get("itemlist"))
        .and_then(|i| i.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("mid"))
        .and_then(|v| v.as_str())
        .ok_or("Song not found")?
        .to_string();

    let lyric_url = format!(
        "http://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?g_tk=5381&format=json&inCharset=utf-8&outCharset=utf-8&songmid={mid}"
    );
    let json = qq_request(ctx, &lyric_url).await?;
    let lyric_b64 = json.get("lyric").and_then(|v| v.as_str()).unwrap_or("");
    if lyric_b64.is_empty() {
        return Err("Lyrics not found".to_string());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(lyric_b64)
        .map_err(|e| e.to_string())?;
    let lrc = String::from_utf8_lossy(&decoded);
    Ok(parse_qq_lrc(&lrc))
}

fn parse_qq_lrc(lyrics: &str) -> SongLyrics {
    let mut out = SongLyrics::default();
    for line in lyrics.split('\n') {
        if line.is_empty() {
            continue;
        }
        let cap = match LRC_TAG.captures(line) {
            Some(c) => c,
            None => continue,
        };
        let m: i64 = cap.get(1).unwrap().as_str().parse().unwrap_or(0);
        let s: i64 = cap.get(3).unwrap().as_str().parse().unwrap_or(0);
        let ms: i64 = cap.get(4).unwrap().as_str().parse().unwrap_or(0);
        let text = LRC_TAG.replace(line, "").to_string();
        out.lines.push(LyricsLine {
            time: (60 * m + s) * 1000 + ms,
            text: html_escape::decode_html_entities(&text).to_string(),
            end_time: None,
            words: None,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Accept bare JSON or a JSONP wrapper like `callback({...})`.
fn parse_jsonp(text: &str) -> Result<Value, String> {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return Ok(v);
    }
    if let (Some(start), Some(end)) = (text.find('('), text.rfind(')')) {
        if end > start {
            if let Ok(v) = serde_json::from_str::<Value>(&text[start + 1..end]) {
                return Ok(v);
            }
        }
    }
    Err("invalid JSON response".to_string())
}

/// JS `+value`-style coercion of a JSON number or numeric string to i64 (ms).
fn num_from_value(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Value::String(s) => s
            .trim()
            .parse::<i64>()
            .or_else(|_| s.trim().parse::<f64>().map(|f| f as i64))
            .unwrap_or(0),
        _ => 0,
    }
}

fn value_to_trimmed_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string().trim().to_string(),
    }
}
