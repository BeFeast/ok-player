# Issue 275 — load-time window fit evidence

Reference contract: issue #275, the Windows `FitToVideo` behavior (native size,
94% work-area budget, no upscale), PRD P1-D14 curated simplicity, and the
canonical Main Player composition. This change does not alter player chrome;
the applicable visual reference is the existing standard player at the fitted
video aspect.

## Captures

- `gtk-small-native-320x180.png` — a 320×180 video produces a 320×180 window.
  Chrome has reached its canonical playing/idle hidden state, leaving only the
  video plane.
- `gtk-4k-fit-secondary-1024x768.png` — a 3840×2160 video loaded on the
  secondary 1024×768 X screen requests 963×541 (94% of the active monitor's
  width, aspect preserved). XFWM settles at 963×542, a one-pixel height
  allocation difference; the rendered aspect remains within the smoke's
  one-pixel tolerance.

## Redline accounting

- Geometry: native size for media inside the budget; uniform downscale for
  larger media; no upscale. The 4K case uses the monitor containing the window,
  not the primary 1280×900 screen.
- Spacing and type: unchanged. Titlebar and OSC retain their canonical insets,
  density, and typography at the smaller viewport.
- Color and material: unchanged. The test fixtures use deterministic dark
  frames solely to make the window edge measurable.
- Iconography and control states: unchanged. The small capture shows the normal
  auto-hidden playing state; the 4K capture shows the normal revealed playback
  state.
- Behavior: a manual resize to 700×500 remains stable after load; maximized and
  fullscreen loads are skipped; only the first valid
  `file-loaded`/video-reconfig dimensions resize each media generation.

## Evidence boundary

Xvfb/XFWM proves deterministic logical sizing, aspect preservation, active
surface-to-monitor selection, and the maximized/manual guards on X11. It does
not prove GNOME Wayland compositor policy, real desktop workarea reservations,
or live multi-monitor movement; those remain operator QA boundaries.
