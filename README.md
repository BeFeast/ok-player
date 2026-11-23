# OK Player

> The most elegant media player for Windows and Linux — macOS-utility-grade polish, native on each platform.

**Status:** 🟢 Implemented and shipping. **Windows** ships a native WinUI 3 line (latest pre-1.0 release `v0.10.14`). **Linux** ships a native GTK4 line, now entering **public beta** with `0.11.0-beta.1` as the first beta build.

OK Player is a **media player**: a refined GUI over the **mpv** engine (libmpv), the way [IINA](https://iina.io) is on macOS. It is a **pure player** — it opens a file or URL and plays it beautifully. Library management lives in a separate companion app, with clean integration between the two.

Each platform gets a **native shell** — WinUI 3 on Windows, GTK4/Relm4 on Linux — over the same libmpv engine, sharing a growing pure-Rust core (`okp-core`) for schema, model, and playback logic. There is no cross-platform UI toolkit and no shared widget layer; the shells stay native while the portable behavior moves into the core.

## Builds at a glance

| Platform | Shell / toolkit | Line | Latest | Maturity |
|---|---|---|---|---|
| Windows 11 | C# / .NET, WinUI 3 (Windows App SDK), Mica | `v*` | `v0.10.14` | Shipping, pre-1.0 |
| Linux (X11 / Wayland) | Rust, GTK4 / Relm4 | Forgejo AppImage | Signed Beta and Stable channels | Public beta |

- **Windows** installs and self-updates through Velopack; discovery uses a static HTTPS feed ([`docs/update-feed.md`](docs/update-feed.md)).
- **Linux** ships as an x86_64 AppImage. See [Install on Linux](#install-on-linux).
- Every release is a **pre-1.0 build**: expect rapid iteration and occasional breaking changes until 1.0.

## The four pillars

1. **Most elegant design** — native per platform (Fluent/Mica on Windows, GTK4 on Linux), macOS-utility-grade refinement.
2. **Best subtitle UX** — effortless loading, comfortable style presets, outstanding sync (building toward Scribe auto-generated subtitles).
3. **Beautiful chapters with thumbnails** — chapters as *thumbnail + title + time*, not bare timestamps; plus auto-chapters for files without metadata and user bookmarks.
4. **Screenshots + precise navigation** — clean & with-subtitles capture, frame-stepping, timecode jumps, frame-accurate readouts.

## Tech

- **Shared core:** `okp-core` — pure Rust schema/model/playback logic shared by every shell; portable behavior lives here, not in the shells.
- **Engine:** **libmpv** on both platforms — the render API on Windows (overlay controls over a single video plane) and a native Wayland/EGL plane with a `GtkGLArea` fallback on Linux.
- **Windows shell:** C# / .NET, **WinUI 3** (Windows App SDK), **Mica** backdrop.
- **Linux shell:** **Rust**, **GTK4 / Relm4**; native Wayland/EGL video with an X11 path.
- **Storage:** human-readable JSON + sidecar files — no database (shared schemas in `okp-core`).

## Install

### Windows

Download the latest installer from [Releases](https://github.com/BeFeast/ok-player/releases) (`v*` tags). Installed builds self-update through the static feed; no store or manual re-download needed.

### Install on Linux

Install the signed AppImage under your user account. The launcher registers media types and the
`ok-player://` URI handler. Install Python 3, GnuPG, `flock`, and `xdg-mime` first.

```bash
git clone https://git.oklabs.uk/BeFeast/ok-player.git
cd ok-player
./scripts/install-linux-appimage.sh beta
~/.local/bin/ok-player
```

Use `~/.local/bin/ok-player --update beta` for later updates. Choose `stable` when a Stable build
has been promoted. See [Linux AppImage delivery](docs/linux-appimage.md) for runtime requirements,
signed manifests, and rollback.

### Supported environments

The AppImage uses the Debian 13 glibc baseline and runs on supported x86_64 GTK4 desktops. Linux acceptance is graded by evidence level (model-unit, headless Xvfb render, installed-package, and live GNOME/Wayland operator) — see [`docs/linux-release-acceptance.md`](docs/linux-release-acceptance.md). Rows that require a real desktop (file/folder chooser, drag/drop, clipboard, portals, compositor fullscreen, keyboard focus) are only ever marked passing by the live operator level; headless runs leave them unverified. The distro/session/package matrix for a given release is recorded in that release's notes.

## Docs

- 📋 **[Product Requirements](docs/OK-Player-PRD.md)** — the full spec: pillars, information architecture, every screen and state, roadmap, open questions. (Authored as the Windows product spec; the Linux shell tracks the same behavior natively.)
- 📦 **[Linux release acceptance](docs/linux-release-acceptance.md)** — how Linux release evidence is graded and what each level may claim.
- 📦 **[Linux AppImage delivery](docs/linux-appimage.md)** — install, verify, update, and roll back.
- ✅ **[QA acceptance records](docs/qa-records/README.md)** — the required reviewable record for QA-only and acceptance-only pull requests.
- 🔄 **[Update feeds](docs/update-feed.md)** — how installed Windows and Linux builds discover updates.
- 📝 **[Release notes template — `0.11.0-beta.1`](docs/release-notes/0.11.0-beta.1.md)** — the presentation template for the first Linux public beta.
- 🖥️ **[Windows development VM](docs/windows-dev-vm.md)** — reproducible VM bootstrap, environment report, and the VM-valid vs. real-hardware acceptance gates.
- 🎨 **[Claude Design prompt](docs/claude-design-prompt.md)** — original orientation brief from the UI design phase.

## Roadmap (summary)

- **Shipped** — immersive native window, auto-hiding OSC with seek hover-thumbnails, chapter panel + timeline markers + bookmarks, dual screenshots, frame/timecode navigation, A–B loop, subtitles (embedded + external, presets, sync, per-file memory), editable Up Next queue (add/play-next/reorder/remove/clear) + `.m3u`, resume + per-file memory, compact music mode, Settings, companion-library launch-with-resume + progress report-back, and native OS media controls (Windows SMTC / Linux MPRIS). Linux adds native Wayland/EGL 4K60 presentation and image-based (PGS/VobSub) subtitle labeling.
- **In progress** — Linux/Windows parity across the remaining player surfaces (tracked per issue).
- **Day-2** — in-app YouTube (yt-dlp), online subtitle search, **Scribe auto-subtitles**, clip/GIF export.
- **Later** — deeper ASS/PGS/WebVTT, dual subtitles, advanced sync, HDR, cross-device sync.

## Non-goals

No media library (separate app), no DVD/Blu-ray, no IPTV, no casting/DLNA server, no transcoding, no video editing, no upscaling shaders.

## License

**[GPL-3.0-or-later](LICENSE).**

OK Player is built on **mpv / libmpv**, which is copyleft — GPL when built with its full feature set (the standard, hassle-free build). Rather than fight that with an LGPL-only build to chase a permissive license nobody here needs, OK Player is **GPL from the start** — the same choice as [IINA](https://iina.io) (GPLv3) and [mpv.net](https://github.com/mpvnet-player/mpv.net) (GPLv2). You get all of mpv's capabilities with no build gymnastics; the trade-off is standard copyleft — derivatives and distributed binaries stay GPL.

Other dependencies — Windows App SDK / WinUI 3 (MIT, GPL-compatible), GTK4 (LGPL), and yt-dlp (invoked as an external tool, Unlicense) — impose no extra obligations.

---

*Built on [mpv](https://mpv.io) / libmpv.*
