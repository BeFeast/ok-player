# OK Player Rust Workspace

This workspace is the Linux-first foundation for the shared OK Player core.

## Crates

- `okp-core` — platform-neutral player logic, parsers, schemas, and state machines.
- `okp-mpv` — libmpv control/render API wrapper boundary.
- `okp-linux-gtk` — GTK4/Relm4 Linux shell.
- `okp-ffi` — future C ABI surface for the WinUI shell.

The Windows WinUI app remains in `src/` while Linux and the shared Rust core grow under
`rust/`.

## Streaming and YouTube URLs

The GTK shell's **Open URL** entry point plays any direct stream mpv understands
(`http(s)`, `smb`, `rtsp`, …) and recognizes YouTube links. YouTube playback
rides mpv's `ytdl_hook`, which shells out to a resolver (`yt-dlp`, or the older
`youtube-dl`); when none is on `PATH`, the dialog says so instead of handing mpv
a link it would silently fail to open. Install one to enable YouTube links:

```bash
sudo apt-get install yt-dlp   # or: pipx install yt-dlp
```

This is the PRD §10.2 **Day-2** reservation: a clean entry point and a URL field
that recognizes YouTube links — deliberately **not** an in-app browser or search
UI. The classification and outcome rules live in `okp-core` (`youtube` module);
the shell only probes the host and renders the result.

## Linux Dependencies

Ubuntu/Debian-family development packages:

```bash
sudo apt-get install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libmpv-dev libgl1-mesa-dev libegl1-mesa-dev libglx-dev mpv ffmpeg cmake ninja-build
```

Notes:

- GTK4 provides the `GtkGLArea` video host planned for libmpv rendering.
- `libmpv-dev` provides headers and `mpv.pc` for build-time detection.
- `libgl1-mesa-dev`, `libegl1-mesa-dev`, and `libglx-dev` provide the OpenGL
  symbols used by the first `GtkGLArea` render spike.
- `mpv` and `ffmpeg` are runtime tools used by the player and media helpers.
- Hardware decode is intentionally left to mpv configuration first; document VAAPI/VDPAU
  observations when the render spike lands.

## Current Checks

All three gates run in CI (`.github/workflows/rust.yml`) on every pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p okp-linux-gtk
```

## UI-Thread Blocking-Read Guard

A synchronous mpv property read on the thread that drives the UI can deadlock
against a briefly-busy core — the freeze class from the Windows #33 postmortem,
guarded there by the DEBUG render-thread guard in `MpvContext`. The Rust twin
lives in `okp-mpv`: the GTK shell calls `Mpv::mark_ui_thread()` on the GLib
main context at attach time, and debug builds then hard-log every blocking
property read from that thread with a backtrace (deduplicated per read shape).

Decision: the guard logs loudly instead of aborting, because the GTK shell
still has known synchronous read sites (the 200 ms state poll and the popover
builders) whose migration to the observe/event path is tracked in the
core-extraction epic. Aborting today would kill every debug run at the first
poll tick; each logged backtrace is a call site on that epic's worklist.
Release builds compile the guard out entirely.

## Linux Packaging

Two package lanes are intentionally separate:

```bash
./scripts/package-linux-velopack.sh 0.1.0-linux-alpha.1
./scripts/package-linux-deb.sh 0.1.0-linux-alpha.1
```

- Velopack lane: AppImage + `releases.linux.json` feed for self-updating GitHub Releases.
- Debian lane: `.deb` for Debian/Ubuntu-family install flow. In-app checks can download the
  newest GitHub Release `.deb`, request admin approval via `pkexec apt-get install -y`, and
  fall back to opening the installer when privileged install is unavailable.
