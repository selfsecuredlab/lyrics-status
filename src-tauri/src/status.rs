//! Port of src/StatusChanger.ts. The timing/decision logic is synchronous and
//! returns the text to send; engine.rs performs the actual Discord PATCH so the
//! 60 Hz loop never blocks on the network.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use crate::autooffset::Autooffset;
use crate::datastore::now_ms;
use crate::lyrics::LyricsLine;
use crate::playback::PlaybackState;
use crate::settings::AllSettings;

static CHINESE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\u{4e00}-\u{9fff}\u{3400}-\u{4dbf}]").unwrap());
static LETTERS_ONLY: Lazy<Regex> = Lazy::new(|| Regex::new(r#"['",.]"#).unwrap());
static CROP: Lazy<Regex> = Lazy::new(|| Regex::new(r"( ?- ?.+)|(\(.+\))").unwrap());
static PAREN_SUFFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r" \(.+\)").unwrap());

pub struct StatusChanger {
    pub sent_lines: Vec<i64>,
    pub sent_no_lyrics_status: bool,
    pub last_no_lyrics_update: i64,
    pub autooffset: Autooffset,

    pub last_sent_status_text: String,
    pub original_custom_status: Option<Value>,
    pub original_status_captured: bool,
    pub lyrics_status_active: bool,
}

impl StatusChanger {
    pub fn new() -> StatusChanger {
        StatusChanger {
            sent_lines: Vec::new(),
            sent_no_lyrics_status: false,
            last_no_lyrics_update: 0,
            autooffset: Autooffset::new(),
            last_sent_status_text: String::new(),
            original_custom_status: None,
            original_status_captured: false,
            lyrics_status_active: false,
        }
    }

    pub fn song_changed(&mut self) {
        self.sent_lines.clear();
        self.sent_no_lyrics_status = false;
        self.last_no_lyrics_update = 0;
        self.last_sent_status_text.clear();
    }

    /// Called after a status PATCH actually succeeds.
    pub fn record_sent(&mut self, roundtrip_ms: i64) {
        self.lyrics_status_active = true;
        self.autooffset.add_value(roundtrip_ms);
    }

    fn max_status_length(settings: &AllSettings) -> usize {
        let requested = settings.view.behavior.max_length;
        requested.clamp(32, 128) as usize
    }

    fn contains_chinese(text: &str) -> bool {
        CHINESE.is_match(text)
    }

    pub fn format_seconds(s: i64) -> String {
        let s = s.max(0);
        let sec = s % 60;
        let min = (s - sec) / 60;
        let pad = if sec > 9 { "" } else { "0" };
        format!("{min}:{pad}{sec}")
    }

    fn get_lyric_text(&self, line: &LyricsLine, pb: &PlaybackState, settings: &AllSettings) -> String {
        if settings.view.behavior.mode != "multi" {
            return line.text.clone();
        }
        let lines = match &pb.lyrics {
            Some(l) => &l.lines,
            None => return line.text.clone(),
        };
        let current_index = lines.iter().position(|c| c.time == line.time);
        let next_line = current_index.and_then(|idx| {
            lines
                .iter()
                .skip(idx + 1)
                .find(|c| !c.text.is_empty() && !Self::contains_chinese(&c.text))
        });
        match next_line {
            Some(n) => format!("{} \u{2022} {}", line.text, n.text),
            None => line.text.clone(),
        }
    }

    fn get_status_string(&self, line: &LyricsLine, pb: &PlaybackState, settings: &AllSettings) -> String {
        let lyric_text = self.get_lyric_text(line, pb, settings).replace('\u{266a}', "\u{1f3b6}");
        let ts = if settings.view.timestamp {
            format!("[{}] ", Self::format_seconds((line.time as f64 / 1000.0).round() as i64))
        } else {
            String::new()
        };
        let label = if settings.view.label { "Song lyrics - " } else { "" };
        let full = format!("{ts}{label}{lyric_text}");
        truncate_chars(&full, Self::max_status_length(settings))
    }

    /// Current status text without side effects (for change detection / events).
    pub fn current_status_text(&self, pb: &PlaybackState, settings: &AllSettings) -> String {
        let current_line = match &pb.current_line {
            Some(l) => l.clone(),
            None => return String::new(),
        };
        if settings.view.advanced.enabled {
            self.parse_status_string(&settings.view.advanced.custom_status, pb, settings)
        } else {
            self.get_status_string(&current_line, pb, settings)
        }
    }

    fn parse_status_string(&self, status: &str, pb: &PlaybackState, settings: &AllSettings) -> String {
        let max_len = Self::max_status_length(settings);
        let line = match &pb.current_line {
            Some(l) => l.clone(),
            None => return truncate_chars(status, max_len),
        };

        let lyric_text = self.get_lyric_text(&line, pb, settings);
        let song_name = &pb.song_name;
        let song_author = &pb.song_author;
        let elapsed = Self::format_seconds((pb.song_progress as f64 / 1000.0).round() as i64);
        let remaining = Self::format_seconds(
            ((pb.song_duration - pb.song_progress).max(0) as f64 / 1000.0).round() as i64,
        );
        let duration = Self::format_seconds((pb.song_duration as f64 / 1000.0).round() as i64);

        let mut s = status.to_string();
        s = replace_first(&s, "{lyrics}", &lyric_text);
        s = replace_first(&s, "{lyrics_upper}", &lyric_text.to_uppercase());
        s = replace_first(&s, "{lyrics_lower}", &lyric_text.to_lowercase());
        s = replace_first(&s, "{lyrics_letters_only}", &letters_only(&lyric_text));
        s = replace_first(&s, "{lyrics_upper_letters_only}", &letters_only(&lyric_text.to_uppercase()));
        s = replace_first(&s, "{lyrics_lower_letters_only}", &letters_only(&lyric_text.to_lowercase()));
        s = replace_first(&s, "{timestamp}", &Self::format_seconds((line.time as f64 / 1000.0).round() as i64));
        s = replace_first(&s, "{elapsed}", &elapsed);
        s = replace_first(&s, "{remaining}", &remaining);
        s = replace_first(&s, "{duration}", &duration);
        s = replace_first(&s, "{song}", song_name);
        s = replace_first(&s, "{song_name}", song_name);
        s = replace_first(&s, "{song_name_upper}", &song_name.to_uppercase());
        s = replace_first(&s, "{song_name_lower}", &song_name.to_lowercase());
        s = replace_first(&s, "{song_name_cropped}", &crop(song_name));
        s = replace_first(&s, "{song_name_upper_cropped}", &crop(&song_name.to_uppercase()));
        s = replace_first(&s, "{song_name_lower_cropped}", &crop(&song_name.to_lowercase()));
        s = replace_first(&s, "{artist}", song_author);
        s = replace_first(&s, "{song_author}", song_author);
        s = replace_first(&s, "{song_author_upper}", &song_author.to_uppercase());
        s = replace_first(&s, "{song_author_lower}", &song_author.to_lowercase());
        s = replace_first(&s, "{album}", &pb.song_album);

        truncate_chars(&s, max_len)
    }

    /// Port of changeStatus(): returns Some(text) when a new status should be
    /// PATCHed to Discord (deduplicated on the last sent text).
    pub fn decide(&mut self, pb: &mut PlaybackState, settings: &AllSettings) -> Option<String> {
        self.autooffset.set_limit(settings.timings.autooffset);

        if pb.ended() || !pb.is_playing {
            return None;
        }

        let max_len = Self::max_status_length(settings);

        // No lyrics: show "song - artist", resend every 5 minutes.
        if !pb.has_lyrics {
            let now = now_ms();
            let should_resend =
                !self.sent_no_lyrics_status || (now - self.last_no_lyrics_update > 300_000);
            if !should_resend {
                return None;
            }
            let no_lyrics_text = format!("{} - {}", pb.song_name, pb.song_author);
            let status_text = if settings.view.advanced.enabled {
                let elapsed = Self::format_seconds((pb.song_progress as f64 / 1000.0).round() as i64);
                let remaining = Self::format_seconds(
                    ((pb.song_duration - pb.song_progress).max(0) as f64 / 1000.0).round() as i64,
                );
                let duration = Self::format_seconds((pb.song_duration as f64 / 1000.0).round() as i64);
                let mut t = settings.view.advanced.custom_status.clone();
                t = replace_first(&t, "{lyrics}", &no_lyrics_text);
                t = replace_first(&t, "{lyrics_upper}", &no_lyrics_text.to_uppercase());
                t = replace_first(&t, "{lyrics_lower}", &no_lyrics_text.to_lowercase());
                t = replace_first(&t, "{lyrics_letters_only}", &letters_only(&no_lyrics_text));
                t = replace_first(&t, "{timestamp}", "0:00");
                t = replace_first(&t, "{elapsed}", &elapsed);
                t = replace_first(&t, "{remaining}", &remaining);
                t = replace_first(&t, "{duration}", &duration);
                t = replace_first(&t, "{song}", &pb.song_name);
                t = replace_first(&t, "{song_name}", &pb.song_name);
                t = replace_first(&t, "{artist}", &pb.song_author);
                t = replace_first(&t, "{song_author}", &pb.song_author);
                t = replace_first(&t, "{album}", &pb.song_album);
                truncate_chars(&t, max_len)
            } else {
                truncate_chars(&no_lyrics_text, max_len)
            };
            self.sent_no_lyrics_status = true;
            self.last_no_lyrics_update = now;
            return self.queue(status_text);
        }

        self.sent_no_lyrics_status = false;
        let lyrics = match &pb.lyrics {
            Some(l) => l.clone(),
            None => return None,
        };

        let song_progress = pb.song_progress;
        let offset: f64 = if settings.timings.enable_autooffset {
            self.autooffset.average() + 100.0
        } else {
            settings.timings.send_time_offset as f64
        };
        let threshold = song_progress as f64 + offset;

        let lines = &lyrics.lines;
        let mut line_to_display: Option<LyricsLine> = None;
        for i in 0..lines.len() {
            let line = &lines[i];
            if (line.time as f64) < threshold {
                if line.text.is_empty() {
                    continue;
                }
                if Self::contains_chinese(&line.text) {
                    continue;
                }
                if let Some(next) = lines.get(i + 1) {
                    if (next.time as f64) < threshold {
                        continue;
                    }
                }
                line_to_display = Some(line.clone());
            }
        }

        let line_to_display = line_to_display?;
        let is_new = pb
            .current_line
            .as_ref()
            .map(|c| c.time != line_to_display.time)
            .unwrap_or(true);
        if !is_new {
            return None;
        }
        if self.sent_lines.iter().any(|t| *t == line_to_display.time) {
            return None;
        }

        pb.current_line = Some(line_to_display.clone());
        let text = if settings.view.advanced.enabled {
            self.parse_status_string(&settings.view.advanced.custom_status, pb, settings)
        } else {
            self.get_status_string(&line_to_display, pb, settings)
        };
        self.sent_lines.push(line_to_display.time);
        self.queue(text)
    }

    /// Deduplicate against the last sent text (changeStatusRequest's 304 path).
    fn queue(&mut self, text: String) -> Option<String> {
        if text == self.last_sent_status_text {
            return None;
        }
        self.last_sent_status_text = text.clone();
        Some(text)
    }
}

fn letters_only(s: &str) -> String {
    LETTERS_ONLY.replace_all(s, "").to_string()
}

fn crop(s: &str) -> String {
    CROP.replace_all(s, "").to_string()
}

/// JS String.prototype.slice(0, n) counts UTF-16 code units; we approximate with
/// chars, which is correct for the BMP text Discord statuses use.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

/// JS String.replace(string, string) replaces only the first occurrence.
fn replace_first(haystack: &str, needle: &str, replacement: &str) -> String {
    match haystack.find(needle) {
        Some(idx) => {
            let mut out = String::with_capacity(haystack.len() + replacement.len());
            out.push_str(&haystack[..idx]);
            out.push_str(replacement);
            out.push_str(&haystack[idx + needle.len()..]);
            out
        }
        None => haystack.to_string(),
    }
}

// Silence unused import warning when the paren-suffix helper is not referenced.
#[allow(dead_code)]
fn strip_paren_suffix(s: &str) -> String {
    PAREN_SUFFIX.replace(s, "").to_string()
}
