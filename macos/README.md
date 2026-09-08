# macOS developer preview

The first Apple Silicon shell is a small native AppKit window over the same Rust
player command/event state machine and libmpv engine as the Linux player. It opens
local videos through **File → Open**, the Open button, a file argument, or Finder's
Open With action. **Space** and the transport button toggle pause. Closing the
window stops playback and releases the renderer before destroying its GL context.
Errors stay visible until another explicit open attempt.

This preview is for local testing. It does not yet contain the Linux shell's URL
resolver, History, saving/cache UI, subtitle controls, update service or full product
styling. The app is ad-hoc signed; Developer ID signing and notarization remain
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

The workflow uploads `ok-player-macos-arm64.zip`, source identity, SHA-256, compiler
logs, dylib/rpath/signature inspection and smoke output. Extract the ZIP and open
**OK Player.app** from the extracted directory. The app does not require
`DYLD_LIBRARY_PATH` or a fixed installation path.

The pinned runtime comes from media-kit/libmpv-darwin-build v0.7.2, containing
mpv 0.36 / FFmpeg 6 and ARM64 OpenGL, CoreAudio and VideoToolbox support. Its embedded
`provenance.json` records upstream source and archive hashes. This runtime has no
Lua or bundled yt-dlp, which is why the preview is scoped to local-file playback.

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
