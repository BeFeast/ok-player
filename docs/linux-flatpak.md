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
redistributable upstream sources. The application source is pinned to a
permanent `main` commit and receives the checked-in Flatpak integration patch,
so the manifest can move directly to the external Flathub repository without a
branch or local-directory source. The manifest smoke check regenerates that
patch from the pinned commit and compares it byte-for-byte with the checked-in
copy, preventing the packaged source from drifting from the reviewed Rust
files. Cargo dependencies are expanded from
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
formats fail immediately through the normal playback error surface rather than
silently advancing an audio clock behind a video track with no presented
frames. The diagnostic names the matching `codecs-extra` extension and playback
is stopped until the user installs codec support or opens another source.

Hardware decoding receives only `--device=dri`. Mesa drivers come from the
runtime/host GL extension, and the optional Freedesktop Intel VAAPI extension is
enabled only when Flatpak detects a matching Intel GPU. Renderer diagnostics
remain the acceptance source for the active decoder; unsupported or hidden
devices fall back through libmpv's `auto-safe` policy.

If the Flatpak starts without an accessible `/dev/dri` node, OK Player selects
libmpv's CPU software render API before GTK initializes. Frames are rendered
directly into an RGB Cairo image surface and painted by a GTK DrawingArea, so
the fallback does not depend on EGL, GLX, Vulkan, VAAPI, or a GPU device. That
launch forces `hwdec=no`, selects libmpv's `sw` render API, and uses GTK's
Cairo scene renderer. Normal DRI launches receive none of those overrides and keep
the native Wayland/EGL path. Startup diagnostics record the selected renderer
policy, libmpv software backend, pixel format, and GTK scene renderer so a live
acceptance run can distinguish real CPU presentation from audio-only playback
behind a black surface.

## Sandbox permissions

The package requests:

- Wayland plus fallback X11 and shared IPC for GTK/libmpv presentation.
- PulseAudio compatibility and read-only PipeWire socket access for audio.
- Network access for user-requested URLs and external links.
- DRI access for GPU rendering and hardware decode.
- Write access to Pictures for the default `Pictures/OK Player` screenshot
  destination.
- Ownership of `org.mpris.MediaPlayer2.okplayer` for MPRIS.

The native Wayland renderer enables libmpv's advanced-control contract, so
GPU-backed clean and subtitled screenshots are captured on the dedicated render
thread instead of falling back to an unsupported hardware-frame software
download. libmpv encodes saved screenshots in sandbox-private temporary
storage. The application validates the output, copies it to a destination-local
staging file, and atomically publishes it under Pictures. This keeps both clean
and subtitled capture modes independent of the external mount's create and
rename behavior without widening filesystem permissions.

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
  org.gnome.Platform//50 \
  org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08
```

Then validate and build the offline beta repository:

```sh
./scripts/smoke-linux-flatpak.sh
./scripts/build-flatpak-beta.sh
```

The build output contains two repository views and two bundles:

- `repo-baseline` exposes only `0.11.0-beta.0`, so a fresh machine can install
  version N instead of silently receiving the latest commit.
- `repo` starts with that exact baseline commit and adds `0.11.0-beta.1` as its
  direct child. `flatpak-beta-artifact.json` records both OSTree commits, their
  parent relationship, the exact source commit stamped into the update build,
  and both bundle SHA-256 values without recording a host path, hostname, URL,
  or credential.
- `artifacts/manual-ui/linux-software-renderer-smoke` contains the packaged
  no-DRI mapped-window evidence: full-window and cropped screenshots, sanitized
  renderer/session logs, presentation samples, `xwininfo` map-state output, and
  `results.json` with `IsViewable`, non-trivial geometry, zero DRI descriptors,
  renderer identity, pixel measurements, and screenshot SHA-256 values.

Exercise the repository lifecycle from the extracted CI artifact. Resolve the
local repository directories to `file://` URLs at runtime; do not paste those
machine-specific URLs into public evidence:

```sh
baseline_repo_url="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve().as_uri())' repo-baseline)"
update_repo_url="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve().as_uri())' repo)"
baseline_commit="$(python3 -c 'import json; print(json.load(open("flatpak-beta-artifact.json"))["baseline"]["ostree_commit"])')"
update_commit="$(python3 -c 'import json; print(json.load(open("flatpak-beta-artifact.json"))["update"]["ostree_commit"])')"

flatpak remote-add --user --no-gpg-verify ok-player-beta "$baseline_repo_url"
flatpak install --user -y ok-player-beta com.befeast.okplayer//beta
test "$(flatpak info --user --show-commit com.befeast.okplayer)" = "$baseline_commit"
flatpak run com.befeast.okplayer

flatpak remote-modify --user --url="$update_repo_url" ok-player-beta
flatpak update --user -y com.befeast.okplayer
test "$(flatpak info --user --show-commit com.befeast.okplayer)" = "$update_commit"
flatpak run com.befeast.okplayer

flatpak update --user -y --commit="$baseline_commit" com.befeast.okplayer
test "$(flatpak info --user --show-commit com.befeast.okplayer)" = "$baseline_commit"
flatpak run com.befeast.okplayer

# Restore the current beta after rollback acceptance.
flatpak update --user -y com.befeast.okplayer
test "$(flatpak info --user --show-commit com.befeast.okplayer)" = "$update_commit"
flatpak uninstall --user -y com.befeast.okplayer
flatpak remote-delete --user ok-player-beta
```

The three `flatpak run` commands are operator steps: confirm a rendered window
and working playback/audio before closing each launch. A command returning zero
does not attest a real compositor, portal, focus, clipboard, drag/drop, chooser,
or PipeWire session.

Create the machine-readable lifecycle template from an exact PR checkout and
the downloaded CI artifact hash:

```sh
cargo run --locked --manifest-path rust/Cargo.toml \
  -p okp-core --bin okp-acceptance-evidence -- \
  flatpak-lifecycle-template \
  --artifact-manifest flatpak-beta-artifact.json \
  --pull-request-head "$(git rev-parse HEAD)" \
  --downloaded-artifact-sha256 "$downloaded_artifact_sha256" \
  --desktop gnome \
  > flatpak-lifecycle-evidence.json
```

Generate and complete a separate record with `--desktop kde` for the KDE
Wayland run. Set a step to `pass` only after its command, applicable deployed
commit assertion, and applicable live launch pass. The `uninstall` and
`remote-cleanup` steps intentionally keep `deployed_commit` as `null`, proving
that cleanup does not claim an installed application revision. Validate each
completed record with `flatpak-lifecycle-validate --manifest
flatpak-lifecycle-evidence.json`. The schema has no host identity, path, URL,
credential, or free-form note fields.

To test the masked-codec state without changing the manifest, temporarily mask
the extension, run the codec acceptance fixtures, then undo the mask:

```sh
flatpak mask --user org.freedesktop.Platform.codecs-extra
flatpak run com.befeast.okplayer
flatpak mask --user --remove org.freedesktop.Platform.codecs-extra
```

## Acceptance boundary

CI proves manifest validity, source pinning, two offline builds, a direct
baseline-to-update OSTree history, repository export, bundle creation, and
portable artifact identity. The packaged no-DRI smoke removes DRI from the app,
requires the libmpv CPU software backend and Cairo scene renderer, opens a
moving red fixture through the production command-line media path, and requires
a mapped GTK player top-level owned by the application process with zero open
`/dev/dri` descriptors. CI initializes an explicit XDG Pictures directory before
installing the bundle so the fixture uses the package's real `xdg-pictures`
grant even on fresh runners without `user-dirs.dirs`. The smoke records
advancing playback positions, requires `xwininfo` to report `IsViewable` with
non-trivial geometry, captures that mapped X11 window, crops the calculated
video region, and requires a substantial nonblack, red-dominant pixel
population plus visible frame-to-frame change. The public artifact includes
the sanitized logs, map-state output, machine-readable measurements, and
checksummed screenshots. An offscreen probe is never accepted as mapped-window
or visible-video evidence. CI still does not
prove a real GNOME/KDE chooser,
drag/drop, clipboard, portal, PipeWire session, MPRIS consumer, or hardware
decoder. Before the beta lane is accepted, an operator must run the issue #345
matrix on fresh GNOME and KDE Wayland installs, with codecs-extra present and
masked, with normal DRI and DRI removed, and record
baseline install/launch, current update/launch, baseline rollback/launch,
current restore, uninstall, and beta-remote cleanup plus renderer diagnostics
and visible playback evidence.
PR #388 must remain draft until the nine required lifecycle steps pass on its
exact head and the remaining live-desktop matrix is posted.
