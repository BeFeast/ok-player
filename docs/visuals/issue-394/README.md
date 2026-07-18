# Issue 394 — persistent Linux Update / Skip surfaces

The player reference is the shipped 1120×680 Windows composition: one video/idle plane with
app-owned title chrome and floating material above content. The Settings reference is the existing
dedicated Linux Updates page, itself translated from the canonical 760×560 About/Settings design.
Issue #394 supplies the new state hierarchy and exact action labels; it does not replace the
established geometry, tokens, or native-control behavior.

## Redline accounting

| Dimension | Reference/accounting | Implementation |
| --- | --- | --- |
| Geometry | Player chrome floats over the content plane; Settings keeps the 760px window, 192px rail, 42px titlebar, and 500px content card | Persistent player card is top-centered at the titlebar seam, 534×74 in the 1120×680 capture; below 620px it reflows copy above actions and moves to an 8px top inset; Settings retains the existing shell/card geometry and natural 760×560 capture |
| Spacing | Canonical 8px rhythm, 14×16 card padding, 7–10px radii | Player card uses 14×16 padding, 16px copy/action gap, 8px action gap, 10px radius; Settings reuses the existing info-card and action-row classes |
| Type | Quiet 11–13px Settings/status hierarchy with 600-weight action labels | Target version is in the 13px semibold heading; detail/error copy is 11.5px; buttons are 12px semibold; Settings keeps its existing 12px wrapped status |
| Color/material | Floating controls use a dark legibility material over arbitrary video; Settings uses light/dark card and accent tokens | Player card uses the shipped dark translucent overlay recipe, one-step stroke/shadow, teal primary action, and solid High Contrast fallback; Settings introduces no new palette |
| Iconography | No new icon was specified; existing Settings Updates rail glyph remains the download-to-tray outline | The persistent card uses text hierarchy and explicit labels rather than inventing an update glyph; the rail icon is unchanged |
| Controls | Existing Updates page had one package-specific install action and Open Releases | Available state now exposes exact **Update** and **Skip this version** actions on both surfaces; skipped state exposes **Install anyway** in Settings; labels are native GTK button text and tooltips describe the actions |
| States | Existing page covered available/checking/check failure | Added persistent available, exact-version skipped, installing, install failure/retry, and installed transitions; failed install retains the same target and restores **Update** |
| Behavior | Long-lived app-owned utility surfaces are non-modal; OSD toasts are transient | The update card is a targeted overlay, not a dialog, and has no hide timer. It remains after the former 1.7s toast lifetime, disappears only after Update/Skip resolves the offer, and leaves the rest of the player interactive |

## Captures

- [Windows player composition reference](reference-player-windows-1120x680.png)
- [Existing Updates page reference](reference-settings-available-dark-760x503.png)
- [Player available · Light](player-available-light-1120x680.png)
- [Player available · Dark](player-available-dark-1120x680.png)
- [Player available · Narrow dark](player-available-dark-480x270.png)
- [Player install failure · Dark](player-install-error-dark-1120x680.png)
- [Settings available · Light](settings-available-light-760x560.png)
- [Settings skipped · Light](settings-skipped-light-760x560.png)
- [Settings install failure · Light](settings-install-error-light-760x560.png)

`scripts/smoke-linux-update-surface.sh` waits five seconds before every player capture, beyond the
old toast timeout, and checks the 1120×680 geometry plus a non-flat card crop. The Settings images
come from the existing deterministic Settings smoke preview path.

That Settings smoke currently captures the requested state successfully and then exits non-zero on
its pre-existing 760×360 resize assertion: the shared companion-window policy enforces a 760×480
minimum. This issue does not change either unrelated contract; the discrepancy remains explicit
rather than being hidden by weakening the check.

These are X11/Xvfb composition captures. They prove deterministic geometry, spacing, theme
material, labels, persistent rendering beyond the old timeout, and the rendered available/skipped/
failure states. They do not prove a real candidate feed, download progress, checksum verification,
pkexec, AppImage replacement, restart identity, live GNOME keyboard focus order, or screen-reader
announcement. Those remain the installed GNOME/Wayland operator gate documented in
`docs/linux-candidate-upgrade-acceptance.md`.
