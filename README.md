# LyricsStatus

Synced Spotify lyrics, right inside your Discord status. LyricsStatus is a Windows desktop app that follows the music you play through the Spotify account linked to Discord.

## Get the app

Download the Windows installer or portable build from the [GitHub Releases](https://github.com/selfsecuredlab/lyrics-status/releases) page.

- **Installer** — standard setup with shortcuts and an uninstall entry.
- **Portable** — run the executable directly without installing it.

## What it does

- Syncs lyrics and track changes from your Discord-linked Spotify account.
- Lets you adjust lyric timing and customize how lyrics appear.
- Previews the Discord activity while you configure the app.
- Keeps the familiar dark appearance and compact desktop controls.

## Requirements

- Windows 10 or 11, 64-bit
- Discord desktop, signed in
- Spotify connected in Discord under **User Settings → Connections**
- Node.js 22.12 or newer (only when running or building from source)

## Run from source

```powershell
git clone https://github.com/selfsecuredlab/lyrics-status.git
cd lyrics-status
npm ci
npm start
```

On first launch, connect Discord and then use the Spotify account linked to that Discord account.

## Build for Windows

Install the dependencies and create the installer:

```powershell
npm ci
npm run dist:installer
```

The installer is written to `release/LyricsStatus-Setup-1.0.0.exe`. The filename uses the version in `package.json`.

Build the portable executable or both artifacts with:

```powershell
npm run dist:portable
npm run dist:release
```

Both build commands write into the ignored `release/` directory.

## Create a GitHub release

For a published release, use the repository's release script from a clean, up-to-date `main` branch:

```powershell
npm run release -- 1.1.0
```

The script validates the version, builds the TypeScript app, commits the version change, creates a matching `v*.*.*` tag, and pushes the commit and tag. The tag starts the GitHub Actions workflow, which builds the installer and portable executable, creates SHA-256 checksums, and publishes the GitHub release. Configure the workflow's required `WEBSITE_ACTIONS_TOKEN` secret before running it; the release workflow uses it for its existing follow-up automation.

## Project layout

```text
src/                 Electron main process and application services
static/              Desktop renderer, styles, and interface assets
res/                 Packaged icons and application resources
scripts/release.ps1  Version, tag, and publish workflow
```

The app's local settings, credentials, lyric cache, logs, dependencies, and build output are not source files and are excluded from Git.

## Credits and license

Created by [selfsecured](https://github.com/selfsecuredlab). Licensed under the MIT License; see [LICENSE](LICENSE).
