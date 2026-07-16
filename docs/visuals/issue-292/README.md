# Issue 292 — scaled workarea fit evidence

Reference contract: issue #292, PRD §15.3, the canonical Windows main-player
composition, and the existing captionless GTK player. The implementation does
not change the titlebar, OSC, typography, material, icons, or control states;
only the initial outer-window geometry changes.

## Deterministic geometry accounting

| State | Physical video | Logical workarea / scale | Reserved budget | Fitted logical window | Desired position |
|---|---:|---:|---:|---:|---:|
| Small/native | 320×180 | 1280×900 / 1× | 1232×810 | 320×180 | +480+360 |
| Oversize | 3840×2160 | 1024×768 / 1× | 976×678 | 976×549 | +24+109 |
| Scaled 4K | 3840×2160 | 1920×1080 / 2× | 1872×990 | 1760×990 | +80+45 |

The reservation is 24 logical pixels on every desktop edge plus 42 logical
pixels of vertical titlebar/chrome headroom. A single uniform scale preserves
the video aspect, and the scale is capped at one after physical-to-logical
conversion so the player never upscales beyond the video's natural size.

## Captures

- `gtk-small-native-320x180.png` — native 320×180 at scale 1, proving no
  upscale and the narrow/small-window path.
- `gtk-4k-fit-1024x768.png` — the HEVC Main10 4K fixture fitted to 976×549
  logical pixels on the 1024×768 workarea, with every edge visible.
- `gtk-4k-fit-scale-2.png` — the same physical 4K fixture on a simulated
  1920×1080 logical workarea at scale factor 2. GTK requests and settles at
  1760×990 logical pixels, fully on-screen.

## Redline accounting

- Geometry: the active monitor's logical workarea and scale factor drive the
  initial fit. The pure result includes a centered/clamped rectangle. GTK4
  intentionally has no absolute Wayland toplevel-position API, so the shell
  requests the fitted size and leaves final centering/clamping to the compositor.
- Spacing: outer desktop clearance is now explicit at 24 px; the existing 42 px
  captionless titlebar height is reserved. All internal titlebar and OSC insets
  remain unchanged.
- Type, color/material, and iconography: unchanged from the canonical player.
- Control states: unchanged. Captures use normal playing states; no smoke-only
  controls or alternate composition are introduced.
- Behavior: fullscreen/maximized windows are skipped, one fit is accepted per
  current media generation, superseded lifecycle dimensions are rejected by
  source identity, and a later manual 700×500 resize remains stable.

## Evidence boundary

The committed Xvfb/XFWM captures prove deterministic fixture decoding, logical
fit math, aspect preservation, active-monitor selection, on-screen edges, and
the manual/fullscreen/maximized guards on X11. They do not prove real GNOME
Wayland workarea reservations, compositor placement, or non-100% desktop scale.
The pull request therefore remains a deliberate operator-QA draft until the
3840×2160 HEVC Main10 fixture passes on the target 4K display at 100% and a
non-100% scale, with every player edge visible.
