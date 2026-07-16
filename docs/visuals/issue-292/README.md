# Issue 292 — scaled workarea fit evidence

Reference contract: issue #292, the Windows `FitToVideo` behavior (native
physical size, no upscale, 94% workarea budget), PRD §2.1/§15 window behavior,
the canonical Windows player capture, and the prior Linux issue #275 capture at
the same 1024×768 viewport. This regression changes only outer-window geometry;
the player composition and control styling are unchanged.

## Captures

- `gtk-4k-fit-centered-1024x768.png` — the deterministic 3840×2160 HEVC Main10
  fixture on a 1024×768 scale-1 screen. The 962×541 window is centered at
  +31,+113, leaving every edge visible. This is the direct same-viewport/state
  comparison to `docs/visuals/issue-275/gtk-4k-fit-secondary-1024x768.png`.
- `gtk-4k-fit-scale-2-3840x2160.png` — the same physical video and codec on a
  3840×2160 scale-2 screen. GTK reports a 1920×1080 logical workarea; the player
  requests and settles at 1729×973 logical pixels, centered at +95,+53. The X11
  evidence reports the device-coordinate position as +190,+106.

## Redline accounting

- Geometry: mpv video dimensions remain physical pixels. The active GDK
  surface scale converts the natural-size ceiling to logical pixels before the
  fit. The shared geometry applies one aspect-preserving scale, never upscales,
  reserves the existing 6% desktop edge budget plus the canonical 42 px custom
  titlebar band, and centers/clamps the result inside an offset-capable workarea.
- Spacing: titlebar, OSC, timeline, and resize-handle insets are unchanged. The
  42 px titlebar height is now also reserved by the outer fit budget rather than
  being allowed to settle against a monitor edge.
- Type: unchanged Segoe-compatible GTK typography, weights, sizes, and tabular
  time readout.
- Color/material: unchanged dark video substrate, over-video scrims, hairlines,
  and fixed over-content accent. The deterministic HEVC frame may render black
  under the software Xvfb path; it is not a palette change.
- Iconography: unchanged canonical player, transport, volume, playlist,
  screenshot, fullscreen, pin, and caption-control glyphs.
- Control states: both captures show loaded playback with the titlebar and OSC
  revealed. Scale changes geometry only; enabled/disabled, hover, focus, and
  active-state behavior is unchanged.
- Behavior: fitting remains one-time per media generation. Fullscreen and
  maximized loads are skipped; a later manual 700×500 resize remains stable.
  Lifecycle dimensions now carry their engine path, so a queued event from
  source A cannot consume source B's one-time fit. X11 applies the centered
  move/resize directly. Wayland suspends the libmpv render context before the
  fitted toplevel is unmapped, remaps it on the next main-loop turn so Mutter
  chooses a fresh visible placement, and restores rendering only after the
  GLArea maps again. This avoids the active-GLArea remap that produced Wayland
  protocol error 71 in the rejected snapshot.

## Evidence boundary and operator gate

Xvfb/XFWM proves deterministic physical-to-logical conversion, HEVC Main10
fixture dimensions, chrome/margin reservation, aspect fit, scale-1/scale-2
centering, edge containment, fullscreen/maximized guards, post-load manual
resize stability on X11, and the real GTK/libmpv render-context suspend/remap/
restore sequence through the forced safe-remap smoke hook. It does not prove
Mutter's Wayland placement policy or protocol behavior: Wayland intentionally
exposes no client-controlled global toplevel coordinates, so GNOME owns the
fresh mapped placement. The fitted GLArea/FBO assertion also depends on #298 /
PR #299, which removes the stale previous-frame viewport as a render-target
source rather than duplicating that separate fix here.

Before this PR is marked ready, operator QA must run the generated `fit-4k.mkv`
fixture on the real GNOME/Wayland 4K display at 100% and a non-100% scale, then
confirm the complete player is visible, aspect-correct, centered or
compositor-clamped, and that a later explicit move/resize is not overridden.
Repeat with media larger than the available workarea. The draft/WIP marker must
remain until those live-desktop rows pass.
