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
sudo apt-get install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libmpv-dev mpv ffmpeg cmake ninja-build
```

Notes:

- GTK4 provides the `GtkGLArea` video host planned for libmpv rendering.
- `libmpv-dev` provides headers and `mpv.pc` for build-time detection.
- `mpv` and `ffmpeg` are runtime tools used by the player and media helpers.
- Hardware decode is intentionally left to mpv configuration first; document VAAPI/VDPAU
  observations when the render spike lands.

## Current Checks

```bash
cargo test --workspace
cargo run -p okp-linux-gtk
```

## Linux Packaging

Two package lanes are intentionally separate:

```bash
./scripts/package-linux-velopack.sh 0.1.0-linux-alpha.1
./scripts/package-linux-deb.sh 0.1.0-linux-alpha.1
```

- Velopack lane: AppImage + `releases.linux.json` feed for self-updating GitHub Releases.
- Debian lane: `.deb` for Debian/Ubuntu-family install flow. This does not use in-app self-update;
  a future APT/Flatpak lane should own package-manager updates.
