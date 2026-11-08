# macOS developer preview

The first Apple Silicon shell is a small native AppKit window over the same Rust
player command/event state machine and libmpv engine as the Linux player. It opens
local videos through **File → Open**, the Open button, a file argument, or Finder's
Open With action. **Space** and the transport button toggle pause. Closing the
window stops playback and releases the renderer before destroying its GL context.
Errors stay visible until another explicit open attempt.

This preview is for local testing. It does not yet contain the Linux shell's URL
resolver, History, saving/cache UI, subtitle controls or update service. Its chrome
uses the shared BeFeast palette in light and dark, following the system appearance;
the full macOS UI redesign is still ahead. The app is ad-hoc signed; Developer ID signing and notarization remain
separate distribution work. No global libmpv or package-manager installation is
needed to run the packaged app.

## Build and artifact

The manual Forgejo `macOS player preview` workflow uses an existing macOS runner
with Rust, Swift and the Apple SDK. It accepts an exact source SHA, a numeric-IP
Forgejo endpoint and an existing toolchain location. It downloads the checksum-pinned
ARM64 runtime and H.264/AAC fixture from Forgejo generic packages, without installing
system tools. `scripts/build-macos-preview.sh` builds the opt-in `live-mpv` FFI,
compiles this shell against the generated header, bundles the entire dylib closure,
rewrites library locations, and signs the app ad hoc.

## Colours

The window chrome takes every colour from `brand/palette.json` at the repository
root, the palette all three shells share. Before compiling, the build script runs
`scripts/generate-macos-palette.py`, which turns each semantic role into a case of a
generated Swift `PaletteRole` with its light and dark sRGB value; `Player.swift` keeps
no colour values of its own. Each role becomes a dynamic `NSColor`, so the window
follows the system light/dark switch while it is open:

| Element | Role |
|---|---|
| Window background | `canvas` |
| Control bar and its top edge | `surface`, `borderSubtle` |
| Open and transport buttons | `accent` fill, `onAccent` label |
| Keyboard focus ring on the buttons | `focus` |
| Playback progress line on the control bar | `accent` |
| Status text: hint, time, error | `textMuted`, `text`, `danger` |

The video surface gets no tint. A malformed palette or a role missing from one
appearance fails the build. `scripts/tests/macos-palette.Tests.py` checks that the
generated table carries the JSON values.

The workflow uploads `ok-player-macos-arm64.zip`, source identity, SHA-256, compiler
logs, dylib/rpath/signature inspection and smoke output. With smoke enabled it also
uploads `snapshots/window-light.png` and `window-dark.png`: after the playback check
the app renders its own window content in each appearance, overriding only that
window's appearance. The OpenGL video area is not part of that render and stays empty. Extract the ZIP and open
**OK Player.app** from the extracted directory. The app does not require
`DYLD_LIBRARY_PATH` or a fixed installation path.

The pinned runtime comes from media-kit/libmpv-darwin-build v0.7.2, containing
mpv 0.36 / FFmpeg 6 and ARM64 OpenGL, CoreAudio and VideoToolbox support. Its embedded
`provenance.json` records upstream source and archive hashes. This runtime has no
Lua or bundled yt-dlp, which is why the preview is scoped to local-file playback.

## GitHub-hosted build

`.github/workflows/macos-player-preview.yml` builds the same app on a GitHub-hosted
Apple Silicon runner, so a build does not wait for the one self-hosted Mac. It runs on
demand and on pull requests that touch this shell, its build script, `brand/` or the
Rust crates the app links. Pull requests build their head commit, not the merge preview.
Its inputs come from public sources, each pinned by SHA-256:

- The runtime is assembled from the upstream media-kit v0.7.2 release archive and the
  mpv 0.36.0 source archive (for the headers). Its `provenance.json` is regenerated from
  the dylibs and has to hash to the value of the Forgejo runtime package's copy. That
  file records every dylib's SHA-256, so a match means the app bundles the same dylibs.
- The H.264/AAC smoke fixture is generated on the runner with the static ffmpeg from
  the imageio-ffmpeg 0.8.1 wheel on PyPI: 12 seconds of a test pattern and a 440 Hz
  tone, 640x360 at 30 fps, H.264 High and AAC-LC 48 kHz mono.
- Rust is the version the Rust workspace job pins, installed with rustup; Swift and the
  SDK are the image's default Xcode.

After `scripts/build-macos-preview.sh`, the job fails if a bundled binary is not arm64,
loads a library from outside the app or keeps a build rpath, or if the signature does
not verify. The bounded smoke runs as in the Forgejo workflow, and the artifact carries
the same files plus the generated fixture's SHA-256.

## Verification boundary

The optional bounded smoke mode opens the deterministic local fixture in a native
window, observes loaded/progress events, checks a stable paused position, resumes,
and reports successful render calls and clean teardown. The complete app is copied
to a new directory and launched with `DYLD_LIBRARY_PATH` unset. A separate 40-second
process deadline prevents a blocked render callback from keeping the job alive.

The ZIP is created before smoke execution and uploaded even when smoke fails.
Successful engine events and rendering calls are automated evidence; seeing the
image and hearing the sound on a physical Mac remain separate operator observations.
The wider codec/display/audio acceptance matrix remains tracked in issue #348.
