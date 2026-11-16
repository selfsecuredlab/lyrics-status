//! Tauri application wiring — the Rust replacement for Electron's main process
//! (src/main.ts): window, tray, IPC registration, lifecycle and auto-start.

mod autooffset;
mod commands;
mod datastore;
mod debug;
mod discord;
mod http;
mod lyrics;
mod playback;
mod settings;
mod spotify;
mod state;
mod status;
mod token_extract;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();

    tauri::Builder::default()
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::window_minimize,
            commands::window_maximize,
            commands::window_close,
            commands::window_quit,
            commands::settings_get,
            commands::settings_set,
            commands::app_start,
            commands::app_stop,
            commands::app_toggle,
            commands::app_status,
            commands::spotify_authorize,
            commands::spotify_play,
            commands::spotify_pause,
            commands::spotify_resume,
            commands::spotify_next,
            commands::spotify_previous,
            commands::token_validate,
            commands::history_get,
            commands::history_clear,
            commands::open_external,
            commands::spotify_user,
            commands::spotify_playlists,
            commands::spotify_liked_songs,
            commands::spotify_devices,
            commands::spotify_genres,
            commands::spotify_playlist_tracks,
            commands::spotify_seek,
            commands::spotify_volume,
            commands::spotify_transfer,
            commands::spotify_recently_played,
            commands::spotify_audio_features,
            commands::spotify_save_track,
            commands::spotify_remove_track,
            commands::spotify_is_track_saved,
            commands::spotify_search_tracks,
            commands::spotify_search_artists,
            commands::spotify_artist,
            commands::spotify_artist_top_tracks,
            commands::spotify_album,
            commands::spotify_album_tracks,
            commands::spotify_track,
            commands::spotify_recommendations,
            commands::spotify_new_releases,
            commands::spotify_featured_playlists,
            commands::discord_user,
            commands::discord_rate_limit_current,
            commands::discord_extract_token,
            commands::discord_dump_leveldb,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            app_state.set_app(handle.clone());

            build_tray(app)?;

            let settings = app_state.settings_snapshot();
            if let Some(window) = app.get_webview_window("main") {
                if settings.general.start_minimized {
                    let _ = window.hide();
                } else {
                    let _ = window.center();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            // Auto-start when a Discord token is configured.
            let st = app_state.clone();
            let emit_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if !st.settings.lock().unwrap().credentials.token.is_empty() {
                    st.start().await;
                    if let Some(user) = st.spotify.user_profile().await {
                        let _ = emit_handle.emit("spotify:user", user);
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle().clone();
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    commands::graceful_quit(app, state).await;
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running LyricsStatus");
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show LyricsStatus", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Pause / Resume", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &sep, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("tray icon".into()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("LyricsStatus")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "toggle" => {
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    state.toggle().await;
                });
            }
            "quit" => {
                let app = app.clone();
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    commands::graceful_quit(app, state).await;
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
