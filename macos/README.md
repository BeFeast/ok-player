# macOS developer preview

The first Apple Silicon shell is a small native AppKit window over the Rust session
(`okp-session`, through the JSON session ABI in [docs/session-abi.md](../docs/session-abi.md))
and the same libmpv engine as the Linux player. It opens local videos through
**File → Open**, the Open button, a file argument, or Finder's Open With action.
The video fills the window and the transport controls float over it (see
[Transport controls](#transport-controls)); **Space** and their play button toggle
pause. Closing the window stops playback,
destroys the render context and then frees the session. Errors stay visible until
another explicit open attempt.

The window runs its content under a transparent title bar, with the media title in
the strip along the top. Opening a file fits the window to the video on the screen
it is on: at most one video pixel per device pixel, centered, never below the
window's minimum size, with room for the control bar under the video so the video
view itself has the video's size. The session keeps the one fit each opened source
is owed and answers the `initial_window_fit` query with okp-core's frame, whichever
way the file was opened. While video shows, a resize keeps the video view at the
video's aspect (`windowWillResize(_:to:)`; AppKit's `contentAspectRatio` would lock
the whole content, bar included). **F**, **Control-Command-F** and a double click on
the video toggle native fullscreen; **Esc** leaves it. A single click on the video
toggles pause once the double-click interval has passed, so a double click never
pauses first. The traffic lights and the title strip hide and show with the
transport controls (`WindowChromeVisibility`); hidden ones take no clicks.

This preview is for local testing. It does not yet contain the Linux shell's URL
resolver, History, saving/cache UI, subtitle styling or update service. Its chrome
uses the shared BeFeast palette in light and dark, following the system appearance;
the full macOS UI redesign is still ahead. The app needs macOS 14 or later on Apple
Silicon. It is signed with a stable self-signed identity when one is configured (see
[Signing](#signing)); Developer ID signing, notarization and a DMG remain separate
distribution work. No global libmpv or package-manager installation is needed to run
the packaged app.

## Install a build on a Mac

Once the bridge token is configured, every pull request head and every main commit
that touches the Mac build gets an artifact on its Forgejo check run (see
[Forgejo pull request check](#forgejo-pull-request-check)), named
`macos-player-<commit SHA>`. Download it from a shell, not a browser: `curl` and
`ditto` set no quarantine flag, so Gatekeeper does not hold the app back and no
administrator rights are needed. `jq` ships with macOS 15; on macOS 14 copy the
artifact link from the run page instead.

```sh
api=https://<forgejo host>/api/v1/repos/BeFeast/ok-player
sha=<full commit SHA the check built>                       # or, for the main tip:
# sha="$(curl -fsS "$api/branches/main" | jq -r .commit.id)"
work="$(mktemp -d)"
url="$(curl -fsS "$api/actions/artifacts?name=macos-player-$sha" |
  jq -er '[.[] | select(.expired | not)][0].archive_download_url')"   # fails when there is none
curl -fsSL -o "$work/artifact.zip" "$url"
ditto -x -k "$work/artifact.zip" "$work/artifact"
(cd "$work/artifact" && shasum -a 256 -c SHA256SUMS.txt)   # must print: ok-player-macos-arm64.zip: OK
pkill -x ok-player || true                                  # quit a running build
rm -rf "$HOME/Applications/OK Player.app"                   # replace the previous build
mkdir -p "$HOME/Applications"
ditto -x -k "$work/artifact/ok-player-macos-arm64.zip" "$HOME/Applications"
open "$HOME/Applications/OK Player.app"
```

A main commit that changed nothing the Mac build reads has no build of its own. To
get one for the main tip, start the check by hand on `main` (any account with write
access, from the workflow's page in the Forgejo Actions tab or through the API below),
wait for the run to finish and download as above:

```sh
curl -fsS -X POST -H "Authorization: token $FORGEJO_TOKEN" -H 'Content-Type: application/json' \
  -d '{"ref":"main"}' "$api/actions/workflows/macos-pr-check.yml/dispatches"
```

To see which build is installed, read its version, `CFBundleVersion` and source
revision (the exact `git describe` it was built from):

```sh
for key in CFBundleShortVersionString CFBundleVersion OKPSourceRevision; do
  plutil -extract "$key" raw "$HOME/Applications/OK Player.app/Contents/Info.plist"
done
```

## Build and package layout

`scripts/build-macos-preview.sh` builds everything from a caller-supplied runtime
prefix (`OKP_MPV_PREFIX`), into an empty `OKP_MACOS_OUTPUT`, with an isolated
`CARGO_TARGET_DIR`:

1. It runs the okp-ffi tests and builds its opt-in `live-mpv` dylib.
2. It copies that dylib and the whole runtime dylib closure into a staging directory
   and rewrites install names and absolute references to `@rpath`.
3. It writes the Swift package's two generated inputs, both ignored by git:
   `Sources/COKPCore/okp_core.h` (the FFI header) and
   `Sources/OKPlayerShell/Generated/Palette.swift` (see [Colours](#colours)).
4. `swift build -c release` builds the `ok-player` executable and `swift test` runs
   the XCTest target against the staged dylibs.
5. It assembles `OK Player.app`: executable, dylibs, `Info.plist` stamped by
   `scripts/generate-macos-info-plist.py`, `Resources/AppIcon.icns`, licence, this
   README and the runtime provenance. Every rpath that points into a build machine is
   removed.
6. It signs every dylib, the executable and the bundle with one identity, records the
   designated requirement in `signing.txt`, and zips the app with its SHA-256.

The Swift package (`Package.swift`, no Xcode project) has three targets:

| Target | What it is |
|---|---|
| `COKPCore` | System library module over the okp-ffi C ABI; links `libokp_ffi` |
| `OKPlayerShell` | The AppKit shell: window, session client, video layer, palette, launcher |
| `OKPlayer` | The thin `ok-player` executable; it only calls `PlayerLauncher.run()` |
| `OKPlayerShellTests` | XCTest target for `OKPlayerShell` |

## Bundle identity, version and icon

- **Bundle id** `uk.oklabs.ok-player`. It is final and must not change again: macOS
  keys privacy grants (Downloads, Desktop, network volumes, local network) on it.
- **Version.** `CFBundleShortVersionString` is `MAJOR.MINOR.PATCH` of the nearest
  `v<semver>` tag, without a prerelease suffix, because macOS reads three integers
  there. `CFBundleVersion` is the number of commits reachable from the build commit,
  then the CI run number (0 locally), so a newer build always has the higher value.
  The build needs the full history and tags; a shallow clone fails instead of
  stamping a wrong version.
- **Icon.** `Resources/AppIcon.icns` is generated from the Linux app icon SVGs in
  `rust/packaging/linux/` by `scripts/generate-macos-icon.py`, on Apple's icon grid
  (an 824-unit body on a 1024-unit canvas), with the size-tuned hicolor SVGs at 64 px
  and below. It is checked in, because rasterising needs librsvg; rerun the script
  after the icon source changes.
- `NSLocalNetworkUsageDescription` explains the local network prompt.

## Signing

An ad-hoc signature is the binary's hash, so every ad-hoc build is a new app to macOS
and every privacy grant is asked for again after an install. A certificate gives
every build the same designated requirement,
`identifier "uk.oklabs.ok-player" and certificate leaf = H"<certificate SHA-1>"`,
so the grants carry over to the next build.

The GitHub-hosted workflow imports a self-signed code-signing certificate from two
repository secrets, `MACOS_SIGNING_P12_BASE64` and `MACOS_SIGNING_P12_PASSWORD`, into a
temporary keychain (`scripts/macos-signing-identity.sh`), signs with it and deletes the
keychain at the end. Without the secrets it signs ad hoc and says so in the job
summary. `security` reports the certificate as not trusted; that is expected for a
self-signed certificate and does not stop `codesign`.
`scripts/tests/macos-signing-identity.Tests.sh` runs in every macOS job: it makes a
throwaway certificate with the recipe below, signs two different builds through the
same script and requires one designated requirement for both, and different ones for
two ad-hoc builds.

One-time setup, on any machine with OpenSSL and `gh` logged in to the mirror. The
certificate must stay the same for the life of the app, because replacing it changes
the designated requirement; keep `identity.p12` and its password in the team's secret
store as the master copy.

```sh
work="$(mktemp -d)" && cd "$work"
cat > cert.cnf <<'EOF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = OK Player self-signed code signing
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
EOF
openssl req -x509 -newkey rsa:3072 -nodes -days 7300 -config cert.cnf -keyout key.pem -out cert.pem
password="$(openssl rand -hex 24)"
# The legacy algorithms are the ones macOS `security import` accepts.
openssl pkcs12 -export -inkey key.pem -in cert.pem -name "OK Player self-signed code signing" \
  -out identity.p12 -passout "pass:$password" -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
base64 < identity.p12 | tr -d '\n' | gh secret set MACOS_SIGNING_P12_BASE64 -R BeFeast/ok-player
printf '%s' "$password" | gh secret set MACOS_SIGNING_P12_PASSWORD -R BeFeast/ok-player
openssl x509 -in cert.pem -noout -fingerprint -sha1   # the H"..." in the designated requirement
```

## Forgejo pull request check

`.forgejo/workflows/macos-pr-check.yml` gives every Forgejo pull request the check
**`macOS PR check / Apple Silicon app (GitHub-hosted) (pull_request)`**. It runs
`scripts/mac-ci-bridge.py`:

- `scope` diffs the pull request against its merge base. When nothing the Mac build
  reads changed (the bridge's `MACOS_PATHS`: `macos/`, the macOS scripts, `brand/`,
  the Cargo manifest and lock file, okp-ffi, okp-session, okp-mpv, okp-core and both
  workflow files), the check passes at once.
- Otherwise `run` pushes the head commit to `mac-ci/pr-<number>` on the GitHub mirror.
  That push starts `.github/workflows/macos-player-preview.yml`; no GitHub pull request
  is opened. The bridge waits up to 60 minutes, prints the run link, fails unless the
  run concluded `success` and its `ok-player-macos-arm64.zip` matches
  `SHA256SUMS.txt`, prints the failing job's log tail when it did not, and deletes the
  branch. A newer push to the pull request cancels both the superseded Forgejo job and
  its GitHub run.
- The downloaded artifact is published again as `macos-player-<commit SHA>` on the
  Forgejo run.

The same job runs on every push to `main`, with the path set compared against the
previous tip and the bridge branch `mac-ci/main`, so main commits that change the Mac
build get their own artifact. Main runs queue rather than cancel each other. Started by
hand (`workflow_dispatch`), it builds the tip of the chosen branch without the path
check, through `mac-ci/<branch>`.

One-time setup: store a GitHub token that can push to `BeFeast/ok-player`, including
workflow files, and read and cancel its Actions runs (a classic token with the `repo`
and `workflow` scopes, or a fine-grained one with **Contents**, **Workflows** and
**Actions** set to read and write) as the Forgejo repository Actions secret
`MAC_CI_GITHUB_TOKEN`. Until it exists, the job builds nothing and passes with a
warning that names the secret, so the check gates nothing yet and blocks no pull
request. Once the secret is set, every failed, cancelled or timed-out Mac run is red;
when the check is green on a macOS-relevant pull request, add its name above to the
required checks of `main`.

The bridge also runs by hand, for example before the secret exists, from a clone that
has the pull request's head commit. With `--forgejo-status` it posts the result as the
commit status `macOS PR check / manual bridge`:

```sh
MAC_CI_GITHUB_TOKEN="$(gh auth token)" FORGEJO_TOKEN=<forgejo token> \
  python3 scripts/mac-ci-bridge.py run --sha <head SHA> --ref mac-ci/pr-<number> \
  --download-dir "$(mktemp -d)" --forgejo-status --forgejo-api https://<forgejo host>/api/v1
```

The manual Forgejo `macOS player preview` workflow stays as an on-demand fallback on
the self-hosted Mac. It accepts an exact source SHA, a numeric-IP Forgejo endpoint and
an existing toolchain location, downloads the checksum-pinned ARM64 runtime and
H.264/AAC fixture from Forgejo generic packages, and runs the same build script.

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
| Open button | `accent` fill, `onAccent` label |
| Keyboard focus ring on the Open button and the scrubber | `focus` |
| Status text: hint, error | `textMuted`, `danger` |
| Transport icons, elapsed time, speed, chapter ticks | `text` |
| Trailing time label | `textMuted` |
| Scrubber track, buffered range, progress and knob | `borderSubtle`, `border`, `accent` |
| Volume track, level up to 100 %, boost above it, 100 % mark | `borderSubtle`, `accent`, `warning`, `text` |
| Volume level while muted | `textMuted` |

The video surface gets no tint. A malformed palette or a role missing from one
appearance fails the build. `scripts/tests/macos-palette.Tests.py` checks that the
generated table carries the JSON values, and the XCTest target checks that every role
resolves to its light value in Aqua and its dark value in Dark Aqua.

The workflow uploads `ok-player-macos-arm64.zip`, source identity, SHA-256, version,
signing mode, compiler and test logs, dylib/rpath/signature inspection and smoke
output. With smoke enabled it also uploads `snapshots/window-light.png` and
`window-dark.png`: after the playback check the app renders its own window content in
each appearance, overriding only that window's appearance. The OpenGL video area is
not part of that render and stays empty. The app does not require
`DYLD_LIBRARY_PATH` or a fixed installation path.

The pinned runtime comes from media-kit/libmpv-darwin-build v0.7.2, containing
mpv 0.36 / FFmpeg 6 and ARM64 OpenGL, CoreAudio and VideoToolbox support. Its embedded
`provenance.json` records upstream source and archive hashes. This runtime has no
Lua or bundled yt-dlp, which is why the preview is scoped to local-file playback.

## GitHub-hosted build

`.github/workflows/macos-player-preview.yml` builds the app on a GitHub-hosted Apple
Silicon runner. It runs on demand, on GitHub pull requests that touch the bridge's
path set, and on pushes to `mac-ci/**` branches, which is how the Forgejo check starts
it. Pull requests build their head commit, not the merge preview. Its inputs come from
public sources, each pinned by SHA-256:

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
not verify. The bounded smoke runs as in the Forgejo workflow, but the step also fails
unless the app wrote both window snapshots. The artifact carries the same files plus
the generated fixture's SHA-256.

## Session and rendering

`SessionClient` owns the session handle. The session's event thread calls its
wakeup once per batch; the wakeup only hops to the main queue, which drains the
batch, decodes it with the `Codable` models in `SessionProtocol.swift` and applies
it to an `@Observable` `SessionStore`. Nothing polls. The XCTest target decodes and
re-encodes every golden document in `rust/crates/okp-session/fixtures`, so a
protocol change on either side fails the Mac build until both agree.

Video is drawn by `VideoLayer`, a `CAOpenGLLayer` with its own CGL context (3.2
core profile, a 16-bit float colour buffer when the GPU offers one) and its own
serial render queue. The render context is created with libmpv's advanced control:

1. libmpv's update callback schedules `okp_session_render_update` on the render
   queue. It never waits for the main thread, because libmpv may be waiting for
   the render thread while the main thread waits for libmpv.
2. Only when that update says a frame is due is the view's display link
   (`NSView.displayLink`) started.
3. The next tick draws on the render queue: `display()`, `CATransaction.flush()`,
   then `okp_session_render_report_swap`.

Ticks without a due frame pause the link again, so a paused or idle player draws
nothing and stops ticking. A resize redraws the current frame once; it never
creates another GL context. Nothing draws on the main thread.

## Transport controls

The transport controls (`Sources/OKPlayerShell/OSC/`) float at the bottom of the
video on a `popover` visual-effect material: the scrubber, previous and next item,
play/pause, the time readout, the speed menu, mute, the volume slider and the
fullscreen button. Every string, fraction and rule they show comes from the
session's `transport` query (`docs/session-abi.md`, "Transport controls"), which
applies the core's time, buffered-range, chapter, volume and visibility rules; the
Swift side only turns input into commands and keeps the hide timer.

- **Scrubber:** a click or drag sends keyframe seeks while the pointer moves and
  one exact seek where it is released; the readout follows the drag position. It
  shows the buffered range and chapter ticks. While it has keyboard focus,
  Left/Right step five seconds and Space toggles pause. It gives keyboard focus
  back to the video as soon as the controls hide or the media can no longer be
  sought, so Space always reaches the player. If the file ends under a held
  press, the drag ends there: nothing seeks on release, and the next file shows
  its own playhead.
- **Time:** elapsed time and a trailing label that switches between remaining and
  total time on a click, in tabular digits.
- **Volume:** 0-130 % with mute. Mute follows the core and the Linux player: the
  button sends `toggle_mute`, which sets the level to 0 and, from 0, back to the
  last level above it, so a slider dragged to 0 % is unmuted by the same button,
  and scrolling or dragging up from 0 brings the sound back. Scrolling anywhere
  over the video sends `nudge_volume` steps: a wheel notch is one step however
  fast the wheel spins, trackpad points add up to steps (at most three per event),
  and the inertia after lifting the fingers changes nothing.
- **Speed:** 0.5, 0.75, 1, 1.25, 1.5 and 2×, with the current speed checked.
- **Previous/next item:** send the session playlist's `previous` and `next`
  (`SessionPlaylist`), enabled while the playlist has two items or more.
- **Auto-hide:** while media plays, the controls and the cursor hide after the
  session's idle timeout (2.5 s) without pointer movement or a key press. The title
  bar's traffic lights and title strip fade with them (`oscVisibilityChanged`). Paused or loading media
  keeps them up, and the pointer over them, an open menu or a scrubber drag holds
  them. Hidden controls take no clicks, and clicks outside the bar always reach the
  video.

## Tracks and subtitles

`Sources/OKPlayerShell/Tracks/` adds two buttons at the front of the transport
controls' trailing group, Audio and Subtitles. Each opens a popover listing the
tracks with the labels the session's `tracks` query gives them (core
`track_label`: name, language, channels, codec, `EXT` for a file loaded beside the
media) and checks the one that plays; choosing one sends `select_track`. The
subtitle popover starts with Off and ends with **Add Subtitle File…** and the
delay and size rows, each with the session's readout and a step down and up.

- **Add Subtitle File…** and **S** open a file panel limited to subtitle files
  (`.srt`, `.ass`, `.ssa`, `.sub`, `.vtt`, `.idx`, `.sup`); the file goes to the
  session as `add_subtitle` and becomes the subtitle track.
- Subtitle files named after the media (`movie.srt`, `movie.en.srt`) load with it;
  libmpv finds them (`sub-auto=exact`).
- **Z** / **Shift+Z** move subtitles 50 ms later / earlier and **]** / **[** make them
  10 % larger / smaller (0.25-4). The keys come from the session's shortcut table,
  the session runs them, and the new value shows for a moment over the top of the
  video.
- `TrackControls.populate(_:kind:)` fills an Audio or Subtitles menu with the same
  items, for the menu bar (#880) to host.

## System integration

`System/SystemIntegration.swift` connects the player to macOS. It is handed the
session snapshot after every drained batch and keeps no playback state of its own:

- **Now Playing.** `MPNowPlayingInfoCenter` gets the title (libmpv's media title,
  else the file name), duration, elapsed time, rate, media type, queue index and count,
  chapter count and the app icon as artwork, and `playbackState`, which is what makes
  the app the Now Playing app on macOS. It is written when the state changes and when
  the position moves more than 0.75 s away from where elapsed time and rate put it (a
  seek), not on every time update. An audio file counts as audio even when embedded
  cover art gives libmpv a video frame. Ended and idle sessions clear it.
- **Remote commands.** Play, pause, play/pause, stop, next and previous item, change
  position, skip ±10 s and change rate from `MPRemoteCommandCenter` become session
  commands (stop closes the file; rates are kept within 0.25-4). This covers the F7-F9
  media keys, Control Center and AirPods with no event tap, so no Accessibility or
  Input Monitoring permission. Next and previous send the session playlist's `next`
  and `previous` commands (`System/SessionPlaylist.swift`, over the playlist
  `SessionStore` keeps from `playlist_changed`), and are offered only while the
  playlist has two items or more: the session wraps at both ends.
- **Display sleep.** An `IOPMAssertionCreateWithName` assertion of type
  `kIOPMAssertionTypePreventUserIdleDisplaySleep` is held while video plays and
  released on pause, end, close, audio-only media and quit.
- **Recent documents.** Every local file that loads, including ones the session opens
  by itself, goes to `NSDocumentController.noteNewRecentDocumentURL`, except during a
  private session (the session's `history.private_session` event). File > Open
  Recent is rebuilt from that list each time it opens, and macOS lists the same files
  in the Dock menu.
- **Dock menu.** Play or Pause, Next and Previous, enabled from the snapshot and the
  playlist; Next and Previous step the session playlist.

## Verification boundary

The optional bounded smoke mode opens the deterministic local fixture in a native
window and checks, from the session's events and the video layer's counters:

- no render calls while idle, before the file is open;
- the position and the render calls advance while playing;
- while playing, the system's Now Playing reports the fixture as playing video,
  IOKit lists the display-sleep assertion, and `MPRemoteCommandCenter` has play,
  pause, play/pause, stop, both skips (10 s), change position and change rate enabled
  (with the offered rates) and next and previous off; the pause is sent by running
  the handler registered with the center's pause command;
- while paused, the position holds and the render calls stay flat for two seconds,
  Now Playing reports the pause at rate 0 and the assertion is gone; the resume is sent
  through the handler registered with the center's play command;
- both advance again after resuming;
- after six window resizes during playback, Core Animation holds exactly one GL
  context (each `copyCGLContext` but one is balanced by `releaseCGLContext`) and
  has drawn into no context but the layer's own;
- every frame drawn reported a swap that reached libmpv: the count
  `okp_session_render_report_swap` returns equals the frames drawn;
- once playing, the window has the frame the fit gave it, the video view has the
  video's aspect after the fit and after a resize through the window's resize rule,
  and the title strip has room under the title bar;
- the transport controls stay up through the paused seconds, hide after the idle
  timeout once playback resumes without pointer activity, take the title bar's
  traffic lights with them, hand the scrubber's keyboard focus back to the video
  as they hide, and come back on activity with the session's time readout, so
  both window snapshots show them;
- F and Escape, sent as key events to the video view, enter native fullscreen and
  leave it again, and each press reaches the transport controls as activity;
- `]` sent to the window steps the subtitle size and shows the session's `110%`
  readout, and a generated SRT file added during playback is the selected external
  subtitle track a few seconds later;
- closing the window during playback tears down in order (close, destroy the
  render context, free the session), after which the center has none of those
  commands enabled and no handler of the app's.

Its `OKP_SMOKE` line also reports those counts, the pixel format, the
volume and track count from the session snapshot, the time readout the controls
showed, and the fitted size, the video view's size after the fit and after the
resize, the aspect and the title strip height. The complete app is copied
to a new directory and launched with `DYLD_LIBRARY_PATH` unset. A separate 40-second
process deadline prevents a blocked render callback from keeping the job alive.

The ZIP is created before smoke execution and uploaded even when smoke fails.
Successful engine events and rendering calls are automated evidence; seeing the
image and hearing the sound on a physical Mac remain separate operator observations,
and so does the check that privacy grants survive an update with the stable identity.
The wider codec/display/audio acceptance matrix remains tracked in issue #348.
