# LyricsStatus

LyricsStatus displays synchronized Spotify lyrics in your Discord custom status.

## Requirements

- Windows 10 or 11 (64-bit)
- Node.js 20 or newer
- Discord desktop app signed in
- Spotify connected in Discord under **User Settings > Connections**

## Setup

```powershell
git clone https://github.com/selfsecuredlab/lyrics-status.git
cd lyrics-status
npm ci
npm start
```

On the first launch, select **Connect Discord**, then **Connect Spotify**. LyricsStatus uses the Spotify account already linked to Discord.

## Build for Windows

Create the Windows installer:

```powershell
npm ci
npm run dist:installer
```

The installer is written to:

```text
release/LyricsStatus-Setup-1.0.0.exe
```

To create a portable executable instead:

```powershell
npm run dist:portable
```

## Project structure

```text
src/       Application and service code
static/    Desktop interface
res/       Packaged application icon
```

Local settings, credentials, lyric cache, dependencies, and build output are excluded from Git.

## Credits

Created by [selfsecured](https://discord.com/users/752923388955328752).

GitHub: [selfsecuredlab](https://github.com/selfsecuredlab)

## License

Licensed under the MIT License. See [LICENSE](LICENSE).
