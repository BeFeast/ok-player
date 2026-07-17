# Linux Flatpak beta lane

OK Player's Flatpak is a beta packaging lane. It is not a claim of Flathub
availability; that claim can be made only after the external Flathub submission
is accepted.

## Runtime and source policy

The manifest uses GNOME Platform/SDK 50, which is based on the maintained
Freedesktop 25.08 line. Update the GNOME runtime each supported GNOME cycle and
never keep a branch after upstream or Flathub marks it end-of-life. Runtime
updates must keep the matching Freedesktop Rust SDK extension and codec/VAAPI
extension branches aligned.

`libmpv` 0.41.0, libplacebo 7.360.1, and libass 0.17.5 are built from pinned,
redistributable upstream sources. Cargo dependencies are expanded from
`rust/Cargo.lock` into `rust/packaging/flatpak/cargo-sources.json`; every crate
has a checksum and Cargo runs with `--offline --locked`. The build script first
downloads declared sources and then rebuilds with `--disable-download`, so an
undeclared fetch fails.

The package installs the project GPL license, third-party notices, and the
upstream mpv/libplacebo/libass license texts under the Flatpak license prefix.
The manifest builds GPL-enabled libmpv against the runtime FFmpeg libraries,
which is compatible with OK Player's GPL-3.0-or-later license.

## Codecs and hardware acceleration

The manifest mounts `org.freedesktop.Platform.codecs-extra//25.08-extra` ahead
of the base runtime libraries. With the extension installed, libmpv sees the
expanded codec set. If a user or distributor masks/removes the extension, OK
Player continues with the codecs in the base runtime; unavailable patented
formats fail explicitly through the normal playback error surface rather than
silently downloading codecs.

Hardware decoding receives only `--device=dri`. Mesa drivers come from the
runtime/host GL extension, and the optional Freedesktop Intel VAAPI extension is
enabled only when Flatpak detects a matching Intel GPU. Renderer diagnostics
remain the acceptance source for the active decoder; unsupported or hidden
devices fall back through libmpv's `auto-safe` policy.

## Sandbox permissions

The package requests:

- Wayland plus fallback X11 and shared IPC for GTK/libmpv presentation.
- PulseAudio compatibility and read-only PipeWire socket access for audio.
- Network access for user-requested URLs and external links.
- DRI access for GPU rendering and hardware decode.
- Write access to Pictures for the default `Pictures/OK Player` screenshot
  destination.
- Ownership of `org.mpris.MediaPlayer2.okplayer` for MPRIS.

There is no blanket home or host filesystem permission. File and folder open,
subtitle selection, custom screenshot folders, and drag/drop rely on GTK/GIO
portals and document grants. Clipboard access needs no additional Flatpak
permission. Flatpak owns application updates; the in-app AppImage/.deb updater
is disabled and Settings reports the install as Flatpak-managed.

## Build and beta repository

Install the SDKs once:

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y flathub \
  org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08
```

Then validate and build the offline beta repository:

```sh
./scripts/smoke-linux-flatpak.sh
./scripts/build-flatpak-beta.sh
```

Install, update, and uninstall from that repository:

```sh
flatpak remote-add --user --if-not-exists ok-player-beta \
  artifacts/linux/flatpak/repo
flatpak install --user -y ok-player-beta com.befeast.okplayer//beta
flatpak update --user -y com.befeast.okplayer
flatpak uninstall --user -y com.befeast.okplayer
```

To test the masked-codec state without changing the manifest, temporarily mask
the extension, run the codec acceptance fixtures, then undo the mask:

```sh
flatpak mask --user org.freedesktop.Platform.codecs-extra
flatpak run com.befeast.okplayer
flatpak mask --user --remove org.freedesktop.Platform.codecs-extra
```

## Acceptance boundary

CI proves manifest validity, source pinning, an offline build, export, and
bundle creation. It does not prove a real GNOME/KDE chooser, drag/drop,
clipboard, compositor, portal, PipeWire session, MPRIS consumer, or hardware
decoder. Before the beta lane is accepted, an operator must run the issue #345
matrix on fresh GNOME and KDE Wayland installs, with codecs-extra present and
masked, and record install/update/uninstall plus renderer diagnostics. The PR
must remain work-in-progress until that live evidence exists.
