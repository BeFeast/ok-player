<div align="center">

# OK Player

**The most elegant media player on Windows** — macOS-utility-grade polish, in native Fluent/Mica.

[![Latest release](https://img.shields.io/github/v/release/BeFeast/ok-player?sort=semver&label=download)](https://github.com/BeFeast/ok-player/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/BeFeast/ok-player/total?label=downloads)](https://github.com/BeFeast/ok-player/releases)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPLv3-blue.svg)](LICENSE)
[![Platform: Windows 11](https://img.shields.io/badge/platform-Windows%2011-0078D6)](https://github.com/BeFeast/ok-player/releases/latest)

OK Player is a Windows-native player — an [IINA](https://iina.io)-class GUI over the **mpv** engine ([libmpv](https://mpv.io)). It’s a **pure player**: open a file, folder, or URL and it plays it beautifully. Library management lives in a separate companion app, with clean integration between the two.

<img src="docs/screenshots/chapters-godfather.png" alt="OK Player — chapters with thumbnail previews" width="880">

</div>

## Download

**[⬇ Download the latest release](https://github.com/BeFeast/ok-player/releases/latest)** — Windows 11, 64-bit.

- **Installer** — `OkPlayer-Setup-v<version>-win-x64.exe` (self-contained; no .NET or runtime to install).
- **Portable** — `OkPlayer-v<version>-win-x64.zip` (unzip and run).

> OK Player isn’t code-signed yet, so Windows SmartScreen may show a one-time warning on first launch. Click **More info → Run anyway**. (It’s free and [open-source](#license) — you can read or build every line yourself.)

## The four pillars

OK Player is built around four things, ranked — when they conflict, the lower one wins.

### 1. Most elegant design

Native **Fluent/Mica**, macOS-utility-grade refinement — the feel of IINA, Elmedia, or CleanMyMac, on Windows. A single video plane with overlay controls that auto-hide; a proper now-playing card for audio with embedded cover art.

<img src="docs/screenshots/now-playing-audio.png" alt="Audio now-playing card with embedded cover art" width="780">

### 2. Best subtitle UX

Effortless loading (embedded + external), comfortable style presets, dual subtitles, and precise sync — with the on-screen controls lifting captions clear of the controls instead of sitting on top of them. Building toward auto-generated subtitles.

### 3. Beautiful chapters with thumbnails

Chapters as **thumbnail + title + time**, not bare timestamps — plus seek-bar hover previews and user bookmarks. Auto-chapters for files that ship without them.

<img src="docs/screenshots/chapters-eyes-wide-shut.png" alt="Chapter panel with per-chapter thumbnails" width="880">

### 4. Screenshots + precise navigation

Clean and with-subtitles capture, copy-frame-to-clipboard, frame stepping, exact timecode jumps, and frame-accurate readouts — for when a second isn’t precise enough.

## Also in the box

- 🎤 **Time-synced lyrics for audio** — an Apple-Music-style overlay: the current line brightens and auto-scrolls, click any line to seek. Resolved on demand from a local `.lrc` sidecar or [LRCLIB](https://lrclib.net); private sessions stay fully local.
- 🎵 **Music mode** — Up-Next-first panel, “Continue listening” with album art, sidecar cover-art support (the Kodi/Jellyfin/Plex convention).
- 🗂 **Folder-as-playlist** — drop a folder to play it (recursive, natural-sorted), with repeat / shuffle / auto-advance.
- ↪️ **Resume** — picks up where you left off, per file; honours a companion library’s “launch with resume”.
- 🪟 **Window modes** — mini-player / picture-in-picture, always-on-top, fullscreen.
- 🔁 **A–B loop**, ⏯ speed control, 🔊 per-output audio device + loudness normalization.
- 🔒 **Private session** + clear-history + retention controls.
- ⚙️ **Curated simplicity** — smart defaults, with an escape hatch (raw `mpv.conf`) for power users.

<p align="center">
  <img src="docs/screenshots/lyrics-overlay.png" alt="Time-synced lyrics overlay for audio" width="780">
</p>

## Tech

- **UI:** C# / .NET 9, **WinUI 3** (Windows App SDK), **Mica** backdrop
- **Engine:** **libmpv** via the render API (overlay controls over a single video plane)
- **Platform:** Windows 11 only
- **Storage:** human-readable JSON + sidecar files — no database

## Build from source

Requires the .NET 9 SDK and the native libmpv binaries.

```powershell
git clone https://github.com/BeFeast/ok-player.git
cd ok-player
./scripts/fetch-natives.ps1      # fetch the bundled libmpv engine
dotnet build OkPlayer.sln -c Release
```

To produce the installer + portable zip: `./installer/build-installer.ps1`.

## Non-goals

No media library (separate companion app), no DVD/Blu-ray, no IPTV, no casting/DLNA server, no transcoding, no video editing, no upscaling shaders.

## License

**[GPL-3.0-or-later](LICENSE).**

OK Player is built on **mpv / libmpv**, which is copyleft — GPL when built with its full feature set (the standard, hassle-free build). Rather than fight that with an LGPL-only build to chase a permissive license nobody here needs, OK Player is **GPL from the start** — the same choice as [IINA](https://iina.io) (GPLv3) and [mpv.net](https://github.com/mpvnet-player/mpv.net) (GPLv2). You get all of mpv’s capabilities with no build gymnastics; the trade-off is standard copyleft — derivatives and distributed binaries stay GPL.

Other dependencies — Windows App SDK / WinUI 3 (MIT, GPL-compatible) and yt-dlp (invoked as an external tool, Unlicense) — impose no extra obligations. See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

---

*Built on [mpv](https://mpv.io) / libmpv.*
