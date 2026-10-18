# Arch Linux and Omarchy

OK Player's Linux shell builds and runs on Arch Linux, and therefore on
[Omarchy](https://omarchy.org) (Arch + Hyprland). There is no binary package
lane for Arch yet: you build from source, either straight from a checkout with
cargo or as a pacman package from the PKGBUILD in
[`rust/packaging/arch/`](../rust/packaging/arch/PKGBUILD).

Either way the binary links the distro's `mpv` (`libmpv.so.2`), the way the
Fedora package does; nothing is bundled. Hyprland is Wayland, so the native
Wayland/EGL video plane is the one that runs. The playback behaviours that are
validated only on GNOME/Wayland (drag/drop, portals, compositor fullscreen —
see [Supported environments](../README.md#supported-environments)) are not
yet validated on Hyprland: file a report if one misbehaves.

## Dependencies

```bash
sudo pacman -S --needed gtk4 mpv libx11 wayland wayland-protocols libglvnd \
  glib2-devel pkgconf rust cargo git xdg-utils
```

- `gtk4`, `mpv`, `libx11`, `wayland`, `libglvnd` are the runtime libraries the
  binary links; `wayland-protocols`, `glib2-devel` (for
  `glib-compile-resources`, which builds the chrome icon set into the binary),
  `pkgconf`, `rust`, `cargo` are build-time only.
- Optional: `yt-dlp` (site URLs), `ffmpeg` (clip export and screenshots).
- Fonts: the interface asks for **Noto Sans** and **JetBrains Mono** and falls
  back to the system sans-serif and monospace when they are absent. On Omarchy
  that fallback is the Cascadia/Caskaydia family, which reads fine; install
  `noto-fonts` and `ttf-jetbrains-mono` to see the typography the design
  specifies.

## Build and run from a checkout

```bash
git clone https://github.com/BeFeast/ok-player.git
cd ok-player/rust
OKP_REQUIRE_SYSTEM_MPV=1 cargo run --release -p okp-linux-gtk -- /path/to/movie.mkv
```

`OKP_REQUIRE_SYSTEM_MPV=1` makes the build refuse a source-tree libmpv and
link the system one; a plain `cargo run` works too and picks the system
library when no bundled one is present. A `cargo run` build identifies itself
as a development build: About says so, and updates are disabled rather than
pretended.

To install the development build for the desktop without packaging it:

```bash
cargo install --path crates/okp-linux-gtk --root ~/.local   # ~/.local/bin/okp-linux-gtk
```

and point a copy of `rust/packaging/linux/com.befeast.okplayer.desktop`
(`Exec=` adjusted) in `~/.local/share/applications/` at it.

## Build a pacman package

```bash
cd rust/packaging/arch
makepkg -si            # builds the tagged release named in the PKGBUILD
```

To package the branch you have checked out rather than the tag:

```bash
_okp_source="$(git rev-parse --show-toplevel)" makepkg -si
```

The working tree is cloned, so commit what you want built. The PKGBUILD mirrors
the Fedora spec: `/usr/bin/ok-player`, the desktop entry, MIME declarations,
AppStream MetaInfo and icon set from the shared Linux packaging directory, and
the licence documents under `/usr/share/licenses/ok-player/`. `check()` runs
the workspace tests plus `desktop-file-validate` and `appstreamcli validate`;
`makepkg --nocheck` skips them.

The package stamps the binary with `OKP_PACKAGE_KIND=arch`. Inside the app that
is the **pacman** install kind: About reports `Install: pacman`, the updater
offers nothing it cannot deliver, and when a newer release exists Settings →
Updates says to update with pacman. The app never downloads or installs a
package of its own on Arch.

## What is not done here

- No AUR submission and no prebuilt package: the PKGBUILD is the recipe, and
  `makepkg` on an Arch host is the verification. It has not been run on a
  machine the maintainers control; please report the first result.
- No Hyprland-specific acceptance evidence yet. The Linux release matrix
  ([`linux-release-acceptance.md`](linux-release-acceptance.md)) is graded on
  GNOME/Wayland and Xvfb; Omarchy rows will be added as they are observed.
