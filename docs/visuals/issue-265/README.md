# Linux canonical playback-state handoff

Issue #265 closes the remaining Main Player playback-state and interaction gaps
without changing the canonical OSC control order established by #257. The
dedicated floating Volume artifact remains owned by #262 and is not reproduced
here.

## Material and geometry

- The OSC uses `rgba(22, 22, 25, 0.50)` over a localized bottom scrim.
- The compact GTK fallback is a 14px radius with 7x14px interior padding and
  32px primary targets.
- The design source calls for a 24-26px blur plus saturation. GTK 4 has no
  portable backdrop-filter, so Linux uses the locked tint, hairline, shadow,
  and scrim as its deterministic fallback. Native compositor blur is not
  claimed by the Xvfb evidence.
- Control order remains Play/Pause, Previous, Next, elapsed, timeline, trailing
  time, volume, speed, subtitles, audio, chapters, screenshot, fullscreen,
  overflow.

## State and interaction mapping

- Paused playback shows a quiet centered `PAUSED` cue and pins chrome.
- Loading shows an indeterminate ring and pulsing timeline band.
- Failure stays in the playback canvas with Retry, Open another, and Copy
  details actions. No modal GTK error dialog is used.
- The OSD is top-centered at 64px, uses the 60% playback material, and remains
  visible for 1700ms.
- A video single-click commits play/pause after the desktop double-click
  interval. A double-click cancels that pending commit and toggles fullscreen.
- The trailing label toggles between remaining and total time.
- The buffered band is rendered below progress while chapter ticks, bookmarks,
  A-B marks, hover preview, seeking, and keyboard navigation stay on the
  existing `GtkScale`.
- The titlebar includes Always on top and appends the active chapter to the
  media title. X11 uses the EWMH `_NET_WM_STATE_ABOVE` request. Wayland has no
  GTK client protocol for forcing this state, so the action reports that
  compositor limitation rather than misusing modal/transient flags.

## Deterministic evidence

The original delivery copied `loaded-paused-osc.png` into the paused,
buffered-timeline, and chapter-context slots. Issue #272 removes those preview
shortcuts and publishes the repaired real-state evidence in
[`../issue-272/`](../issue-272/README.md).

The release harness records exact `1120x680` captures for:

- paused
- buffering/loading
- playback error
- playing idle
- OSD
- buffered timeline
- chapter context
- bright video
- dark video

`scripts/smoke-linux-playback-interactions.sh` separately drives real mpv and
proves single-click play/pause, double-click fullscreen cancellation, the time
label toggle, Always on top on X11, and gesture isolation from seek/panel
controls.

Live GNOME/Wayland acceptance remains operator-only. It must verify compositor
fullscreen behavior, cursor hiding, motion quality, subtitle lift, focus
navigation, and the Wayland Always-on-top limitation before merge.
