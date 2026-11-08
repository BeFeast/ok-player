# Linux GTK shell on the BeFeast palette

Issue #791 baseline: the GTK shell after its colours moved from the old teal identity
to `brand/palette.json`. These are reference scenes, not a gate.

Every capture comes from an existing Xvfb smoke script run against a debug build
(1280x900 screen, xfwm4, software GL):

| Scene | Light | Dark | Script |
|---|---|---|---|
| Welcome / Continue watching | [light](gtk-welcome-light.png) | [dark](gtk-welcome-dark.png) | [`smoke-linux-empty-states.sh`](../../../scripts/smoke-linux-empty-states.sh) |
| History | [light](gtk-history-light.png) | [dark](gtk-history-dark.png) | [`smoke-linux-empty-states.sh`](../../../scripts/smoke-linux-empty-states.sh) |
| Settings, Appearance page (theme selector) | [light](gtk-settings-appearance-light.png) | [dark](gtk-settings-appearance-dark.png) | [`smoke-linux-settings.sh`](../../../scripts/smoke-linux-settings.sh) `appearance light` / `appearance dark` |
| Playback, paused and playing with controls | [paused](gtk-playback-paused.png) | [playing](gtk-playback-playing-controls.png) | [`smoke-linux-playback-chrome.sh`](../../../scripts/smoke-linux-playback-chrome.sh) |

The playback controls sit over the video, so they use the same translucent neutral
surface in both themes; only their accent marks (the seek fill and the volume level)
take a palette colour, the dark scheme's accent. Both playback captures use the
script's bright test frame. The script stops at a keyboard-traversal check before
it captures its dark frame. That check reads the interaction log, not colours.
