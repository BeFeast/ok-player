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
branch or local-directory source. Cargo dependencies are expanded from
`rust/Cargo.lock` into `rust/packaging/flatpak/cargo-sources.json`; every crate
has a checksum and Cargo runs with `--offline --locked`. The build script first
downloads declared sources and then rebuilds with `--disable-download`, so an
undeclared fetch fails.

## Why the pin is validated, not compared

The pinned commit and the patch are one frozen pair, and the pair is what
flatpak-builder actually builds. `scripts/smoke-linux-flatpak.sh` therefore
validates the pair on its own terms: the pinned tree is materialised, the patch
must apply to it cleanly, and the applied result must still contain the Flatpak
integration it packages - the software no-DRI renderer selection, the
`codecs-extra` diagnostic, libmpv advanced control, the Flatpak-managed update
state, and the Flatpak third-party notices.

"Still contains the integration" is enforced per file, not in aggregate, by
`scripts/flatpak_integration_markers.py`. Every repository path the patch
touches must declare at least one marker of the behaviour it carries, and every
declared marker must appear in the pinned-and-patched tree **on a line that is
not a comment**. Deleting a hunk therefore fails the gate instead of shipping a
package that quietly lost a feature; commenting the integration out fails it
too; and adding a newly patched file without a marker fails as well, so the
coverage cannot silently fall behind the patch. A file type with no comment
syntax declared is an error rather than a silent pass.

The limit of that guarantee is worth stating, because a green marker check is
what a future reader will trust. A marker is a substring on a non-comment line.
It does not prove the code it names is reachable, compiled, or correct - a
string literal or dead-but-uncommented code would satisfy it. The "and it
builds" half of the contract is the offline Flatpak build, and the "and it
behaves" half is the lifecycle and renderer lanes.
`scripts/tests/flatpak-integration-markers.sh` is the self-test for the checker
itself: it drives it against synthetic trees and requires line-commented,
block-commented, and HTML-commented markers to be rejected while a Markdown
heading is not mistaken for a comment.

The same comment rule applies to the literal assertions the smoke makes about
`.github/workflows/flatpak.yml` and `scripts/smoke-linux-software-renderer.sh`:
both files are matched with whole-line comments removed, so a comment cannot
stand in for the code being asserted. Those assertions are still substring
matches - they prove text is present as code, not that it runs.

The check deliberately does not regenerate the patch from the working tree and
compare it byte-for-byte. That comparison passes only while nothing has touched
a patched file since the pin was taken, so every later change to
`okp-linux-gtk`, `okp-mpv`, or `okp-core` would fail an unrelated packaging
check with a byte offset. Freshness is a scheduled maintenance task, not a
merge gate: `scripts/flatpak-repin.sh` moves the pin to the current default
branch and regenerates the patch, and the nightly `repin` job in
`.github/workflows/flatpak.yml` runs it and proposes the result as a pull
request. It never pushes to `main`. A pull request opened with the workflow
token does not start checks by itself, so the job also dispatches the Flatpak
lane explicitly against `chore/flatpak-repin`; the run appears under that branch
rather than on the pull request, and no manual close/reopen is needed.

Maintenance is not the same as no bound. The smoke check fails when the pin is
more than 50 commits or 14 days behind `origin/main` (override with
`OKP_FLATPAK_MAX_PIN_DRIFT_COMMITS` and `OKP_FLATPAK_MAX_PIN_DRIFT_DAYS`). Days
are measured between the pin and the tip of `origin/main`, so a quiet default
branch never ages the pin. Past those bounds the "integration patch" stops being
a reviewable delta and becomes an unreviewable catch-up diff, and the fix is to
run the re-pin script and commit the result rather than to widen the bound.

Once the integration is merged upstream, the regenerated patch is empty. The
re-pin script then deletes the patch file and removes the `patch` source from
the manifest, and the smoke check requires those two to stay consistent.

## Where the lane runs

The Flatpak workflow runs on pull requests that touch the packaging manifest,
its scripts, the workflow, or `rust/Cargo.lock`; on every push to `main` that
touches the same files; nightly; and on manual dispatch. Changes elsewhere in
`rust/crates` deliberately do not trigger it, because the pinned source, not
the working tree, is what the lane builds - the nightly run and the re-pin pull
request are what carry current default-branch work into the package. Every step
after the offline build runs even if an earlier step failed, so one red gate can
never hide whether the build, the delivery lifecycle, and the renderer smoke
work. That guard is asserted per step by `scripts/smoke-linux-flatpak.sh`,
which parses the job's steps and requires it on each named step and on every
step after the offline build; an occurrence count would let one step lose its
guard behind another gaining one.

### Why this lane is not a required status check

The three required contexts on `main` (`Unit tests (engine-agnostic Core,
headless)`, `Rust workspace (Linux)`, `Integration tests (real libmpv +
render-thread guard)`) have no path filter, so they report on every pull
request. This workflow's `pull_request` trigger is path-filtered. A required
context that never runs is never reported, and GitHub holds the pull request at
"Expected - Waiting for status to be reported" indefinitely. Adding `Offline
Flatpak beta build` to the required contexts as the workflow stands would
therefore block every pull request that does not touch the packaging paths.

Making it required needs a companion job first: a second job with the same
`name:` (so it reports the same check context), triggered on `pull_request` with
`paths-ignore:` mirroring this workflow's `paths:`, doing nothing and
succeeding. Only with that skip shim in place does the context report on every
pull request, and only then is promoting it to required safe.

Cost is not the objection. The lane's `timeout-minutes: 150` is a ceiling, not a
duration; observed end-to-end job times are around ten minutes (9m45s and 10m24s
on two runs of the same job). Until the shim
exists, the pin drift bound plus the nightly run are what keep the pin honest on
pull requests that do not run the lane.

The lifecycle lane's negative control is checked by status, not by mere
failure. `scripts/smoke-linux-flatpak-lifecycle.sh` exits 3 only when the
controlled step's own assertion is what failed; a missing tool exits 127, a
missing artifact manifest or an unknown control id exits 2, and any other
lifecycle assertion exits 1. The workflow requires exactly 3 plus the
assertion's own message, so a control cannot report success because the lane
died before reaching it. `scripts/tests/flatpak-lifecycle-control.sh` drives
that wiring against scripted stand-ins for Flatpak and the launch probe and is
run by the workflow before the offline build. It checks both sides of the
status-3 contract: a controlled step must produce 3, and a failure at any other
step while a control is set must stay at 1. Without the second side, routing
every failure to 3 would satisfy the workflow's check.

A launch control suppresses the launch and leaves the probe's mapped-window
search to notice, rather than reporting the failure itself, so the self-test
fails if that assertion is deleted. The stand-ins run the real probe body with
window visibility tied to whether the stand-in application process is alive;
they still prove nothing about real X, real Flatpak, or real window mapping,
which is the packaged lane's job.

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
- `flatpak-lifecycle-ci.json` and `lifecycle-logs/` record the automated
  lifecycle lane described below.
- `artifacts/manual-ui/linux-software-renderer-smoke` contains the packaged
  no-DRI mapped-window evidence: full-window and cropped screenshots, sanitized
  renderer/session logs, presentation samples, `xwininfo` map-state output, and
  `results.json` with `IsViewable`, non-trivial geometry, zero DRI descriptors,
  renderer identity, pixel measurements, and screenshot SHA-256 values.

## Automated lifecycle lane

`scripts/smoke-linux-flatpak-lifecycle.sh` runs the full delivery lifecycle in
CI against the freshly exported repositories: it installs the baseline from a
baseline-only remote, points the same remote at the two-commit repository,
updates, rolls back to the parent commit, restores the child commit, uninstalls,
and deletes the remote. Every step reads the deployed OSTree commit back from
Flatpak and compares it with the identity recorded in
`flatpak-beta-artifact.json`, so an update that reports success without moving
the deployment, or a rollback that does not return to the parent commit, fails
the lane. The three launch steps start the deployed revision under a throwaway X
server and require a mapped, viewable OK Player top-level window, so a revision
that installs but cannot run also fails.

The lane writes `flatpak-lifecycle-ci.json` using the same schema as operator
acceptance, with `desktop: headless` and `session: headless-ci`, and validates
it with `flatpak-lifecycle-validate --transitions-only`. A headless record can
never satisfy `validate_ready`, so an automated result cannot be mistaken for
live-desktop sign-off.

One field in that record means something narrower than its name. In an operator
record `downloaded_artifact_sha256` is the digest of the artifact the operator
downloaded, and it binds the record to that download. The CI lane cannot do
that: the artifact it would hash is produced by the upload step that runs after
it. The headless record therefore carries the digest of the update `.flatpak`
bundle under test, which duplicates `artifact.update.bundle.sha256` and is
provenance rather than independent evidence.

CI also runs the lane once with
`OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL=update-current`, which skips exactly one
transition command. That run must fail; if it passes, the lane is not asserting
anything and the job fails instead. Any required step id can be named there to
re-check a specific transition.

Exercise the repository lifecycle by hand from the extracted CI artifact.
Resolve the local repository directories to `file://` URLs at runtime; do not
paste those machine-specific URLs into public evidence:

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
baseline-to-update OSTree history, repository export, bundle creation,
portable artifact identity, and the complete install, update, rollback,
restore, uninstall, and remote-cleanup transition chain with a mapped window
after each deployment change. The packaged no-DRI smoke removes DRI from the app,
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
