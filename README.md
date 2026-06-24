# OK Player

> The most elegant media player on Windows — macOS-utility-grade polish, in native Fluent/Mica.

**Status:** 📐 Design phase — PRD locked, UI design next. Not yet implemented.

OK Player is a Windows-native **media player**: a refined GUI over the **mpv** engine (libmpv), the way [IINA](https://iina.io) is on macOS. It is a **pure player** — it opens a file or URL and plays it beautifully. Library management lives in a separate companion app, with clean integration between the two.

## The four pillars

1. **Most elegant design** — native Fluent/Mica, macOS-utility-grade refinement.
2. **Best subtitle UX** — effortless loading, comfortable style presets, outstanding sync (building toward Scribe auto-generated subtitles).
3. **Beautiful chapters with thumbnails** — chapters as *thumbnail + title + time*, not bare timestamps; plus auto-chapters for files without metadata and user bookmarks.
4. **Screenshots + precise navigation** — clean & with-subtitles capture, frame-stepping, timecode jumps, frame-accurate readouts.

## Tech

- **UI:** C# / .NET, **WinUI 3** (Windows App SDK), **Mica** backdrop
- **Engine:** **libmpv** via the render API (overlay controls over a single video plane)
- **Platform:** Windows 11 only
- **Storage:** human-readable JSON + sidecar files — no database

## Docs

- 📋 **[Product Requirements](docs/OK-Player-PRD.md)** — the full spec: pillars, information architecture, every screen and state, roadmap, open questions.
- 🎨 **[Claude Design prompt](docs/claude-design-prompt.md)** — orientation brief / input for the UI design phase.

## Roadmap (summary)

- **MVP** — immersive Mica window, auto-hiding OSC with seek hover-thumbnails, chapter panel + timeline markers + bookmarks, dual screenshots, frame/timecode navigation, A–B loop, subtitles (embedded + external SRT, presets, sync, per-file memory), folder-as-playlist + queue, resume + per-file memory, mini-player/PiP + compact music mode, settings, companion-library launch-with-resume + progress report-back.
- **Day-2** — in-app YouTube (yt-dlp), online subtitle search, **Scribe auto-subtitles**, clip/GIF export.
- **Later** — ASS/PGS/WebVTT, dual subtitles, subtitle search & line-seek, advanced sync, HDR, cross-device sync.

## Non-goals

No media library (separate app), no DVD/Blu-ray, no IPTV, no casting/DLNA server, no transcoding, no video editing, no upscaling shaders.

## License

**[GPL-3.0-or-later](LICENSE).**

OK Player is built on **mpv / libmpv**, which is copyleft — GPL when built with its full feature set (the standard, hassle-free build). Rather than fight that with an LGPL-only build to chase a permissive license nobody here needs, OK Player is **GPL from the start** — the same choice as [IINA](https://iina.io) (GPLv3) and [mpv.net](https://github.com/mpvnet-player/mpv.net) (GPLv2). You get all of mpv's capabilities with no build gymnastics; the trade-off is standard copyleft — derivatives and distributed binaries stay GPL.

Other dependencies — Windows App SDK / WinUI 3 (MIT, GPL-compatible) and yt-dlp (invoked as an external tool, Unlicense) — impose no extra obligations.

---

*Built on [mpv](https://mpv.io) / libmpv.*
