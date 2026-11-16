//! Port of src/PlaybackState.ts — the live view of what Spotify is playing.

use crate::lyrics::{LyricsLine, SongLyrics};

pub struct PlaybackState {
    pub song_name: String,
    pub song_author: String,

    pub song_id: String,
    pub old_song_id: String,
    pub song_uri: String,
    pub album_art: String,
    pub song_album: String,

    pub song_duration: i64,
    pub song_progress: i64,

    pub lyrics: Option<SongLyrics>,
    pub current_line: Option<LyricsLine>,
    pub has_lyrics: bool,

    pub is_playing: bool,
    pub spotify_connected: bool,
}

impl PlaybackState {
    pub fn new() -> PlaybackState {
        PlaybackState {
            song_name: String::new(),
            song_author: String::new(),
            song_id: String::new(),
            old_song_id: String::new(),
            song_uri: String::new(),
            album_art: String::new(),
            song_album: String::new(),
            song_duration: 0,
            song_progress: 0,
            lyrics: None,
            current_line: None,
            has_lyrics: false,
            is_playing: false,
            spotify_connected: true,
        }
    }

    pub fn ended(&self) -> bool {
        self.song_duration < self.song_progress
    }
}
