# LyricsStatus

Synced Spotify lyrics, right inside your Discord status. LyricsStatus is a native desktop app that follows the music you play through the Spotify account linked to Discord. It is built in Rust with Tauri v2, so it ships as a tiny binary and renders on the system WebView instead of bundling a browser.

## Get the app

Download the latest build for your platform from the [GitHub Releases](https://github.com/selfsecuredlab/lyrics-status/releases) page.

- **Windows** — x64 NSIS setup installer, with shortcuts and an uninstall entry. The portable Windows build is available in the [legacy v1.1.1 release](https://github.com/selfsecuredlab/lyrics-status/releases/tag/v1.1.1).
- **macOS** — `.dmg` disk image / `.app`, universal for Intel and Apple Silicon.
- **Linux** — `.deb`, `.rpm`, or `.AppImage`.

LyricsStatus checks GitHub for a newer stable release on startup and every six hours. The in-app notice opens the release downloads in your browser. To turn off checks, open **Settings → Application** and switch off **Check for updates automatically**.

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
git clone --branch rust https://github.com/selfsecuredlab/lyrics-status.git
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

Update the version in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`. Add a matching version heading and notes to [CHANGELOG.md](CHANGELOG.md).

Pushing the version change to `rust` starts the release workflow. It creates the matching `v*.*.*` tag and uploads the Windows, macOS and Linux bundles to a draft. The workflow publishes the release with the changelog entry once all platform builds and the asset check pass. A matching tag pushed manually also triggers the workflow.

## Project layout

```text
src-tauri/   Rust backend, Tauri configuration, and bundle icons
static/      HTML, CSS, and JavaScript interface, reused unchanged
.github/     Release workflow
```

The interface in `static/` is reused unchanged from the previous build; the former Electron sources under `src/` are no longer the app. Local settings, credentials, lyric cache, logs, dependencies, and build output are not source files and are excluded from Git.

## Credits and license

Created by [selfsecured](https://github.com/selfsecuredlab). Licensed under the MIT License; see [LICENSE](LICENSE).
