# Linux Wayland DMA-BUF embedding

OK Player prefers mpv's `dmabuf-wayland` video output when it is linked to the
patched mpv v0.40.0 build produced by `scripts/build-local-mpv.sh`. Debian and
AppImage packages prepare that exact pinned upstream commit, apply the embed
patch, and install `libmpv.so.2` plus its complete non-platform dependency
closure beside the application binary. That closure includes the exact FFmpeg,
libplacebo, libass, libbluray, Rubber Band, and JPEG sonames resolved by the
pinned build. JPEG is rewritten to a private `libokp-libjpeg.so.*` SONAME so
mpv's screenshot encoder keeps the builder ABI without shadowing the target
JPEG used by TIFF/GDK modules. Every bundled ELF carries an origin-relative
runtime path, so neither the executable nor a transitive media library can
silently select a different host copy. The dynamic loader, glibc, ALSA, the
remaining image-codec libraries, and graphics-driver ABI libraries stay
target-provided and are checked by the cross-distro packaging gate. The Debian
package declares `libasound2 | libasound2t64`, `libwebp7`, `libwebpmux3`, and
`libpng16-16 | libpng16-16t64`. Fedora remains on its explicit system-mpv
packaging contract.

Shipping Debian and AppImage artifacts are built inside the repository's
digest-pinned Debian 13 builder image, which is the oldest supported runtime.
This bounds the bundled media closure to the support-floor glibc ABI. The
target desktop still supplies the complete glibc family, ALSA, GTK,
TIFF/WebP/PNG image-codec families, Wayland/X11, and graphics-driver ABI
libraries according to the package dependency and portability contracts.
Package verification runs independently on Debian testing and Ubuntu 26.04.

The embed patch is kept at
`rust/patches/mpv-v0.40.0-wayland-embed.patch`. The small
`rust/patches/mpv-v0.40.0-ffmpeg-8.patch` backports upstream mpv commit
`26b29fba02a2782f68e2906f837d21201fc6f1b9` so the pinned release builds with
current FFmpeg headers.

## Integration boundary

Stock libmpv does not expose a caller-owned Wayland surface to native video
outputs. The patch adds five pre-initialization options:

- `wayland-embed-display`: the caller-owned `wl_display` pointer
- `wayland-embed-parent`: the caller-owned parent `wl_surface` pointer
- `wayland-embed-size`: the physical output size
- `wayland-embed-scale`: the compositor scale in 120ths
- `wayland-embed-presentation-log`: enables final-surface presentation records

The embedded Wayland backend creates its own subsurface and assigns all of its
proxies to a dedicated event queue on the caller's display connection. It does
not own or destroy the display or parent surface. GTK retains both resources
until mpv has shut down.

The embedded mpv surface is a transparent container, not a black substrate.
Its DMA-BUF video and OSD children remain above that container, while OK
Player's retained EGL plane stays below it as the same-instance fallback.
During a Standard-to-Mini resize, a compositor may briefly remap the DMA-BUF
child; transparency lets the advancing EGL frame remain visible instead of
exposing an opaque black container. Standalone mpv keeps its existing opaque
black window substrate.

The GTK parent and root surfaces are transparent only while a media source is
active. EOF without a follow-up item and Close Media both clear the active
source before the welcome surface is projected, returning the GTK toplevel to
an opaque background. This keeps a detached or last-frame child surface from
becoming visible while mpv and the compositor retire the video plane.

OK Player requests `vo=dmabuf-wayland,libmpv`. If the compositor, driver, or
decoded format cannot initialize the direct DMA-BUF VO, mpv falls through to
the existing libmpv OpenGL render API in the same player instance. Development
and Fedora system libmpv builds reject the first embed option before
initialization, so OK Player selects the same OpenGL path. Shipped Debian and
AppImage payloads must pass `scripts/verify-linux-bundled-mpv.sh`; packaging
fails if the executable resolves any libmpv outside its own payload, a bundled
object has an unresolved dependency or non-origin runtime path, the closure
manifest does not match its files, or the patched embed option is absent.

## Acceptance evidence

When `OKP_PRESENT_LOG` is set, both native paths record compositor-backed
`wp_presentation_feedback` events from the final child video surface. The
evidence schema retains presented and discarded counts, physical geometry,
and the steady-window median, p95, and p99 presentation intervals. Local
render callbacks and `eglSwapBuffers` submissions are not acceptance presents.

Set `OKP_DEBUG_WINDOW_FIT=1` for fullscreen geometry acceptance. Each
fullscreen request and compositor `fullscreened` acknowledgement logs the GTK
toplevel bounds together with the requested and applied native surface,
subsurface, and buffer bounds. The render thread separately logs the exact
Wayland surface/subsurface resize after it is applied. Transition-time GTK
allocations are intentionally held; the acknowledgement line is the boundary
that requests the new native geometry and forces a frame. Logs contain only
state labels and numeric geometry.

For the screenshot/fullscreen regression, live GNOME/Wayland acceptance runs
at least 20 fullscreen → save screenshot → exit cycles split between Escape
and double-click. Run the packaged native path and the supported
`OKP_VIDEO_BACKEND=gtk` fallback. After every native cycle, the acknowledged
GTK/video-host bounds and the following applied surface/subsurface bounds must
agree and remain within one monitor workarea; the fallback run must restore one
coherent GTK/libmpv window with no native child plane.

`scripts/run-linux-wayland-presentation.sh` also launches the native backend in
Mini-player geometry. That row requires the compact command to settle at the
canonical 480×270 size and the child video surface to keep receiving
compositor presentation feedback at the acceptance cadence, with presented
frames dominating discarded feedback. This catches an opaque GTK parent
surface covering the retained native subsurface; the generated logs and
summaries remain external operator evidence.

The patch and its affected mpv translation units are build-checked against the
v0.40.0 tag. Final cadence, VA-API state, drop deltas, seeking, speed changes,
and compositor geometry still require live GNOME/Wayland hardware acceptance.
The package-bound idle-return smoke covers the EOF and Close Media state
transitions, captured alpha, and welcome identity under Xvfb. It does not prove
Wayland subsurface retirement; that remains a live GNOME/Wayland operator row.
