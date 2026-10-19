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
sudo pacman -S --needed base-devel gtk4 mpv libx11 wayland wayland-protocols \
  libglvnd pkgconf rust cargo git xdg-utils
```

- `gtk4`, `mpv`, `libx11`, `wayland`, `libglvnd` are the runtime libraries the
  binary links; `base-devel` (the build script compiles the native Wayland
  video plane and the generated protocol glue with the system C compiler),
  `wayland-protocols`, `pkgconf`, `rust`, `cargo` are build-time only.
  `glib-compile-resources`, which the build runs to compile the chrome icon
  set into the binary, ships in `glib2`, which `gtk4` depends on. `makepkg`
  assumes `base-devel` as well, which is why the PKGBUILD does not list a
  compiler.
- Optional: `yt-dlp` (site URLs), `ffmpeg` (clip export and screenshots).
- Fonts: the interface asks for **Noto Sans** and **JetBrains Mono** and falls
  back to the system sans-serif and monospace when they are absent. `fc-match
  'Noto Sans'` and `fc-match 'JetBrains Mono'` tell you what your install
  resolves them to; `noto-fonts` and `ttf-jetbrains-mono` provide the faces the
  design specifies if they are not already there.

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
makepkg -si            # builds the checkout you are in
```

The working tree is cloned, so commit what you want built, on a branch: a
detached HEAD has nothing for the clone to check out. To build another
checkout, name it: `_okp_source=/path/to/checkout makepkg -si`. To build a
published Linux release instead, `_okp_release=0.12.0 makepkg -si` fetches the
`linux-v0.12.0` tag — once a release carries this lane. No release does yet
(the first will be noted here), and the recipe refuses releases that predate
it, because they lack the files it installs and the pacman detection the app
relies on. The package is versioned
by what it is, so `0.1.0linuxalpha.112.r7.gabc1234` to pacman and
`0.1.0-linux-alpha.112+7.gabc1234` in About, and the updater compares that,
not the pinned release. With a release tag in reach that is the nearest
`linux-v*` tag and the commits since it, and a checkout exactly at a release
tag is that release. Releases are tagged on their own commits, off `main`, so
a checkout of `main` has none in reach: it is stamped with the newest
`linux-v*` tag the clone knows plus its own commit count, which still orders
after that release for pacman and ties with it for the updater, so a newer
release is offered and the release itself is not. `makepkg` writes its
mirror, `src/`, `pkg/` and the finished `.pkg.tar.zst` next to the PKGBUILD
unless `SRCDEST`/`BUILDDIR`/`PKGDEST` say otherwise (they are ignored by git).
The PKGBUILD mirrors the Fedora spec: `/usr/bin/ok-player`, the desktop entry,
MIME declarations, AppStream MetaInfo and icon set from the shared Linux
packaging directory, and the licence documents under
`/usr/share/licenses/ok-player/`. `check()` runs the workspace tests plus
`desktop-file-validate` and `appstreamcli validate`; `makepkg --nocheck` skips
them.

An installed package is recognised at runtime: the app asks `pacman -Qo` who
owns its executable, the same probe the Debian and Fedora packages rely on with
`dpkg` and `rpm`, and pacman owning `/usr/bin/ok-player` is the **pacman**
install kind. About reports `Install: pacman`. The app still checks the Linux
release feed, and when a newer release exists Settings → Updates names it and
says how to get it: pull the checkout and rebuild from the PKGBUILD (`git
pull`, then `makepkg -si` - the recipe builds the checkout as it stands, so
rebuilding without pulling rebuilds the commit already running), or build that
release by name (`_okp_release=<version> makepkg -si`). No pacman repository
carries OK Player yet, so "update with pacman" would point nowhere. It never
downloads or installs a package of its own on Arch, and it follows releases
only: the rolling candidate channel publishes `.deb` and AppImage builds that
a PKGBUILD pinned to a release tag cannot reproduce, so a pacman install reads
the release feed whatever update channel is configured, and Settings → Updates
says so. (`OKP_PACKAGE_KIND=arch` stamps the
build with the lane it came from; it is recorded in the update log, not
consulted. Run the binary from an extracted package rather than an installed
one and pacman does not own it, so it is not that kind.)

## What is not done here

- No AUR submission and no prebuilt package: the PKGBUILD is the recipe, and
  `makepkg` on an Arch host is the verification. It has not been run on a
  machine the maintainers control; please report the first result.
- No Hyprland-specific acceptance evidence yet. The Linux release matrix
  ([`linux-release-acceptance.md`](linux-release-acceptance.md)) is graded on
  GNOME/Wayland and Xvfb; Omarchy rows will be added as they are observed.
