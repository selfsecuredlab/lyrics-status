# Changelog

## v2.0.1

- LyricsStatus checks GitHub for a newer stable release at startup and every six hours. The update notice opens the official downloads in your browser.
- Automatic update checks can be turned off under **Settings → Application**.
- The GitHub download link opens in the default browser on Windows, macOS and Linux.

### Downloads

- **Windows x64** — NSIS setup executable.
- **macOS** — universal DMG for Apple silicon and Intel.
- **Linux x64** — AppImage, Debian package or RPM.

Install v2.0.1 manually once to receive in-app notifications for future releases.

[Full changelog](https://github.com/selfsecuredlab/lyrics-status/compare/v2.0.0...v2.0.1)

## v2.0.0

- Rebuilt the desktop app in Rust with Tauri 2 and the system WebView.
- Added native packages for Windows, macOS and Linux.

### Downloads

- **Windows x64** — NSIS setup executable.
- **macOS** — universal DMG for Apple silicon and Intel.
- **Linux x64** — AppImage, Debian package or RPM.

[Full changelog](https://github.com/selfsecuredlab/lyrics-status/compare/v1.1.1...v2.0.0)

## v1.1.1

- Fixed startup failures for system-wide Windows installations.
- Corrected the branch used to dispatch the optional website and VirusTotal refresh.

Download the x64 setup or portable executable for Windows. SHA-256 checksums for both are in `SHA256SUMS.txt`.

[Full changelog](https://github.com/selfsecuredlab/lyrics-status/compare/v1.1.0...v1.1.1)
