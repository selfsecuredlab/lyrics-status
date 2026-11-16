//! Tauri command handlers — the whole IPC surface the renderer calls through
//! window.electronAPI (see main.ts / preload.ts). The compatibility shim in
//! static/electron-shim.js maps each electronAPI method onto these.

use serde_json::{json, Value};
use tauri::State;

use crate::discord;
use crate::state::AppState;

type R<T> = Result<T, String>;

// --- Window controls ------------------------------------------------------

#[tauri::command]
pub fn window_minimize(window: tauri::WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn window_maximize(window: tauri::WebviewWindow) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub async fn window_close(app: tauri::AppHandle, state: State<'_, AppState>) -> R<()> {
    graceful_quit(app, state.inner().clone()).await;
    Ok(())
}

#[tauri::command]
pub async fn window_quit(app: tauri::AppHandle, state: State<'_, AppState>) -> R<()> {
    graceful_quit(app, state.inner().clone()).await;
    Ok(())
}

/// Restore the user's Discord status, persist, then exit — mirrors forceQuit().
pub async fn graceful_quit(app: tauri::AppHandle, state: AppState) {
    state.stop().await;
    app.exit(0);
}

// --- Settings -------------------------------------------------------------

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Value {
    state.settings.lock().unwrap().as_value()
}

#[tauri::command]
pub async fn settings_set(settings: Value, state: State<'_, AppState>) -> R<bool> {
    let st = state.inner().clone();
    {
        let mut s = st.settings.lock().unwrap();
        s.apply_patch(&settings);
        s.save(&st.settings_path);
    }
    let has_token = !st.settings.lock().unwrap().credentials.token.is_empty();
    if !st.is_running() && has_token {
        st.start().await;
    }
    Ok(true)
}

// --- App controls ---------------------------------------------------------

#[tauri::command]
pub async fn app_start(state: State<'_, AppState>) -> R<bool> {
    let st = state.inner().clone();
    st.start().await;
    Ok(st.is_running())
}

#[tauri::command]
pub async fn app_stop(state: State<'_, AppState>) -> R<bool> {
    let st = state.inner().clone();
    st.stop().await;
    Ok(true)
}

#[tauri::command]
pub async fn app_toggle(state: State<'_, AppState>) -> R<bool> {
    let st = state.inner().clone();
    st.toggle().await;
    Ok(st.is_running())
}

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> Value {
    state.app_status()
}

// --- Spotify auth / playback ---------------------------------------------

#[tauri::command]
pub async fn spotify_authorize(app: tauri::AppHandle, state: State<'_, AppState>) -> R<Value> {
    use tauri::Emitter;
    let st = state.inner().clone();
    if st.settings.lock().unwrap().credentials.token.is_empty() {
        return Ok(json!({ "success": false, "error": "Enter and save your Discord token first" }));
    }
    if !st.spotify.refresh().await {
        return Ok(json!({
            "success": false,
            "error": "No usable Spotify account was found. Link Spotify in Discord Settings > Connections, then try again."
        }));
    }
    let user = st.spotify.user_profile().await;
    if let Some(ref u) = user {
        let _ = app.emit("spotify:user", u.clone());
    }
    let _ = app.emit("spotify:auth-complete", json!({ "source": "discord" }));
    if !st.is_running() {
        st.start().await;
    }
    Ok(json!({ "success": true, "user": user }))
}

#[tauri::command]
pub async fn spotify_play(data: Value, state: State<'_, AppState>) -> R<Value> {
    let st = state.inner().clone();
    let mut uri = data.get("uri").and_then(|v| v.as_str()).map(|s| s.to_string());
    if uri.is_none() {
        let name = data.get("name").and_then(|v| v.as_str());
        let artist = data.get("artist").and_then(|v| v.as_str());
        if let (Some(n), Some(a)) = (name, artist) {
            uri = st.spotify.search_track(n, a).await;
        }
    }
    let uri = match uri {
        Some(u) => u,
        None => return Ok(json!({ "success": false, "error": "Could not find track" })),
    };
    let success = st.spotify.play_track(&uri).await;
    Ok(json!({
        "success": success,
        "error": if success { Value::Null } else { json!("Failed to play. Make sure Spotify is linked in Discord and open on an active device.") },
    }))
}

#[tauri::command]
pub async fn spotify_pause(state: State<'_, AppState>) -> R<Value> {
    let st = state.inner().clone();
    let success = st.spotify.pause().await;
    if success {
        st.clear_discord_status().await;
    }
    Ok(json!({ "success": success, "error": if success { Value::Null } else { json!("Spotify could not pause playback") } }))
}

#[tauri::command]
pub async fn spotify_resume(state: State<'_, AppState>) -> R<Value> {
    let success = state.inner().clone().spotify.resume().await;
    Ok(json!({ "success": success, "error": if success { Value::Null } else { json!("Spotify could not resume playback") } }))
}

#[tauri::command]
pub async fn spotify_next(state: State<'_, AppState>) -> R<Value> {
    let success = state.inner().clone().spotify.next().await;
    Ok(json!({ "success": success, "error": if success { Value::Null } else { json!("Spotify could not skip to the next track") } }))
}

#[tauri::command]
pub async fn spotify_previous(state: State<'_, AppState>) -> R<Value> {
    let success = state.inner().clone().spotify.previous().await;
    Ok(json!({ "success": success, "error": if success { Value::Null } else { json!("Spotify could not return to the previous track") } }))
}

// --- Token validation -----------------------------------------------------

#[tauri::command]
pub async fn token_validate(token: String, state: State<'_, AppState>) -> R<Value> {
    Ok(discord::validate_token(&state.http, &token).await)
}

// --- History --------------------------------------------------------------

#[tauri::command]
pub fn history_get(state: State<'_, AppState>) -> Value {
    state.datastore.lock().unwrap().get_history()
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> bool {
    state.datastore.lock().unwrap().clear_history();
    true
}

// --- External links -------------------------------------------------------

#[tauri::command]
pub fn open_external(url: String) {
    if url.starts_with("http://") || url.starts_with("https://") {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", &url])
                .spawn();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
    }
}

// --- Spotify data (pass-through to SpotifyService) ------------------------

macro_rules! spotify_opt {
    ($name:ident, $method:ident) => {
        #[tauri::command]
        pub async fn $name(state: State<'_, AppState>) -> R<Value> {
            Ok(state.inner().clone().spotify.$method().await.unwrap_or(Value::Null))
        }
    };
}

spotify_opt!(spotify_user, user_profile);
spotify_opt!(spotify_playlists, playlists);
spotify_opt!(spotify_liked_songs, liked_songs);
spotify_opt!(spotify_devices, devices);
spotify_opt!(spotify_genres, available_genres);

#[tauri::command]
pub async fn spotify_playlist_tracks(playlist_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.playlist_tracks(&playlist_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_seek(position_ms: i64, state: State<'_, AppState>) -> R<bool> {
    Ok(state.inner().clone().spotify.seek(position_ms).await)
}

#[tauri::command]
pub async fn spotify_volume(volume_percent: i64, state: State<'_, AppState>) -> R<bool> {
    Ok(state.inner().clone().spotify.set_volume(volume_percent).await)
}

#[tauri::command]
pub async fn spotify_transfer(device_id: String, play: Option<bool>, state: State<'_, AppState>) -> R<bool> {
    Ok(state.inner().clone().spotify.transfer_playback(&device_id, play.unwrap_or(true)).await)
}

#[tauri::command]
pub async fn spotify_recently_played(limit: Option<i64>, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.recently_played(limit.unwrap_or(20)).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_audio_features(track_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.audio_features(&track_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_save_track(track_id: String, state: State<'_, AppState>) -> R<bool> {
    Ok(state.inner().clone().spotify.save_track(&track_id).await)
}

#[tauri::command]
pub async fn spotify_remove_track(track_id: String, state: State<'_, AppState>) -> R<bool> {
    Ok(state.inner().clone().spotify.remove_track(&track_id).await)
}

#[tauri::command]
pub async fn spotify_is_track_saved(track_id: String, state: State<'_, AppState>) -> R<bool> {
    Ok(state.inner().clone().spotify.is_track_saved(&track_id).await)
}

#[tauri::command]
pub async fn spotify_search_tracks(query: String, limit: Option<i64>, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.search_tracks(&query, limit.unwrap_or(20)).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_search_artists(query: String, limit: Option<i64>, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.search_artists(&query, limit.unwrap_or(20)).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_artist(artist_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.artist(&artist_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_artist_top_tracks(artist_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.artist_top_tracks(&artist_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_album(album_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.album(&album_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_album_tracks(album_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.album_tracks(&album_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_track(track_id: String, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.track(&track_id).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_recommendations(
    seed_tracks: Option<Vec<String>>,
    seed_artists: Option<Vec<String>>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> R<Value> {
    Ok(state
        .inner()
        .clone()
        .spotify
        .recommendations(seed_tracks, seed_artists, limit.unwrap_or(20))
        .await
        .unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_new_releases(limit: Option<i64>, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.new_releases(limit.unwrap_or(20)).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn spotify_featured_playlists(limit: Option<i64>, state: State<'_, AppState>) -> R<Value> {
    Ok(state.inner().clone().spotify.featured_playlists(limit.unwrap_or(20)).await.unwrap_or(Value::Null))
}

// --- Discord --------------------------------------------------------------

#[tauri::command]
pub async fn discord_user(state: State<'_, AppState>) -> R<Value> {
    let token = state.settings.lock().unwrap().credentials.token.clone();
    Ok(discord::user_profile(&state.http, &token).await.unwrap_or(Value::Null))
}

#[tauri::command]
pub fn discord_rate_limit_current() -> Value {
    // The renderer also receives live discord:rate-limit events; this snapshot
    // is best-effort and returns null (the original cached the last event).
    Value::Null
}

#[tauri::command]
pub async fn discord_extract_token(state: State<'_, AppState>) -> R<Value> {
    let http = state.http.clone();
    Ok(crate::token_extract::extract_token(http.as_ref()).await)
}

#[tauri::command]
pub fn discord_dump_leveldb() -> Value {
    json!({ "success": false, "error": "Diagnostic dump is not available in this build." })
}
