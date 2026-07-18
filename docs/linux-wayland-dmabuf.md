# Linux Wayland DMA-BUF embedding

OK Player prefers mpv's `dmabuf-wayland` video output when it is linked to the
optional patched mpv v0.40.0 build produced by `scripts/build-local-mpv.sh`.
The patch is kept at `rust/patches/mpv-v0.40.0-wayland-embed.patch`.

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

OK Player requests `vo=dmabuf-wayland,libmpv`. If the compositor, driver, or
decoded format cannot initialize the direct DMA-BUF VO, mpv falls through to
the existing libmpv OpenGL render API in the same player instance. Unpatched
system libmpv rejects the first embed option before initialization, so OK
Player selects the same OpenGL path without changing distro packaging.

## Acceptance evidence

When `OKP_PRESENT_LOG` is set, both native paths record compositor-backed
`wp_presentation_feedback` events from the final child video surface. The
evidence schema retains presented and discarded counts, physical geometry,
and the steady-window median, p95, and p99 presentation intervals. Local
render callbacks and `eglSwapBuffers` submissions are not acceptance presents.

The patch and its affected mpv translation units are build-checked against the
v0.40.0 tag. Final cadence, VA-API state, drop deltas, seeking, speed changes,
and compositor geometry still require live GNOME/Wayland hardware acceptance.
