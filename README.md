# LyricsStatus

Synced Spotify lyrics, right inside your Discord status. LyricsStatus is a native desktop app that follows the music you play through the Spotify account linked to Discord. It is built in Rust with Tauri v2, so it ships as a tiny binary and renders on the system WebView instead of bundling a browser.

## Get the app

Download the latest build for your platform from the [GitHub Releases](https://github.com/selfsecuredlab/lyrics-status/releases) page.

- **Windows** — NSIS installer, with shortcuts and an uninstall entry, or a standalone portable `.exe`.
- **macOS** — `.dmg` disk image / `.app`, universal for Intel and Apple Silicon.
- **Linux** — `.deb`, `.rpm`, or `.AppImage`.

## What it does

- Syncs lyrics and track changes from your Discord-linked Spotify account.
- Lets you adjust lyric timing and customize how lyrics appear.
- Previews the Discord activity while you configure the app.
- Keeps the familiar dark appearance and compact desktop controls.

## Requirements

- Windows 10 or 11, macOS, or Linux (64-bit)
- Discord desktop, signed in
- Spotify connected in Discord under **User Settings → Connections**

On Windows the app runs on WebView2, which is preinstalled on Windows 11 and added automatically by the setup installer on older versions.

## Run from source

You will need:

- [Rust](https://www.rust-lang.org/tools/install) (stable, installed via rustup)
- Node.js 20 or newer — only for the Tauri CLI
- The WebView and build dependencies Tauri needs for your OS; see the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for the per-platform list.

Then, from the repository root:

```powershell
git clone https://github.com/selfsecuredlab/lyrics-status.git
cd lyrics-status
npm ci
npx tauri dev
```

On first launch, connect Discord and then use the Spotify account linked to that Discord account.

## Build locally

```powershell
npx tauri build
```

Platform bundles land in `src-tauri/target/release/bundle/` — for example `nsis/` on Windows, `dmg/` on macOS, and `deb/` and `appimage/` on Linux. The standalone Windows executable is written to `src-tauri/target/release/lyricsstatus.exe`.

## Create a release

Push a `v*.*.*` git tag to publish a release:

```powershell
git tag v1.2.0
git push origin v1.2.0
```

The tag starts a GitHub Actions workflow that builds the Windows, macOS, and Linux bundles and publishes them to a matching GitHub release.

## Project layout

```text
src-tauri/   Rust backend, Tauri configuration, and bundle icons
static/      HTML, CSS, and JavaScript interface, reused unchanged
.github/     Release workflow
```

The interface in `static/` is reused unchanged from the previous build; the former Electron sources under `src/` are no longer the app. Local settings, credentials, lyric cache, logs, dependencies, and build output are not source files and are excluded from Git.

## Credits and license

Created by [selfsecured](https://github.com/selfsecuredlab). Licensed under the MIT License; see [LICENSE](LICENSE).
