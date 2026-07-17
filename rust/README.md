# OK Player Rust Workspace

This workspace is the Linux-first foundation for the shared OK Player core.

## Crates

- `okp-core` — platform-neutral player logic, parsers, schemas, and state machines.
- `okp-mpv` — libmpv control/render API wrapper boundary.
- `okp-linux-gtk` — GTK4/Relm4 Linux shell.
- `okp-ffi` — future C ABI surface for the WinUI shell.

The Windows WinUI app remains in `src/` while Linux and the shared Rust core grow under
`rust/`.

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

## Companion launch contract

The Linux shell accepts the PRD §13.1 handoff through ordinary process invocation:

```bash
okp-linux-gtk /media/movie.mkv --resume 1:23:45 --sub 2 --audio 1
```

- `--resume` accepts seconds or a timecode and overrides remembered resume for that open only.
- `--sub` and `--audio` accept a 1-based mpv track id, or `off`/`no`.
- `--sub /media/captions.srt` remains supported for loading an external subtitle file.
- Progress and watched transitions feed a local pluggable sink. The MVP sink is intentionally a
  no-op until a companion transport is configured; private sessions suppress both that channel and
  history writes. No database or remote endpoint is involved.

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

Three package lanes are intentionally separate:

```bash
./scripts/package-linux-velopack.sh 0.1.0-linux-alpha.1
./scripts/package-linux-deb.sh 0.1.0-linux-alpha.1
./scripts/package-linux-rpm.sh --version 0.11.0-beta.1
```

- Velopack lane: AppImage + `releases.linux.json` feed for self-updating GitHub Releases.
- Candidate packaging sets `OKP_LINUX_CHANNEL=linux-candidate`; `okp-core` resolves the generated
  channel-qualified AppImage, Full nupkg, and feed names, verifies their hashes/sizes, and stages
  the versioned AppImage atomically. Run the real CLI contract test with
  `OKP_RUN_VELOPACK_PACK_TEST=1 cargo test -p okp-linux-gtk real_velopack_pack` after installing
  Velopack CLI 1.2.0 and `squashfs-tools`.
- Debian lane: `.deb` for Debian/Ubuntu-family install flow. In-app checks can download the
  newest GitHub Release `.deb`, request admin approval via `pkexec apt-get install -y`, and
  fall back to opening the installer when privileged install is unavailable.
- Fedora lane: reproducible vendored SRPM plus a native RPM linked to Fedora's
  system `mpv-libs`. Fedora 43/44 clean-chroot, rpmlint, install/upgrade/remove,
  and config-preservation checks are documented in `docs/fedora-rpm.md`.

Linux publishing is a two-phase gate. A non-publish workflow run builds the candidate and writes
`package-identity.json` plus `acceptance-template.json`. After installed-package and live
GNOME/Wayland rows are recorded, a publish run downloads that exact candidate run and validates
the completed manifest before creating the release. See `docs/linux-release-acceptance.md`.
