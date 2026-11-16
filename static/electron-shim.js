// electron-shim.js
// Recreates the window.electronAPI bridge (originally provided by Electron's
// preload.ts) on top of Tauri v2's global invoke/event APIs, so the existing
// renderer runs unchanged against the Rust backend.
(function () {
  'use strict';

  var T = window.__TAURI__;
  if (!T || !T.core || !T.event) {
    console.error('[electron-shim] Tauri globals are unavailable; UI will not connect to the backend.');
    return;
  }

  var invoke = T.core.invoke;
  var listen = T.event.listen;

  // Pass both camelCase and snake_case keys; the backend reads whichever it
  // expects and ignores the rest.
  function id(obj) { return obj; }
  function arg(camel, snake, value) {
    var o = {};
    o[camel] = value;
    o[snake] = value;
    return o;
  }

  function on(channel) {
    return function (callback) {
      listen(channel, function (event) { callback(event.payload); });
    };
  }

  window.electronAPI = {
    // Window controls
    minimize: function () { return invoke('window_minimize'); },
    maximize: function () { return invoke('window_maximize'); },
    close: function () { return invoke('window_close'); },
    quit: function () { return invoke('window_quit'); },

    // Settings
    getSettings: function () { return invoke('settings_get'); },
    setSettings: function (settings) { return invoke('settings_set', { settings: settings }); },

    // App controls
    startApp: function () { return invoke('app_start'); },
    stopApp: function () { return invoke('app_stop'); },
    toggleApp: function () { return invoke('app_toggle'); },
    getStatus: function () { return invoke('app_status'); },

    // Spotify
    authorizeSpotify: function () { return invoke('spotify_authorize'); },
    playSong: function (data) { return invoke('spotify_play', { data: data }); },
    pauseSong: function () { return invoke('spotify_pause'); },
    resumeSong: function () { return invoke('spotify_resume'); },
    nextSong: function () { return invoke('spotify_next'); },
    previousSong: function () { return invoke('spotify_previous'); },
    validateToken: function (token) { return invoke('token_validate', { token: token }); },

    // History
    getHistory: function () { return invoke('history_get'); },
    clearHistory: function () { return invoke('history_clear'); },

    // External links
    openExternal: function (url) { return invoke('open_external', { url: url }); },

    // Spotify user / library / browse
    getSpotifyUser: function () { return invoke('spotify_user'); },
    getPlaylists: function () { return invoke('spotify_playlists'); },
    getPlaylistTracks: function (playlistId) { return invoke('spotify_playlist_tracks', arg('playlistId', 'playlist_id', playlistId)); },
    getLikedSongs: function () { return invoke('spotify_liked_songs'); },
    seek: function (positionMs) { return invoke('spotify_seek', arg('positionMs', 'position_ms', positionMs)); },
    setVolume: function (volumePercent) { return invoke('spotify_volume', arg('volumePercent', 'volume_percent', volumePercent)); },
    getDevices: function () { return invoke('spotify_devices'); },
    transferPlayback: function (deviceId, play) {
      var o = arg('deviceId', 'device_id', deviceId);
      o.play = play;
      return invoke('spotify_transfer', o);
    },
    getRecentlyPlayed: function (limit) { return invoke('spotify_recently_played', { limit: limit }); },
    getAudioFeatures: function (trackId) { return invoke('spotify_audio_features', arg('trackId', 'track_id', trackId)); },
    saveTrack: function (trackId) { return invoke('spotify_save_track', arg('trackId', 'track_id', trackId)); },
    removeTrack: function (trackId) { return invoke('spotify_remove_track', arg('trackId', 'track_id', trackId)); },
    isTrackSaved: function (trackId) { return invoke('spotify_is_track_saved', arg('trackId', 'track_id', trackId)); },
    searchTracks: function (query, limit) { return invoke('spotify_search_tracks', { query: query, limit: limit }); },
    searchArtists: function (query, limit) { return invoke('spotify_search_artists', { query: query, limit: limit }); },
    getArtist: function (artistId) { return invoke('spotify_artist', arg('artistId', 'artist_id', artistId)); },
    getArtistTopTracks: function (artistId) { return invoke('spotify_artist_top_tracks', arg('artistId', 'artist_id', artistId)); },
    getAlbum: function (albumId) { return invoke('spotify_album', arg('albumId', 'album_id', albumId)); },
    getAlbumTracks: function (albumId) { return invoke('spotify_album_tracks', arg('albumId', 'album_id', albumId)); },
    getTrack: function (trackId) { return invoke('spotify_track', arg('trackId', 'track_id', trackId)); },
    getRecommendations: function (seedTracks, seedArtists, limit) {
      return invoke('spotify_recommendations', {
        seedTracks: seedTracks, seed_tracks: seedTracks,
        seedArtists: seedArtists, seed_artists: seedArtists,
        limit: limit
      });
    },
    getGenres: function () { return invoke('spotify_genres'); },
    getNewReleases: function (limit) { return invoke('spotify_new_releases', { limit: limit }); },
    getFeaturedPlaylists: function (limit) { return invoke('spotify_featured_playlists', { limit: limit }); },

    // Discord
    getDiscordUser: function () { return invoke('discord_user'); },
    getDiscordRateLimit: function () { return invoke('discord_rate_limit_current'); },
    extractDiscordToken: function () { return invoke('discord_extract_token'); },
    dumpDiscordLevelDB: function () { return invoke('discord_dump_leveldb'); },

    // Event listeners (main -> renderer)
    onSongChanged: on('song:changed'),
    onLyricsUpdated: on('lyrics:updated'),
    onStatusSent: on('status:sent'),
    onSpotifyUser: on('spotify:user'),
    onSpotifyAuthComplete: on('spotify:auth-complete'),
    onDiscordRateLimit: on('discord:rate-limit'),
    onDiscordRateLimitCleared: on('discord:rate-limit-cleared'),
    onceSpotifyAuthComplete: function (callback) {
      var fired = false;
      var unlistenPromise = listen('spotify:auth-complete', function (event) {
        if (fired) return;
        fired = true;
        callback(event.payload);
        unlistenPromise.then(function (un) { un(); });
      });
    }
  };

  // Keep id() referenced to avoid dead-code stripping complaints in strict tools.
  void id;
})();
